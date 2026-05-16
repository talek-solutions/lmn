use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::capture::{
    CaptureDefinition, CaptureState, inject_captures, inject_captures_into_headers, value_to_string,
};
use crate::execution::{OnStepFailure, RpsLimiter};
use crate::http::{Request, RequestConfig, RequestRecord};
use crate::request_template::Template;
use crate::response_template::extractor::resolve_path;
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
    /// Capture definitions to extract from the response body.
    pub captures: Vec<CaptureDefinition>,
    /// Inline request body (mutually exclusive with `request_template`).
    pub inline_body: Option<Arc<str>>,
    /// True if any header value contains `{{capture.` references.
    pub has_capture_headers: bool,
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
    /// Optional shared RPS limiter. When present, one permit is awaited per
    /// HTTP step (not per iteration), so the cap reflects requests-per-second
    /// regardless of how many steps a scenario has.
    pub rate_limiter: Option<Arc<RpsLimiter>>,
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

    /// Emits a skipped `RequestRecord` for the given step.
    fn emit_skipped(&self, step: &StepExec) {
        let _ = self.result_tx.send(RequestRecord {
            duration: std::time::Duration::ZERO,
            completed_at: Instant::now(),
            success: false,
            status_code: None,
            extraction: None,
            scenario: Some(Arc::clone(&self.scenario_name)),
            step: Some(Arc::clone(&step.step_name)),
            skipped: true,
        });
    }

    /// Consumes the `ScenarioVu` and spawns it as a Tokio task.
    ///
    /// Returns a `JoinHandle` that resolves when the VU exits — either because
    /// the budget is exhausted, the cancellation token fires, or the result
    /// channel is closed.
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut captures = CaptureState::new();

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

                // Clear capture state at the start of each iteration.
                captures.clear();

                // Execute steps sequentially.
                let mut abort_remaining = false;
                for (step_idx, step) in self.steps.iter().enumerate() {
                    if self.cancellation_token.is_cancelled() {
                        return;
                    }

                    // If a previous step triggered abort, emit skipped for remaining steps.
                    if abort_remaining {
                        self.emit_skipped(step);
                        continue;
                    }

                    // 1. Generate body (from request_template, inline_body, or none).
                    let mut body_string: Option<String> = match step
                        .request_template
                        .as_ref()
                        .map(|t| t.generate_one())
                    {
                        None => step.inline_body.as_ref().map(|b| b.to_string()),
                        Some(Ok(s)) => Some(s),
                        Some(Err(e)) => {
                            eprintln!(
                                "error: template serialization failed in {}/{}, aborting iteration: {e}",
                                self.scenario_name, step.step_name
                            );
                            for remaining in &self.steps[step_idx..] {
                                self.emit_skipped(remaining);
                            }
                            break;
                        }
                    };

                    // 2. Inject {{capture.KEY}} into body.
                    if let Some(ref body) = body_string {
                        match inject_captures(body, &captures) {
                            Ok(injected) => body_string = Some(injected),
                            Err(e) => {
                                eprintln!(
                                    "warning: capture injection into body failed in {}/{}, aborting iteration: {e}",
                                    self.scenario_name, step.step_name
                                );
                                // Emit skipped for this step and all remaining, then
                                // break out of the step loop (no abort_remaining needed).
                                for remaining in &self.steps[step_idx..] {
                                    self.emit_skipped(remaining);
                                }
                                break;
                            }
                        }
                    }

                    // 3. Inject {{capture.KEY}} into headers (if needed).
                    let headers = if step.has_capture_headers {
                        match inject_captures_into_headers(&step.plain_headers, &captures) {
                            Ok(injected) => {
                                if injected.is_empty() {
                                    None
                                } else {
                                    Some(Arc::new(injected))
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "warning: capture injection into headers failed in {}/{}, aborting iteration: {e}",
                                    self.scenario_name, step.step_name
                                );
                                for remaining in &self.steps[step_idx..] {
                                    self.emit_skipped(remaining);
                                }
                                break;
                            }
                        }
                    } else if step.plain_headers.is_empty() {
                        None
                    } else {
                        Some(Arc::clone(&step.plain_headers))
                    };

                    // Wait for an RPS permit if a limiter is configured.
                    // Awaited per-step so the cap is requests-per-second, not
                    // iterations-per-second.
                    if let Some(ref rl) = self.rate_limiter {
                        tokio::select! {
                            _ = self.cancellation_token.cancelled() => return,
                            _ = rl.acquire() => {}
                        }
                    }

                    // 4. Build and execute the HTTP request.
                    // Resolve body through request_config (handles Body::Formatted).
                    let resolved = step.request_config.resolve_body(body_string);

                    let client = step.request_config.client.clone();
                    let url = Arc::clone(&step.request_config.host);
                    let method = step.request_config.method;
                    let tracked_fields = step.response_template.clone();
                    let needs_response_body = tracked_fields.is_some() || !step.captures.is_empty();

                    let result_fut = async {
                        let mut req = Request::new(client, url, method);
                        if let Some((content, content_type)) = resolved {
                            req = req.body(content, content_type);
                        }
                        if needs_response_body {
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
                            // Parse response body once — shared by extraction and capture.
                            let parsed_body: Option<serde_json::Value> =
                                result.response_body.as_deref().and_then(|s| {
                                    serde_json::from_str(s).ok()
                                });

                            // 5. Extract response_template fields (existing, for stats).
                            let extraction = if let Some(ref fields) = tracked_fields {
                                parsed_body
                                    .as_ref()
                                    .map(|val| crate::response_template::extractor::extract(val, fields))
                            } else {
                                None
                            };

                            // 6. Extract captures from response JSON.
                            if !step.captures.is_empty()
                                && let Some(ref body_val) = parsed_body {
                                    for cap in &step.captures {
                                        if let Some(matched) = resolve_path(body_val, &cap.path)
                                            && let Some(s) = value_to_string(matched) {
                                                captures.insert(cap.alias.clone(), s);
                                            }
                                    }
                                }

                            let step_failed = !result.success;

                            // 7. Send RequestRecord through channel.
                            let record = RequestRecord {
                                duration: result.duration,
                                completed_at: result.completed_at,
                                success: result.success,
                                status_code: result.status_code,
                                extraction,
                                scenario: Some(Arc::clone(&self.scenario_name)),
                                step: Some(Arc::clone(&step.step_name)),
                                skipped: false,
                            };

                            if self.result_tx.send(record).is_err() {
                                return;
                            }

                            // 8. If step failed + on_step_failure == AbortIteration → skip remaining.
                            if step_failed
                                && matches!(self.on_step_failure, OnStepFailure::AbortIteration)
                            {
                                abort_remaining = true;
                            }
                        }
                    }
                }
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
            captures: vec![],
            inline_body: None,
            has_capture_headers: false,
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
                captures: vec![],
                inline_body: None,
                has_capture_headers: false,
            }],
            on_step_failure: OnStepFailure::Continue,
            cancellation_token: CancellationToken::new(),
            result_tx: tx,
            budget: None,
            rate_limiter: None,
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
            rate_limiter: None,
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
