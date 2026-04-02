use std::sync::Arc;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::command::Command;
use crate::execution::{
    ExecutionMode, RequestSpec, RunMode, RunStats, SamplingConfig,
    build_request_config, compute_response_stats, resolve_tracked_fields,
};
use crate::execution::curve::{CurveExecutor, CurveExecutorParams};
use crate::execution::fixed::{FixedExecutor, FixedExecutorParams};
use crate::load_curve::LoadCurve;
use crate::request_template::Template;
use crate::sampling::SamplingParams;

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

// ── execute_fixed ─────────────────────────────────────────────────────────────

/// Fixed-count semaphore-based execution path.
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

    // Parse template for on-demand body generation (no pre-generation).
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
        eprintln!("\nShutdown signal received — waiting for in-flight requests to finish...");
        cancel.cancel();
    });

    let started_at = Instant::now();

    let result = FixedExecutor::new(FixedExecutorParams {
        request_config: Arc::clone(&request_config),
        template,
        total,
        concurrency,
        cancellation_token,
        sampling: SamplingParams {
            vu_threshold: sampling.sample_threshold,
            reservoir_size: sampling.result_buffer,
        },
    })
    .execute()
    .await;

    let response_stats = compute_response_stats(&result.results, &request_config.tracked_fields);

    Ok(Some(RunStats {
        elapsed: started_at.elapsed(),
        template_duration: None,
        response_stats,
        results: result.results,
        mode: RunMode::Fixed,
        curve_duration: None,
        curve_stages: None,
        total_requests: result.total_requests,
        total_failures: result.total_failures,
        sample_rate: result.sample_rate,
        min_sample_rate: result.min_sample_rate,
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

    use crate::execution::{RunMode, RunStats};
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
