use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::http::{Request, RequestConfig, RequestResult};
use crate::request_template::Template;
use crate::sampling::{ReservoirAction, SamplingParams, SamplingState};

use super::LoadCurve;

// ── CurveExecutorParams ───────────────────────────────────────────────────────

/// Parameters for constructing a `CurveExecutor`.
pub struct CurveExecutorParams {
    pub curve: LoadCurve,
    pub request_config: Arc<RequestConfig>,
    pub template: Option<Arc<Template>>,
    pub cancellation_token: CancellationToken,
    pub sampling: SamplingParams,
}

// ── CurveExecutionResult ──────────────────────────────────────────────────────

/// Result returned by `CurveExecutor::execute`. Carries the reservoir-bounded
/// sample of results plus the four sampling counters for `RunStats`.
pub struct CurveExecutionResult {
    pub results: Vec<RequestResult>,
    pub total_requests: usize,
    pub total_failures: usize,
    pub sample_rate: f64,
    pub min_sample_rate: f64,
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
    /// dictates. Applies VU-threshold + reservoir sampling to bound memory
    /// usage. Returns a `CurveExecutionResult` when the curve completes or a
    /// cancellation signal is received.
    pub async fn execute(self) -> CurveExecutionResult {
        let CurveExecutorParams { curve, request_config, template, cancellation_token, sampling } =
            self.params;

        let total_duration = curve.total_duration();
        let started_at = Instant::now();

        // Pre-convert headers once before spawning any VUs to avoid per-VU allocation.
        let plain_headers: Arc<Vec<(String, String)>> = Arc::new(
            request_config.headers
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect()
        );

        // Unbounded channel; VUs push results as they complete without risk of blocking.
        let (tx, mut rx) = mpsc::unbounded_channel::<RequestResult>();

        // Track active VU handles and their per-VU cancellation tokens.
        let mut vu_handles: Vec<(JoinHandle<()>, CancellationToken)> = Vec::new();

        let mut sampling = SamplingState::new(sampling);
        let mut results: Vec<RequestResult> = Vec::new();

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

                    if target > current {
                        // Spawn additional VUs
                        let to_add = target - current;
                        for _ in 0..to_add {
                            let vu_token = CancellationToken::new();
                            let handle = spawn_vu(VuParams {
                                request_config: Arc::clone(&request_config),
                                plain_headers: Arc::clone(&plain_headers),
                                template: template.as_ref().map(Arc::clone),
                                cancellation_token: vu_token.clone(),
                                result_tx: tx.clone(),
                            });
                            vu_handles.push((handle, vu_token));
                        }
                    } else if target < current {
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
                    // If target == current: nothing to do

                    // Update sampling rate based on the current active VU count.
                    sampling.set_active_vus(vu_handles.len());

                    // Drain all results currently in the channel without blocking.
                    // This prevents channel backpressure from inflating latency at
                    // high throughput — a correctness fix independent of sampling.
                    while let Ok(result) = rx.try_recv() {
                        sampling.record_request(result.success);
                        if sampling.should_collect() {
                            match sampling.reservoir_slot(results.len()) {
                                ReservoirAction::Push => results.push(result),
                                ReservoirAction::Replace(idx) => results[idx] = result,
                                ReservoirAction::Discard => {}
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

        CurveExecutionResult {
            results,
            total_requests: sampling.total_requests(),
            total_failures: sampling.total_failures(),
            sample_rate: sampling.sample_rate(),
            min_sample_rate: sampling.min_sample_rate(),
        }
    }
}

// ── VU task ───────────────────────────────────────────────────────────────────

struct VuParams {
    request_config: Arc<RequestConfig>,
    /// Pre-converted header pairs shared across all VUs — avoids per-request allocation.
    plain_headers: Arc<Vec<(String, String)>>,
    template: Option<Arc<Template>>,
    cancellation_token: CancellationToken,
    result_tx: mpsc::UnboundedSender<RequestResult>,
}

fn spawn_vu(params: VuParams) -> JoinHandle<()> {
    tokio::spawn(async move {
        let VuParams { request_config, plain_headers, template, cancellation_token, result_tx } = params;

        loop {
            // Generate body on demand for this request
            let body = template.as_ref().map(|t| t.generate_one());

            let resolved = request_config.resolve_body(body);

            let client = request_config.client.clone();
            let url = request_config.host.as_str().to_string();
            let method = request_config.method;
            let capture_body = request_config.tracked_fields.is_some();

            // Clone the Arc cheaply; dereference to get a Vec clone only when needed.
            let headers = Arc::clone(&plain_headers);

            let result_fut = async {
                let mut req = Request::new(client, url, method);
                if let Some((content, content_type)) = resolved {
                    req = req.body(content, content_type);
                }
                if capture_body {
                    req = req.read_response();
                }
                if !headers.is_empty() {
                    req = req.headers((*headers).clone());
                }
                req.execute().await
            };

            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    break;
                }
                result = result_fut => {
                    // Best-effort send — if receiver is gone, we stop
                    if result_tx.send(result).is_err() {
                        break;
                    }
                }
            }
        }
    })
}
