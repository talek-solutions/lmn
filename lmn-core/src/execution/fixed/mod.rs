use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use tracing::info_span;

use crate::execution::{DrainMetricsAccumulator, ResolvedScenario, ScenarioStats, assign_scenario};
use crate::histogram::{LatencyHistogram, StatusCodeHistogram};
use crate::http::{RequestConfig, RequestRecord};
use crate::monitoring::SpanName;
use crate::request_template::Template;
use crate::response_template::stats::ResponseStats;
use crate::vu::Vu;
use crate::vu::scenario::{ScenarioVu, StepExec};

// ── FixedExecutorParams ───────────────────────────────────────────────────────

/// Parameters for constructing a `FixedExecutor`.
pub struct FixedExecutorParams {
    pub request_config: Arc<RequestConfig>,
    pub template: Option<Arc<Template>>,
    pub total: usize,
    pub concurrency: usize,
    pub cancellation_token: CancellationToken,
    /// When present, the executor spawns `ScenarioVu` instances instead of
    /// plain `Vu` instances. Each VU is assigned a scenario via weighted
    /// round-robin. Budget counts iterations (one per full scenario loop).
    pub scenarios: Option<Vec<ResolvedScenario>>,
}

// ── FixedExecutionResult ──────────────────────────────────────────────────────

/// Result returned by `FixedExecutor::execute`.
pub struct FixedExecutionResult {
    pub latency: LatencyHistogram,
    pub status_codes: StatusCodeHistogram,
    pub total_requests: u64,
    pub total_failures: u64,
    pub response_stats: Option<ResponseStats>,
    pub scenario_stats: Option<Vec<ScenarioStats>>,
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
    /// of `total` requests. Returns a `FixedExecutionResult` when all requests
    /// complete or a cancellation signal is received.
    pub async fn execute(self) -> Result<FixedExecutionResult, crate::execution::RunError> {
        let FixedExecutorParams {
            request_config,
            template,
            total,
            concurrency,
            cancellation_token,
            scenarios,
        } = self.params;

        // Pre-convert headers once before spawning VUs to avoid per-request allocation.
        let plain_headers: Arc<Vec<(String, String)>> = Arc::new(
            request_config
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
        );

        // `has_tracked_fields` is true when the single-request path uses a
        // response template, OR when any step across any scenario does.
        let has_tracked_fields = if let Some(ref sc) = scenarios {
            sc.iter()
                .flat_map(|s| s.steps.iter())
                .any(|step| step.response_template.is_some())
        } else {
            request_config.tracked_fields.is_some()
        };

        async {
            let budget = Arc::new(AtomicUsize::new(total));
            let (tx, rx) = mpsc::unbounded_channel::<RequestRecord>();

            // Spawn a dedicated drain task that owns the receiver and all
            // accumulator state. It returns a `FixedExecutionResult` once the
            // channel closes (all VU senders dropped).
            let drain_handle = tokio::spawn(async move {
                let mut rx = rx;
                let mut acc = DrainMetricsAccumulator::new(has_tracked_fields);

                while let Some(record) = rx.recv().await {
                    acc.record_request(&record);
                    acc.record_extraction(record.extraction);
                }
                let scenario_stats = acc.finalize_scenario_stats();

                FixedExecutionResult {
                    latency: acc.latency,
                    status_codes: acc.status_codes,
                    total_requests: acc.total_requests,
                    total_failures: acc.total_failures,
                    response_stats: acc.response_stats,
                    scenario_stats,
                }
            });

            let vu_handles: Vec<_> = if let Some(ref scenarios) = scenarios {
                // Scenario mode: spawn `ScenarioVu` instances. Each VU is
                // assigned a scenario via weighted round-robin and claims one
                // budget unit per full iteration (not per step).
                (0..concurrency)
                    .map(|vu_idx| {
                        let scenario = &scenarios[assign_scenario(vu_idx, scenarios)];
                        let steps = scenario
                            .steps
                            .iter()
                            .map(|step| StepExec {
                                step_name: Arc::clone(&step.name),
                                request_config: Arc::clone(&step.request_config),
                                plain_headers: Arc::clone(&step.plain_headers),
                                request_template: step.request_template.as_ref().map(Arc::clone),
                                response_template: step.response_template.as_ref().map(Arc::clone),
                            })
                            .collect();
                        ScenarioVu {
                            scenario_name: Arc::clone(&scenario.name),
                            steps,
                            on_step_failure: scenario.on_step_failure,
                            cancellation_token: cancellation_token.clone(),
                            result_tx: tx.clone(),
                            budget: Some(Arc::clone(&budget)),
                        }
                        .spawn()
                    })
                    .collect()
            } else {
                // Single-request mode: spawn plain `Vu` instances sharing the budget.
                (0..concurrency)
                    .map(|_| {
                        Vu {
                            request_config: Arc::clone(&request_config),
                            plain_headers: Arc::clone(&plain_headers),
                            template: template.as_ref().map(Arc::clone),
                            scenario_label: None,
                            step_label: None,
                            cancellation_token: cancellation_token.clone(),
                            result_tx: tx.clone(),
                            budget: Some(Arc::clone(&budget)),
                        }
                        .spawn()
                    })
                    .collect()
            };

            // Drop the coordinator's sender so the channel closes once all VU
            // senders are also dropped (they are, once each VU task exits).
            drop(tx);

            // Await all VU tasks to ensure they have finished sending.
            for handle in vu_handles {
                let _ = handle.await;
            }

            // All VU senders are now dropped — channel is closed. Await the
            // drain task to get the accumulated result.
            Ok(drain_handle.await?)
        }
        .instrument(info_span!(SpanName::REQUESTS, total))
        .await
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
            latency: LatencyHistogram::new(),
            status_codes: StatusCodeHistogram::new(),
            total_requests: 10,
            total_failures: 1,
            response_stats: None,
            scenario_stats: None,
        };
        assert_eq!(result.total_requests, 10);
        assert_eq!(result.total_failures, 1);
        assert!(result.latency.is_empty());
        assert!(result.response_stats.is_none());
        assert!(result.scenario_stats.is_none());
    }

    // ── struct_shape_fixed_executor_params ────────────────────────────────────

    #[test]
    fn struct_shape_fixed_executor_params() {
        use crate::command::HttpMethod;
        use crate::http::RequestConfig;
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
            scenarios: None,
        };

        assert_eq!(params.total, 5);
        assert_eq!(params.concurrency, 2);
        assert!(params.template.is_none());
        assert!(params.scenarios.is_none());
    }

    // ── fixed_executor_new_stores_params ─────────────────────────────────────

    #[test]
    fn fixed_executor_new_stores_params() {
        use crate::command::HttpMethod;
        use crate::http::RequestConfig;
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
            scenarios: None,
        });

        assert_eq!(executor.params.total, 1);
        assert_eq!(executor.params.concurrency, 1);
    }
}
