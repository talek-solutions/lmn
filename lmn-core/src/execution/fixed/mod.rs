use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use tracing::info_span;

use crate::execution::SamplingStats;
use crate::http::{RequestConfig, RequestResult};
use crate::monitoring::SpanName;
use crate::request_template::Template;
use crate::sampling::{ReservoirAction, SamplingParams, SamplingState};
use crate::vu::Vu;

// ── FixedExecutorParams ───────────────────────────────────────────────────────

/// Parameters for constructing a `FixedExecutor`.
pub struct FixedExecutorParams {
    pub request_config: Arc<RequestConfig>,
    pub template: Option<Arc<Template>>,
    pub total: usize,
    pub concurrency: usize,
    pub cancellation_token: CancellationToken,
    pub sampling: SamplingParams,
}

// ── FixedExecutionResult ──────────────────────────────────────────────────────

/// Result returned by `FixedExecutor::execute`. Carries the reservoir-bounded
/// sample of results plus the sampling counters for `RunStats`.
pub struct FixedExecutionResult {
    pub results: Vec<RequestResult>,
    pub sampling_stats: SamplingStats,
}

// ── FixedExecutor ─────────────────────────────────────────────────────────────

/// Executes a fixed-count load test using a worker-pool model: spawns exactly
/// `concurrency` long-lived VU tasks that share an atomic request budget and
/// self-terminate when the budget is exhausted.
pub struct FixedExecutor {
    params: FixedExecutorParams,
}

impl FixedExecutor {
    pub fn new(params: FixedExecutorParams) -> Self {
        Self { params }
    }

    /// Runs the fixed load test. Spawns `concurrency` VU tasks sharing a budget
    /// of `total` requests. Applies VU-threshold + reservoir sampling to bound
    /// memory usage. Returns a `FixedExecutionResult` when all requests complete
    /// or a cancellation signal is received.
    pub async fn execute(self) -> FixedExecutionResult {
        let FixedExecutorParams {
            request_config,
            template,
            total,
            concurrency,
            cancellation_token,
            sampling,
        } = self.params;

        // Pre-convert headers once before spawning VUs to avoid per-request allocation.
        let plain_headers: Arc<Vec<(String, String)>> = Arc::new(
            request_config
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
        );

        let (all_results, sampling_state) = async {
            let budget = Arc::new(AtomicUsize::new(total));
            let (tx, mut rx) = mpsc::unbounded_channel::<RequestResult>();

            // Spawn exactly `concurrency` VU tasks. Each claims requests from the
            // shared budget and self-terminates when the budget is exhausted.
            let vu_handles: Vec<_> = (0..concurrency)
                .map(|_| {
                    Vu {
                        request_config: Arc::clone(&request_config),
                        plain_headers: Arc::clone(&plain_headers),
                        template: template.as_ref().map(Arc::clone),
                        cancellation_token: cancellation_token.clone(),
                        result_tx: tx.clone(),
                        budget: Some(Arc::clone(&budget)),
                    }
                    .spawn()
                })
                .collect();

            // Drop the coordinator's sender so the channel closes once all VU
            // senders are also dropped (they are, once each VU task exits).
            drop(tx);

            let reservoir_size = sampling.reservoir_size;
            let mut sampling_state = SamplingState::new(sampling);
            // VU count is constant for the duration of a fixed run.
            sampling_state.set_active_vus(concurrency);

            let mut results: Vec<RequestResult> = Vec::with_capacity(total.min(reservoir_size));
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

            // VUs have exited (their senders dropped, closing the channel).
            // Await handles to ensure all task resources are released.
            for handle in vu_handles {
                let _ = handle.await;
            }

            (results, sampling_state)
        }
        .instrument(info_span!(SpanName::REQUESTS, total))
        .await;

        FixedExecutionResult {
            results: all_results,
            sampling_stats: SamplingStats {
                total_requests: sampling_state.total_requests(),
                total_failures: sampling_state.total_failures(),
                sample_rate: sampling_state.sample_rate(),
                min_sample_rate: sampling_state.min_sample_rate(),
            },
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── struct_shape_fixed_execution_result ───────────────────────────────────

    #[test]
    fn struct_shape_fixed_execution_result() {
        let result = FixedExecutionResult {
            results: vec![],
            sampling_stats: SamplingStats {
                total_requests: 10,
                total_failures: 1,
                sample_rate: 1.0,
                min_sample_rate: 0.8,
            },
        };
        assert_eq!(result.sampling_stats.total_requests, 10);
        assert_eq!(result.sampling_stats.total_failures, 1);
        assert_eq!(result.sampling_stats.sample_rate, 1.0);
        assert_eq!(result.sampling_stats.min_sample_rate, 0.8);
        assert!(result.results.is_empty());
    }

    // ── struct_shape_fixed_executor_params ────────────────────────────────────

    #[test]
    fn struct_shape_fixed_executor_params() {
        use crate::command::HttpMethod;
        use crate::http::RequestConfig;
        use crate::sampling::SamplingParams;
        use tokio_util::sync::CancellationToken;

        let config = Arc::new(RequestConfig {
            client: reqwest::Client::new(),
            host: Arc::new("http://localhost".to_string()),
            method: HttpMethod::Get,
            body: Arc::new(None),
            tracked_fields: None,
            headers: Arc::new(vec![]),
        });

        let params = FixedExecutorParams {
            request_config: Arc::clone(&config),
            template: None,
            total: 5,
            concurrency: 2,
            cancellation_token: CancellationToken::new(),
            sampling: SamplingParams {
                vu_threshold: 100,
                reservoir_size: 10_000,
            },
        };

        assert_eq!(params.total, 5);
        assert_eq!(params.concurrency, 2);
        assert!(params.template.is_none());
    }

    // ── fixed_executor_new_stores_params ─────────────────────────────────────

    #[test]
    fn fixed_executor_new_stores_params() {
        use crate::command::HttpMethod;
        use crate::http::RequestConfig;
        use crate::sampling::SamplingParams;
        use tokio_util::sync::CancellationToken;

        let config = Arc::new(RequestConfig {
            client: reqwest::Client::new(),
            host: Arc::new("http://localhost".to_string()),
            method: HttpMethod::Get,
            body: Arc::new(None),
            tracked_fields: None,
            headers: Arc::new(vec![]),
        });

        let executor = FixedExecutor::new(FixedExecutorParams {
            request_config: config,
            template: None,
            total: 1,
            concurrency: 1,
            cancellation_token: CancellationToken::new(),
            sampling: SamplingParams {
                vu_threshold: 100,
                reservoir_size: 10_000,
            },
        });

        assert_eq!(executor.params.total, 1);
        assert_eq!(executor.params.concurrency, 1);
    }
}
