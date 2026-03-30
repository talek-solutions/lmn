use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use tracing::info_span;

use crate::command::{Body, Command};
use crate::config::secret::SensitiveString;
use crate::http::{Request, RequestConfig, RequestResult};
use crate::load_curve::LoadCurve;
use crate::load_curve::executor::{CurveExecutor, CurveExecutorParams};
use crate::monitoring::SpanName;
use crate::request_template::Template;
use crate::response_template::ResponseTemplate;
use crate::response_template::extractor;
use crate::response_template::field::TrackedField;
use crate::response_template::stats::ResponseStats;
use crate::sampling::{ReservoirAction, SamplingParams, SamplingState};

// ── RunMode ───────────────────────────────────────────────────────────────────

/// Indicates which execution strategy produced the run results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Classic semaphore-based fixed-count mode.
    Fixed,
    /// Time-based dynamic VU mode driven by a `LoadCurve`.
    Curve,
}

// ── RunStats ──────────────────────────────────────────────────────────────────

pub struct RunStats {
    pub elapsed: Duration,
    pub template_duration: Option<Duration>,
    pub response_stats: Option<ResponseStats>,
    pub results: Vec<RequestResult>,
    pub mode: RunMode,
    /// Total curve duration (only meaningful when `mode == RunMode::Curve`).
    pub curve_duration: Option<Duration>,
    /// Curve stages captured from the `LoadCurve` after execution.
    /// `Some` only when `mode == RunMode::Curve`.
    pub curve_stages: Option<Vec<crate::load_curve::Stage>>,
    /// Actual (unsampled) total request count.
    pub total_requests: usize,
    /// Actual (unsampled) failure count.
    pub total_failures: usize,
    /// Final VU-threshold sample rate at end of run (1.0 = no threshold sampling).
    pub sample_rate: f64,
    /// Lowest sample rate observed at any point during the run.
    pub min_sample_rate: f64,
}

// ── RequestSpec ───────────────────────────────────────────────────────────────

/// All request-level parameters for a run.
pub struct RequestSpec {
    pub host: String,
    pub method: crate::command::HttpMethod,
    pub body: Option<Body>,
    pub template_path: Option<PathBuf>,
    pub response_template_path: Option<PathBuf>,
    /// Custom HTTP headers to send with every request in this run.
    pub headers: Vec<(String, SensitiveString)>,
}

// ── SamplingConfig ────────────────────────────────────────────────────────────

/// Sampling and reservoir parameters for a run.
pub struct SamplingConfig {
    pub sample_threshold: usize,
    pub result_buffer: usize,
}

// ── ExecutionMode ─────────────────────────────────────────────────────────────

/// Determines the execution strategy for a run.
pub enum ExecutionMode {
    /// Classic semaphore-based fixed-count execution.
    Fixed {
        request_count: usize,
        concurrency: usize,
    },
    /// Time-based dynamic VU execution driven by a `LoadCurve`.
    Curve(LoadCurve),
}

// ── RunCommand ────────────────────────────────────────────────────────────────

pub struct RunCommand {
    pub request: RequestSpec,
    pub execution: ExecutionMode,
    pub sampling: SamplingConfig,
}

impl Command for RunCommand {
    async fn execute(self) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
        match self.execution {
            ExecutionMode::Fixed {
                request_count,
                concurrency,
            } => execute_fixed(self.request, self.sampling, request_count, concurrency).await,
            ExecutionMode::Curve(curve) => execute_curve(self.request, self.sampling, curve).await,
        }
    }
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn resolve_tracked_fields(
    path: Option<PathBuf>,
) -> Result<Option<Arc<Vec<TrackedField>>>, Box<dyn std::error::Error>> {
    path.map(|p| {
        ResponseTemplate::parse(&p)
            .map(|rt| Arc::new(rt.fields))
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    })
    .transpose()
}

fn build_request_config(
    host: String,
    method: crate::command::HttpMethod,
    body: Option<Body>,
    tracked_fields: Option<Arc<Vec<TrackedField>>>,
    headers: Vec<(String, SensitiveString)>,
) -> Arc<RequestConfig> {
    Arc::new(RequestConfig {
        client: reqwest::Client::new(),
        host: Arc::new(host),
        method,
        body: Arc::new(body),
        tracked_fields,
        headers: Arc::new(headers),
    })
}

fn compute_response_stats(
    results: &[RequestResult],
    tracked_fields: &Option<Arc<Vec<TrackedField>>>,
) -> Option<ResponseStats> {
    tracked_fields.as_ref().map(|fields| {
        let mut rs = ResponseStats::new();
        for result in results {
            if let Some(ref body_str) = result.response_body
                && let Ok(body_val) = serde_json::from_str(body_str)
            {
                rs.record(extractor::extract(&body_val, fields));
            }
        }
        rs
    })
}

// ── execute_fixed ─────────────────────────────────────────────────────────────

/// Fixed-count semaphore-based execution path (original behaviour, unchanged).
async fn execute_fixed(
    request_spec: RequestSpec,
    sampling: SamplingConfig,
    total: usize,
    concurrency: usize,
) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
    let RequestSpec {
        host,
        method,
        body,
        template_path,
        response_template_path,
        headers,
    } = request_spec;

    // Pre-generate all template bodies before any requests fire
    let gen_start = Instant::now();
    let all_bodies: Option<Vec<String>> = template_path
        .map(|path| {
            let template = Template::parse(&path)?;
            let bodies = template.pre_generate(total);
            Ok::<Vec<String>, Box<dyn std::error::Error>>(bodies)
        })
        .transpose()?;
    let template_duration = all_bodies.as_ref().map(|_| gen_start.elapsed());

    let tracked_fields = resolve_tracked_fields(response_template_path)?;
    let request = build_request_config(host, method, body, tracked_fields, headers);

    let token = CancellationToken::new();
    let cancel = token.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl_c");
        eprintln!("\nShutdown signal received — waiting for in-flight requests to finish...");
        cancel.cancel();
    });

    let started_at = Instant::now();

    let sample_threshold = sampling.sample_threshold;
    let result_buffer = sampling.result_buffer;

    // Pre-convert headers once before the hot loop to avoid per-request allocation.
    let plain_headers: Arc<Vec<(String, String)>> = Arc::new(
        request
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect(),
    );

    let (all_results, sampling_state) = async {
        let sem = Arc::new(Semaphore::new(concurrency));
        let (tx, mut rx) = mpsc::channel::<RequestResult>(concurrency);

        for i in 0..total {
            let resolved = request.resolve_body(all_bodies.as_ref().map(|bs| bs[i].clone()));

            let client = request.client.clone();
            let url = request.host.as_str().to_string();
            let method = request.method;
            let capture_body = request.tracked_fields.is_some();
            let headers = Arc::clone(&plain_headers);
            let tx = tx.clone();

            tokio::select! {
                _ = token.cancelled() => break,
                permit = sem.clone().acquire_owned() => {
                    let permit = permit.unwrap();
                    tokio::spawn(async move {
                        let _permit = permit;
                        let mut req = Request::new(client, url, method);
                        if let Some((content, content_type)) = resolved {
                            req = req.body(content, content_type);
                        }
                        if capture_body {
                            req = req.read_response();
                        }
                        if !headers.is_empty() {
                            req = req.headers(headers);
                        }
                        let _ = tx.send(req.execute().await).await;
                    });
                }
            }
        }

        // Close the last sender — rx drains once all tasks have finished
        drop(tx);

        let mut sampling_state = SamplingState::new(SamplingParams {
            vu_threshold: sample_threshold,
            reservoir_size: result_buffer,
        });
        // In fixed mode the VU count is constant — set it once before draining.
        sampling_state.set_active_vus(concurrency);

        let mut results: Vec<RequestResult> = Vec::with_capacity(total.min(result_buffer));
        while let Some(result) = rx.recv().await {
            sampling_state.record_request(result.success);
            if sampling_state.should_collect() {
                match sampling_state.reservoir_slot(results.len()) {
                    ReservoirAction::Push => results.push(result),
                    ReservoirAction::Replace(idx) => results[idx] = result,
                    ReservoirAction::Discard => {}
                }
            }
        }
        (results, sampling_state)
    }
    .instrument(info_span!(SpanName::REQUESTS, total))
    .await;

    let response_stats = compute_response_stats(&all_results, &request.tracked_fields);

    Ok(Some(RunStats {
        elapsed: started_at.elapsed(),
        template_duration,
        response_stats,
        results: all_results,
        mode: RunMode::Fixed,
        curve_duration: None,
        curve_stages: None,
        total_requests: sampling_state.total_requests(),
        total_failures: sampling_state.total_failures(),
        sample_rate: sampling_state.sample_rate(),
        min_sample_rate: sampling_state.min_sample_rate(),
    }))
}

// ── execute_curve ─────────────────────────────────────────────────────────────

/// Curve-based dynamic VU execution path.
async fn execute_curve(
    request_spec: RequestSpec,
    sampling: SamplingConfig,
    curve: LoadCurve,
) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
    let RequestSpec {
        host,
        method,
        body,
        template_path,
        response_template_path,
        headers,
    } = request_spec;
    let curve_duration = curve.total_duration();
    let curve_stages = curve.stages.clone();

    // Parse template for on-demand body generation (no pre-generation in curve mode)
    let template: Option<Arc<Template>> = template_path
        .map(|path| Template::parse(&path).map(Arc::new))
        .transpose()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    let tracked_fields = resolve_tracked_fields(response_template_path)?;
    let request_config = build_request_config(host, method, body, tracked_fields, headers);

    let cancellation_token = CancellationToken::new();
    let cancel = cancellation_token.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl_c");
        eprintln!("\nShutdown signal received — cancelling curve execution...");
        cancel.cancel();
    });

    let started_at = Instant::now();

    let executor = CurveExecutor::new(CurveExecutorParams {
        curve,
        request_config: Arc::clone(&request_config),
        template,
        cancellation_token,
        sampling: SamplingParams {
            vu_threshold: sampling.sample_threshold,
            reservoir_size: sampling.result_buffer,
        },
    });

    let curve_result = executor.execute().await;

    let response_stats =
        compute_response_stats(&curve_result.results, &request_config.tracked_fields);

    Ok(Some(RunStats {
        elapsed: started_at.elapsed(),
        template_duration: None,
        response_stats,
        results: curve_result.results,
        mode: RunMode::Curve,
        curve_duration: Some(curve_duration),
        curve_stages: Some(curve_stages),
        total_requests: curve_result.total_requests,
        total_failures: curve_result.total_failures,
        sample_rate: curve_result.sample_rate,
        min_sample_rate: curve_result.min_sample_rate,
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{RunMode, RunStats};
    use crate::load_curve::{RampType, Stage};

    fn make_stats_fixed() -> RunStats {
        RunStats {
            elapsed: Duration::from_secs(1),
            template_duration: None,
            response_stats: None,
            results: vec![],
            mode: RunMode::Fixed,
            curve_duration: None,
            curve_stages: None,
            total_requests: 10,
            total_failures: 0,
            sample_rate: 1.0,
            min_sample_rate: 1.0,
        }
    }

    fn make_stats_curve(stages: Vec<Stage>) -> RunStats {
        RunStats {
            elapsed: Duration::from_secs(10),
            template_duration: None,
            response_stats: None,
            results: vec![],
            mode: RunMode::Curve,
            curve_duration: Some(Duration::from_secs(10)),
            curve_stages: Some(stages),
            total_requests: 100,
            total_failures: 2,
            sample_rate: 1.0,
            min_sample_rate: 1.0,
        }
    }

    // ── curve_stages_none_for_fixed_mode ──────────────────────────────────────

    #[test]
    fn curve_stages_none_for_fixed_mode() {
        let stats = make_stats_fixed();
        assert!(
            stats.curve_stages.is_none(),
            "fixed-mode RunStats must have curve_stages == None"
        );
    }

    // ── curve_stages_some_for_curve_mode ──────────────────────────────────────

    #[test]
    fn curve_stages_some_for_curve_mode() {
        let stages = vec![
            Stage {
                duration: Duration::from_secs(5),
                target_vus: 50,
                ramp: RampType::Linear,
            },
            Stage {
                duration: Duration::from_secs(5),
                target_vus: 100,
                ramp: RampType::Step,
            },
        ];
        let stats = make_stats_curve(stages.clone());

        let stored = stats
            .curve_stages
            .expect("curve_stages must be Some in curve mode");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].target_vus, 50);
        assert_eq!(stored[0].ramp, RampType::Linear);
        assert_eq!(stored[1].target_vus, 100);
        assert_eq!(stored[1].ramp, RampType::Step);
    }

    // ── curve_stages_count_matches_original ───────────────────────────────────

    #[test]
    fn curve_stages_count_matches_original() {
        let stages: Vec<Stage> = (0..5)
            .map(|i| Stage {
                duration: Duration::from_secs(10),
                target_vus: (i + 1) * 20,
                ramp: RampType::Linear,
            })
            .collect();
        let count = stages.len();
        let stats = make_stats_curve(stages);
        assert_eq!(
            stats.curve_stages.unwrap().len(),
            count,
            "stored stage count must equal original stage count"
        );
    }

    // ── run_mode_fixed_variant ────────────────────────────────────────────────

    #[test]
    fn run_mode_fixed_variant() {
        let stats = make_stats_fixed();
        assert_eq!(stats.mode, RunMode::Fixed);
    }

    // ── run_mode_curve_variant ────────────────────────────────────────────────

    #[test]
    fn run_mode_curve_variant() {
        let stages = vec![Stage {
            duration: Duration::from_secs(5),
            target_vus: 10,
            ramp: RampType::Linear,
        }];
        let stats = make_stats_curve(stages);
        assert_eq!(stats.mode, RunMode::Curve);
    }
}
