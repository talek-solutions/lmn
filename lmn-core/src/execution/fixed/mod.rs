use std::sync::Arc;

use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use tracing::info_span;

use crate::http::{Request, RequestConfig, RequestResult};
use crate::monitoring::SpanName;
use crate::request_template::Template;
use crate::sampling::{ReservoirAction, SamplingParams, SamplingState};

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
/// sample of results plus the four sampling counters for `RunStats`.
pub struct FixedExecutionResult {
    pub results: Vec<RequestResult>,
    pub total_requests: usize,
    pub total_failures: usize,
    pub sample_rate: f64,
    pub min_sample_rate: f64,
}

// ── FixedExecutor ─────────────────────────────────────────────────────────────

/// Executes a fixed-count load test using a semaphore-bounded concurrency model.
pub struct FixedExecutor {
    params: FixedExecutorParams,
}

impl FixedExecutor {
    pub fn new(params: FixedExecutorParams) -> Self {
        Self { params }
    }

    /// Runs the fixed load test, dispatching exactly `total` requests at up to
    /// `concurrency` in-flight at a time. Applies VU-threshold + reservoir
    /// sampling to bound memory usage. Returns a `FixedExecutionResult` when
    /// all requests complete or a cancellation signal is received.
    pub async fn execute(self) -> FixedExecutionResult {
        let FixedExecutorParams {
            request_config,
            template,
            total,
            concurrency,
            cancellation_token,
            sampling,
        } = self.params;

        // Pre-convert headers once before the hot loop to avoid per-request allocation.
        let plain_headers: Arc<Vec<(String, String)>> = Arc::new(
            request_config
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
        );

        let (all_results, sampling_state) = async {
            let sem = Arc::new(Semaphore::new(concurrency));
            let (tx, mut rx) = mpsc::channel::<RequestResult>(concurrency);

            let method = Arc::new(request_config.method);

            for _ in 0..total {
                // Generate body on demand for this request — no pre-generation.
                let body = template.as_ref().map(|t| t.generate_one());
                let resolved = request_config.resolve_body(body);

                let client = request_config.client.clone();
                let capture_body = request_config.tracked_fields.is_some();
                let headers = Arc::clone(&plain_headers);
                let url = Arc::clone(&request_config.host).to_string();
                let method_clone = Arc::clone(&method);

                let tx = tx.clone();

                tokio::select! {
                    _ = cancellation_token.cancelled() => break,
                    permit = sem.clone().acquire_owned() => {
                        let permit = permit.unwrap();

                        tokio::spawn(async move {
                            let _permit = permit;
                            let mut req = Request::new(
                                client,
                                url,
                                *method_clone
                            );
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

            // Close the last sender — rx drains once all tasks have finished.
            drop(tx);

            let reservoir_size = sampling.reservoir_size;
            let mut sampling_state = SamplingState::new(sampling);
            // In fixed mode the VU count is constant — set it once before draining.
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
            (results, sampling_state)
        }
        .instrument(info_span!(SpanName::REQUESTS, total))
        .await;

        FixedExecutionResult {
            results: all_results,
            total_requests: sampling_state.total_requests(),
            total_failures: sampling_state.total_failures(),
            sample_rate: sampling_state.sample_rate(),
            min_sample_rate: sampling_state.min_sample_rate(),
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
            total_requests: 10,
            total_failures: 1,
            sample_rate: 1.0,
            min_sample_rate: 0.8,
        };
        assert_eq!(result.total_requests, 10);
        assert_eq!(result.total_failures, 1);
        assert_eq!(result.sample_rate, 1.0);
        assert_eq!(result.min_sample_rate, 0.8);
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

        // Verify the executor was constructed — just check it compiles and holds state.
        assert_eq!(executor.params.total, 1);
        assert_eq!(executor.params.concurrency, 1);
    }
}
