use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::execution::StageStats;
use crate::histogram::{LatencyHistogram, StatusCodeHistogram};
use crate::http::{RequestConfig, RequestRecord};
use crate::load_curve::LoadCurve;
use crate::request_template::Template;
use crate::response_template::stats::ResponseStats;
use crate::vu::Vu;

// ── CurveExecutorParams ───────────────────────────────────────────────────────

/// Parameters for constructing a `CurveExecutor`.
pub struct CurveExecutorParams {
    pub curve: LoadCurve,
    pub request_config: Arc<RequestConfig>,
    pub template: Option<Arc<Template>>,
    pub cancellation_token: CancellationToken,
}

// ── CurveExecutionResult ──────────────────────────────────────────────────────

/// Result returned by `CurveExecutor::execute`.
pub struct CurveExecutionResult {
    pub latency: LatencyHistogram,
    pub status_codes: StatusCodeHistogram,
    pub total_requests: u64,
    pub total_failures: u64,
    pub response_stats: Option<ResponseStats>,
    pub stage_stats: Vec<StageStats>,
}

// ── stage_index_at ────────────────────────────────────────────────────────────

/// Returns the 0-based stage index for a given elapsed duration.
fn stage_index_at(stages: &[crate::load_curve::Stage], elapsed: Duration) -> usize {
    let mut offset = Duration::ZERO;
    for (i, stage) in stages.iter().enumerate() {
        offset += stage.duration;
        if elapsed < offset {
            return i;
        }
    }
    stages.len().saturating_sub(1)
}

// ── CurveExecutor ─────────────────────────────────────────────────────────────

/// Executes a load test driven by a `LoadCurve`, dynamically scaling VUs.
pub struct CurveExecutor {
    params: CurveExecutorParams,
}

impl CurveExecutor {
    pub fn new(params: CurveExecutorParams) -> Self {
        Self { params }
    }

    /// Runs the load curve, spawning and cancelling VU tasks as the curve
    /// dictates. Returns a `CurveExecutionResult` when the curve completes or a
    /// cancellation signal is received.
    pub async fn execute(self) -> CurveExecutionResult {
        let CurveExecutorParams {
            curve,
            request_config,
            template,
            cancellation_token,
        } = self.params;

        let total_duration = curve.total_duration();
        let started_at = Instant::now();

        // Pre-convert headers once before spawning any VUs to avoid per-VU allocation.
        let plain_headers: Arc<Vec<(String, String)>> = Arc::new(
            request_config
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
        );

        // Unbounded channel; VUs push results as they complete without risk of blocking.
        let (tx, mut rx) = mpsc::unbounded_channel::<RequestRecord>();

        // Track active VU handles and their per-VU cancellation tokens.
        let mut vu_handles: Vec<(JoinHandle<()>, CancellationToken)> = Vec::new();

        let has_tracked_fields = request_config.tracked_fields.is_some();
        let n_stages = curve.stages.len();

        let mut latency = LatencyHistogram::new();
        let mut status_codes = StatusCodeHistogram::new();
        let mut total_requests: u64 = 0;
        let mut total_failures: u64 = 0;
        let mut response_stats: Option<ResponseStats> = if has_tracked_fields {
            Some(ResponseStats::new())
        } else {
            None
        };

        // Pre-allocate per-stage histograms.
        let mut stage_stats: Vec<StageStats> = (0..n_stages)
            .map(|_| StageStats {
                latency: LatencyHistogram::new(),
                status_codes: StatusCodeHistogram::new(),
                total_requests: 0,
                total_failures: 0,
            })
            .collect();

        let mut ticker = tokio::time::interval(tokio::time::Duration::from_millis(100));

        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    debug!("curve executor: parent cancellation received");
                    break;
                }
                _ = ticker.tick() => {
                    let elapsed = started_at.elapsed();

                    if elapsed >= total_duration {
                        debug!("curve executor: total duration elapsed, shutting down");
                        break;
                    }

                    let target = curve.target_vus_at(elapsed) as usize;
                    let current = vu_handles.len();

                    match target.cmp(&current) {
                        std::cmp::Ordering::Greater => {
                            // Spawn additional VUs
                            let to_add = target - current;
                            for _ in 0..to_add {
                                let vu_token = CancellationToken::new();
                                let handle = Vu {
                                    request_config: Arc::clone(&request_config),
                                    plain_headers: Arc::clone(&plain_headers),
                                    template: template.as_ref().map(Arc::clone),
                                    cancellation_token: vu_token.clone(),
                                    result_tx: tx.clone(),
                                    budget: None,
                                }
                                .spawn();
                                vu_handles.push((handle, vu_token));
                            }
                        }
                        std::cmp::Ordering::Less => {
                            // Cancel excess VUs (cancel from the end of the list)
                            let to_remove = current - target;
                            let drain_start = vu_handles.len() - to_remove;
                            let excess: Vec<_> = vu_handles.drain(drain_start..).collect();
                            // Cancel all tokens first so all VUs begin exiting simultaneously
                            for (_, token) in &excess {
                                token.cancel();
                            }
                            // Await sequentially — VUs are already exiting in parallel on the runtime
                            for (handle, _) in excess {
                                let _ = handle.await;
                            }
                        }
                        std::cmp::Ordering::Equal => {}
                    }

                    // Drain all results currently in the channel without blocking.
                    let stage_idx = stage_index_at(&curve.stages, elapsed);
                    while let Ok(record) = rx.try_recv() {
                        total_requests += 1;
                        if !record.success {
                            total_failures += 1;
                        }
                        latency.record(record.duration);
                        status_codes.record(record.status_code);
                        stage_stats[stage_idx].latency.record(record.duration);
                        stage_stats[stage_idx].status_codes.record(record.status_code);
                        stage_stats[stage_idx].total_requests += 1;
                        if !record.success {
                            stage_stats[stage_idx].total_failures += 1;
                        }
                        if let Some(extraction) = record.extraction {
                            if let Some(ref mut rs) = response_stats {
                                rs.record(extraction);
                            }
                        }
                    }
                }
            }
        }

        // Cancel all remaining VU tasks — cancel all tokens first, then await
        for (_, token) in &vu_handles {
            token.cancel();
        }
        for (handle, _) in vu_handles {
            let _ = handle.await;
        }

        // Drop the coordinator's sender so the channel closes once all VU
        // senders (clones) are also dropped (they are, since tasks ended).
        drop(tx);

        // Final drain: collect any results that arrived between the last tick
        // and the VU tasks completing.
        let final_elapsed = started_at.elapsed();
        let final_stage_idx = stage_index_at(&curve.stages, final_elapsed);
        while let Some(record) = rx.recv().await {
            total_requests += 1;
            if !record.success {
                total_failures += 1;
            }
            latency.record(record.duration);
            status_codes.record(record.status_code);
            stage_stats[final_stage_idx].latency.record(record.duration);
            stage_stats[final_stage_idx].status_codes.record(record.status_code);
            stage_stats[final_stage_idx].total_requests += 1;
            if !record.success {
                stage_stats[final_stage_idx].total_failures += 1;
            }
            if let Some(extraction) = record.extraction {
                if let Some(ref mut rs) = response_stats {
                    rs.record(extraction);
                }
            }
        }

        CurveExecutionResult {
            latency,
            status_codes,
            total_requests,
            total_failures,
            response_stats,
            stage_stats,
        }
    }
}
