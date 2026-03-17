use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use tracing::info_span;

use crate::command::{Command, Body};
use crate::http::{Request, RequestConfig, RequestResult};
use crate::load_curve::LoadCurve;
use crate::load_curve::executor::{CurveExecutor, CurveExecutorParams};
use crate::monitoring::SpanName;
use crate::request_template::Template;
use crate::response_template::extractor;
use crate::response_template::stats::ResponseStats;
use crate::response_template::ResponseTemplate;
use crate::sampling::{ReservoirAction, SamplingParams, SamplingState};

// ── ExecutionMode ─────────────────────────────────────────────────────────────

/// Indicates which execution strategy produced the run results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
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
    pub mode: ExecutionMode,
    /// Total curve duration (only meaningful when `mode == ExecutionMode::Curve`).
    pub curve_duration: Option<Duration>,
    /// Actual (unsampled) total request count.
    pub total_requests: usize,
    /// Actual (unsampled) failure count.
    pub total_failures: usize,
    /// Final VU-threshold sample rate at end of run (1.0 = no threshold sampling).
    pub sample_rate: f64,
    /// Lowest sample rate observed at any point during the run.
    pub min_sample_rate: f64,
}

// ── RunCommand ────────────────────────────────────────────────────────────────

pub struct RunCommand {
    pub host: String,
    pub request_count: usize,
    pub concurrency: usize,
    pub method: crate::command::HttpMethod,
    pub body: Option<Body>,
    pub template_path: Option<PathBuf>,
    pub response_template_path: Option<PathBuf>,
    pub load_curve: Option<LoadCurve>,
    /// VU count below which all results are collected (0 = disabled).
    pub sample_threshold: usize,
    /// Maximum results to retain for percentile computation.
    pub result_buffer: usize,
}

impl Command for RunCommand {
    async fn execute(self) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
        // Destructure to separate the curve from the rest of self so both
        // paths can receive the fields they need without a partial-move error.
        let RunCommand {
            host,
            request_count,
            concurrency,
            method,
            body,
            template_path,
            response_template_path,
            load_curve,
            sample_threshold,
            result_buffer,
        } = self;

        let base = RunCommand {
            host,
            request_count,
            concurrency,
            method,
            body,
            template_path,
            response_template_path,
            load_curve: None,
            sample_threshold,
            result_buffer,
        };

        match load_curve {
            Some(curve) => base.execute_curve(curve).await,
            None => base.execute_fixed().await,
        }
    }
}

impl RunCommand {
    /// Fixed-count semaphore-based execution path (original behaviour, unchanged).
    async fn execute_fixed(self) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
        let total = self.request_count;
        let concurrency = self.concurrency;

        // Pre-generate all template bodies before any requests fire
        let gen_start = Instant::now();
        let all_bodies: Option<Vec<String>> = self
            .template_path
            .map(|path| {
                let template = Template::parse(&path)?;
                let bodies = template.pre_generate(total);
                Ok::<Vec<String>, Box<dyn std::error::Error>>(bodies)
            })
            .transpose()?;
        let template_duration = all_bodies.as_ref().map(|_| gen_start.elapsed());

        let tracked_fields = self
            .response_template_path
            .map(|path| {
                ResponseTemplate::parse(&path)
                    .map(|rt| Arc::new(rt.fields))
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
            })
            .transpose()?;

        let request = Arc::new(RequestConfig {
            client: reqwest::Client::new(),
            host: Arc::new(self.host),
            method: self.method,
            body: Arc::new(self.body),
            tracked_fields,
        });

        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
            eprintln!("\nShutdown signal received — waiting for in-flight requests to finish...");
            cancel.cancel();
        });

        let started_at = Instant::now();

        let sample_threshold = self.sample_threshold;
        let result_buffer = self.result_buffer;

        let (all_results, sampling) = async {
            let sem = Arc::new(Semaphore::new(concurrency));
            let (tx, mut rx) = mpsc::channel::<RequestResult>(concurrency);

            for i in 0..total {
                let resolved = request.resolve_body(
                    all_bodies.as_ref().map(|bs| bs[i].clone()),
                );

                let client = request.client.clone();
                let url = request.host.as_str().to_string();
                let method = request.method;
                let capture_body = request.tracked_fields.is_some();
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
                            let _ = tx.send(req.execute().await).await;
                        });
                    }
                }
            }

            // Close the last sender — rx drains once all tasks have finished
            drop(tx);

            let mut sampling = SamplingState::new(SamplingParams {
                vu_threshold: sample_threshold,
                reservoir_size: result_buffer,
            });
            // In fixed mode the VU count is constant — set it once before draining.
            sampling.set_active_vus(concurrency);

            let mut results: Vec<RequestResult> = Vec::with_capacity(total.min(result_buffer));
            while let Some(result) = rx.recv().await {
                sampling.record_request(result.success);
                if sampling.should_collect() {
                    match sampling.reservoir_slot(results.len()) {
                        ReservoirAction::Push => results.push(result),
                        ReservoirAction::Replace(idx) => results[idx] = result,
                        ReservoirAction::Discard => {}
                    }
                }
            }
            (results, sampling)
        }
        .instrument(info_span!(SpanName::REQUESTS, total))
        .await;

        let response_stats = request.tracked_fields.as_ref().map(|fields| {
            let mut rs = ResponseStats::new();
            for result in &all_results {
                if let Some(ref body_str) = result.response_body {
                    if let Ok(body_val) = serde_json::from_str(body_str) {
                        rs.record(extractor::extract(&body_val, fields));
                    }
                }
            }
            rs
        });

        Ok(Some(RunStats {
            elapsed: started_at.elapsed(),
            template_duration,
            response_stats,
            results: all_results,
            mode: ExecutionMode::Fixed,
            curve_duration: None,
            total_requests: sampling.total_requests(),
            total_failures: sampling.total_failures(),
            sample_rate: sampling.sample_rate(),
            min_sample_rate: sampling.min_sample_rate(),
        }))
    }

    /// Curve-based dynamic VU execution path.
    async fn execute_curve(self, curve: LoadCurve) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
        let curve_duration = curve.total_duration();

        // Parse template for on-demand body generation (no pre-generation in curve mode)
        let template: Option<Arc<Template>> = self
            .template_path
            .map(|path| Template::parse(&path).map(Arc::new))
            .transpose()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        let tracked_fields = self
            .response_template_path
            .map(|path| {
                ResponseTemplate::parse(&path)
                    .map(|rt| Arc::new(rt.fields))
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
            })
            .transpose()?;

        let request_config = Arc::new(RequestConfig {
            client: reqwest::Client::new(),
            host: Arc::new(self.host),
            method: self.method,
            body: Arc::new(self.body),
            tracked_fields,
        });

        let cancellation_token = CancellationToken::new();
        let cancel = cancellation_token.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
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
                vu_threshold: self.sample_threshold,
                reservoir_size: self.result_buffer,
            },
        });

        let curve_result = executor.execute().await;

        let response_stats = request_config.tracked_fields.as_ref().map(|fields| {
            let mut rs = ResponseStats::new();
            for result in &curve_result.results {
                if let Some(ref body_str) = result.response_body {
                    if let Ok(body_val) = serde_json::from_str(body_str) {
                        rs.record(extractor::extract(&body_val, fields));
                    }
                }
            }
            rs
        });

        Ok(Some(RunStats {
            elapsed: started_at.elapsed(),
            template_duration: None,
            response_stats,
            results: curve_result.results,
            mode: ExecutionMode::Curve,
            curve_duration: Some(curve_duration),
            total_requests: curve_result.total_requests,
            total_failures: curve_result.total_failures,
            sample_rate: curve_result.sample_rate,
            min_sample_rate: curve_result.min_sample_rate,
        }))
    }
}
