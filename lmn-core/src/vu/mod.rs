use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::http::{Request, RequestConfig, RequestRecord};
use crate::request_template::Template;

// ── Vu ────────────────────────────────────────────────────────────────────────

/// A virtual user. Owns its configuration and drives its own execution loop.
///
/// Call `spawn` to launch the VU as a Tokio task. The VU loops, making requests
/// as fast as it can until its budget is exhausted (fixed mode) or its
/// cancellation token fires (curve mode).
pub struct Vu {
    pub request_config: Arc<RequestConfig>,
    /// Pre-converted header pairs shared across all VUs — avoids per-request allocation.
    pub plain_headers: Arc<Vec<(String, String)>>,
    pub template: Option<Arc<Template>>,
    pub cancellation_token: CancellationToken,
    pub result_tx: mpsc::UnboundedSender<RequestRecord>,
    /// Optional request budget shared across all VUs in fixed-count mode.
    ///
    /// Each VU atomically claims one unit before dispatching a request and stops
    /// when the counter reaches zero. `None` means run until the cancellation
    /// token is triggered (curve mode).
    pub budget: Option<Arc<AtomicUsize>>,
}

impl Vu {
    /// Attempts to claim one unit from the shared request budget.
    ///
    /// Returns `true` if the VU should proceed with the next request, `false`
    /// if the budget is exhausted and the VU should stop.
    ///
    /// In curve mode (`budget` is `None`) always returns `true` — the VU runs
    /// until its cancellation token fires.
    ///
    /// `fetch_update` is used instead of `fetch_sub` to prevent `usize` underflow:
    /// the decrement is only committed when the value is still `> 0`, so exactly
    /// one VU claims the last unit even when many race simultaneously.
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

    /// Consumes the VU and spawns it as a Tokio task.
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                if !self.claim_budget() {
                    break;
                }

                let body = self.template.as_ref().map(|t| t.generate_one());
                let resolved = self.request_config.resolve_body(body);

                let client = self.request_config.client.clone();
                let url = Arc::clone(&self.request_config.host);
                let method = self.request_config.method;
                let tracked_fields = self.request_config.tracked_fields.clone();
                let capture_body = tracked_fields.is_some();

                // Only clone the Arc when there are headers — avoids an atomic op on
                // the common no-headers path.
                let headers = if self.plain_headers.is_empty() {
                    None
                } else {
                    Some(Arc::clone(&self.plain_headers))
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
                    _ = self.cancellation_token.cancelled() => break,
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

                        let record = RequestRecord {
                            duration: result.duration,
                            completed_at: result.completed_at,
                            success: result.success,
                            status_code: result.status_code,
                            extraction,
                        };

                        if self.result_tx.send(record).is_err() {
                            break;
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

    // ── struct_shape_vu ───────────────────────────────────────────────────────

    #[test]
    fn struct_shape_vu() {
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
        let vu = Vu {
            request_config: Arc::clone(&config),
            plain_headers: Arc::new(vec![]),
            template: None,
            cancellation_token: CancellationToken::new(),
            result_tx: tx,
            budget: None,
        };

        assert!(vu.template.is_none());
        assert!(vu.budget.is_none());
    }

    // ── struct_shape_vu_with_budget ───────────────────────────────────────────

    #[test]
    fn struct_shape_vu_with_budget() {
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

        let budget = Arc::new(AtomicUsize::new(100));
        let (tx, _rx) = mpsc::unbounded_channel();
        let vu = Vu {
            request_config: Arc::clone(&config),
            plain_headers: Arc::new(vec![]),
            template: None,
            cancellation_token: CancellationToken::new(),
            result_tx: tx,
            budget: Some(Arc::clone(&budget)),
        };

        assert_eq!(vu.budget.unwrap().load(Ordering::Relaxed), 100);
    }

    // ── budget_fetch_update_prevents_underflow ────────────────────────────────

    #[test]
    fn budget_fetch_update_prevents_underflow() {
        let budget = Arc::new(AtomicUsize::new(1));

        // First claim succeeds
        let first = budget.fetch_update(Ordering::Acquire, Ordering::Relaxed, |n| {
            if n > 0 { Some(n - 1) } else { None }
        });
        assert!(first.is_ok());
        assert_eq!(budget.load(Ordering::Relaxed), 0);

        // Second claim fails — counter does not underflow
        let second = budget.fetch_update(Ordering::Acquire, Ordering::Relaxed, |n| {
            if n > 0 { Some(n - 1) } else { None }
        });
        assert!(second.is_err());
        assert_eq!(budget.load(Ordering::Relaxed), 0);
    }
}
