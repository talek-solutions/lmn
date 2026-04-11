use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::execution::OnStepFailure;
use crate::http::{Request, RequestConfig, RequestRecord};
use crate::request_template::Template;
use crate::response_template::field::TrackedField;

// ── StepExec ──────────────────────────────────────────────────────────────────

/// All execution-ready data for a single step within a scenario.
pub struct StepExec {
    pub step_name: Arc<str>,
    pub request_config: Arc<RequestConfig>,
    /// Pre-converted header pairs — avoids per-request allocation.
    pub plain_headers: Arc<Vec<(String, String)>>,
    pub request_template: Option<Arc<Template>>,
    /// Tracked fields parsed from the response template, if present.
    pub response_template: Option<Arc<Vec<TrackedField>>>,
}

// ── ScenarioVu ────────────────────────────────────────────────────────────────

/// A virtual user that executes a fixed sequence of steps on each iteration.
///
/// Each loop iteration claims a single budget unit (covering all steps), then
/// executes every step sequentially. Step failures are handled per
/// `on_step_failure`: either the remaining steps are skipped (`AbortIteration`)
/// or execution continues (`Continue`).
pub struct ScenarioVu {
    pub scenario_name: Arc<str>,
    pub steps: Vec<StepExec>,
    pub on_step_failure: OnStepFailure,
    pub cancellation_token: CancellationToken,
    pub result_tx: mpsc::UnboundedSender<RequestRecord>,
    /// Optional request budget shared across all VUs in fixed-count mode.
    ///
    /// One unit is claimed per full iteration (not per step). `None` means the
    /// VU runs until the cancellation token fires (curve mode).
    pub budget: Option<Arc<AtomicUsize>>,
}

impl ScenarioVu {
    /// Attempts to claim one iteration unit from the shared budget.
    ///
    /// Returns `true` if the VU should proceed with the next iteration, `false`
    /// if the budget is exhausted and the VU should stop.
    ///
    /// Uses `fetch_update` instead of `fetch_sub` to prevent underflow: the
    /// decrement is only committed when the value is still `> 0`.
    fn claim_budget(&self) -> bool {
        match &self.budget {
            None => true,
            Some(b) => b
                .fetch_update(Ordering::Acquire, Ordering::Relaxed, |n| {
                    if n > 0 { Some(n - 1) } else { None }
                })
                .is_ok(),
        }
    }

    /// Consumes the `ScenarioVu` and spawns it as a Tokio task.
    ///
    /// Returns a `JoinHandle` that resolves when the VU exits — either because
    /// the budget is exhausted, the cancellation token fires, or the result
    /// channel is closed.
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                // Check cancellation before claiming the budget so we don't
                // consume a unit we won't use.
                if self.cancellation_token.is_cancelled() {
                    break;
                }

                // Claim one iteration unit. In curve mode this always succeeds.
                if !self.claim_budget() {
                    break;
                }

                // Execute steps sequentially.
                let mut abort_iteration = false;
                for step in &self.steps {
                    if self.cancellation_token.is_cancelled() {
                        return;
                    }

                    // Generate request body from the request template, if present.
                    let body = match step.request_template.as_ref().map(|t| t.generate_one()) {
                        None => None,
                        Some(Ok(s)) => Some(s),
                        Some(Err(e)) => {
                            tracing::error!(
                                scenario = %self.scenario_name,
                                step = %step.step_name,
                                error = %e,
                                "template serialization failed, skipping step"
                            );
                            continue;
                        }
                    };

                    let resolved = step.request_config.resolve_body(body);

                    let client = step.request_config.client.clone();
                    let url = Arc::clone(&step.request_config.host);
                    let method = step.request_config.method;
                    let tracked_fields = step.response_template.clone();
                    let capture_body = tracked_fields.is_some();

                    // Only clone the Arc when there are headers — avoids an atomic op on
                    // the common no-headers path.
                    let headers = if step.plain_headers.is_empty() {
                        None
                    } else {
                        Some(Arc::clone(&step.plain_headers))
                    };

                    let result_fut = async {
                        let mut req = Request::new(client, url, method);
                        if let Some((content, content_type)) = resolved {
                            req = req.body(content, content_type);
                        }
                        if capture_body {
                            req = req.read_response();
                        }
                        if let Some(h) = headers {
                            req = req.headers(h);
                        }
                        req.execute().await
                    };

                    tokio::select! {
                        _ = self.cancellation_token.cancelled() => return,
                        result = result_fut => {
                            // Perform response body extraction inline in the VU before
                            // sending over the channel, so raw response bodies never
                            // transit the channel.
                            let extraction = if let Some(ref fields) = tracked_fields {
                                if let Some(ref body_str) = result.response_body {
                                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(body_str) {
                                        Some(crate::response_template::extractor::extract(&val, fields))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            let step_failed = !result.success;

                            let record = RequestRecord {
                                duration: result.duration,
                                completed_at: result.completed_at,
                                success: result.success,
                                status_code: result.status_code,
                                extraction,
                                scenario: Some(Arc::clone(&self.scenario_name)),
                                step: Some(Arc::clone(&step.step_name)),
                            };

                            if self.result_tx.send(record).is_err() {
                                return;
                            }

                            if step_failed
                                && matches!(self.on_step_failure, OnStepFailure::AbortIteration)
                            {
                                abort_iteration = true;
                                break;
                            }
                        }
                    }
                }

                // `abort_iteration` is only used to break out of the step loop;
                // the outer loop always continues to the next iteration.
                let _ = abort_iteration;
            }
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    // ── struct_shape_step_exec ────────────────────────────────────────────────

    #[test]
    fn struct_shape_step_exec() {
        use crate::command::HttpMethod;
        use crate::http::RequestConfig;

        let config = Arc::new(RequestConfig {
            client: reqwest::Client::new(),
            host: Arc::new("http://localhost".to_string()),
            method: HttpMethod::Get,
            body: Arc::new(None),
            tracked_fields: None,
            headers: Arc::new(vec![]),
        });

        let step = StepExec {
            step_name: Arc::from("login"),
            request_config: Arc::clone(&config),
            plain_headers: Arc::new(vec![]),
            request_template: None,
            response_template: None,
        };

        assert_eq!(&*step.step_name, "login");
        assert!(step.request_template.is_none());
        assert!(step.response_template.is_none());
    }

    // ── struct_shape_scenario_vu ──────────────────────────────────────────────

    #[test]
    fn struct_shape_scenario_vu() {
        use crate::command::HttpMethod;
        use crate::http::RequestConfig;

        let config = Arc::new(RequestConfig {
            client: reqwest::Client::new(),
            host: Arc::new("http://localhost".to_string()),
            method: HttpMethod::Get,
            body: Arc::new(None),
            tracked_fields: None,
            headers: Arc::new(vec![]),
        });

        let (tx, _rx) = mpsc::unbounded_channel();

        let vu = ScenarioVu {
            scenario_name: Arc::from("checkout"),
            steps: vec![StepExec {
                step_name: Arc::from("add_to_cart"),
                request_config: Arc::clone(&config),
                plain_headers: Arc::new(vec![]),
                request_template: None,
                response_template: None,
            }],
            on_step_failure: OnStepFailure::Continue,
            cancellation_token: CancellationToken::new(),
            result_tx: tx,
            budget: None,
        };

        assert_eq!(&*vu.scenario_name, "checkout");
        assert_eq!(vu.steps.len(), 1);
        assert!(vu.budget.is_none());
    }

    // ── struct_shape_scenario_vu_with_budget ──────────────────────────────────

    #[test]
    fn struct_shape_scenario_vu_with_budget() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let budget = Arc::new(AtomicUsize::new(50));

        let vu = ScenarioVu {
            scenario_name: Arc::from("checkout"),
            steps: vec![],
            on_step_failure: OnStepFailure::AbortIteration,
            cancellation_token: CancellationToken::new(),
            result_tx: tx,
            budget: Some(Arc::clone(&budget)),
        };

        assert_eq!(vu.budget.unwrap().load(Ordering::Relaxed), 50);
    }

    // ── budget_claim_exhausts_correctly ──────────────────────────────────────

    #[test]
    fn budget_claim_exhausts_correctly() {
        let budget = Arc::new(AtomicUsize::new(2));

        // First claim
        let first = budget.fetch_update(Ordering::Acquire, Ordering::Relaxed, |n| {
            if n > 0 { Some(n - 1) } else { None }
        });
        assert!(first.is_ok());
        assert_eq!(budget.load(Ordering::Relaxed), 1);

        // Second claim
        let second = budget.fetch_update(Ordering::Acquire, Ordering::Relaxed, |n| {
            if n > 0 { Some(n - 1) } else { None }
        });
        assert!(second.is_ok());
        assert_eq!(budget.load(Ordering::Relaxed), 0);

        // Third claim — exhausted, no underflow
        let third = budget.fetch_update(Ordering::Acquire, Ordering::Relaxed, |n| {
            if n > 0 { Some(n - 1) } else { None }
        });
        assert!(third.is_err());
        assert_eq!(budget.load(Ordering::Relaxed), 0);
    }
}
