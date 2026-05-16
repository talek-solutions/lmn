use std::future::Future;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::redirect::Policy;

use crate::publish::config::{MAX_PAYLOAD_BYTES, PublishConfig};
use crate::publish::envelope::PublishEnvelope;
use crate::publish::error::PublishError;
use crate::publish::sink::{PublishOutcome, ResultSink};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Maximum retry attempts for transient (network / 5xx) failures. First
/// attempt is attempt #1, so `MAX_ATTEMPTS = 3` means up to 2 retries.
pub const MAX_ATTEMPTS: u32 = 3;

/// Base delay for exponential backoff. Successive waits are
/// `BASE_BACKOFF * 2^(attempt-1)` capped at `MAX_BACKOFF`.
pub const BASE_BACKOFF: Duration = Duration::from_millis(500);

/// Maximum single backoff delay.
pub const MAX_BACKOFF: Duration = Duration::from_secs(5);

/// Maximum size of an error response body that we include in a
/// `BadRequest` error message. Keeps logs bounded if the server returns an
/// unexpected large body.
pub const MAX_ERROR_BODY_SNIPPET: usize = 512;

// ── HttpSink ──────────────────────────────────────────────────────────────────

/// Publishes run reports to the Lumen platform over HTTPS.
///
/// Transport properties (all enforced, no escape hatch in v1):
/// - `rustls` for TLS (compiled into the workspace `reqwest` dependency)
/// - No redirect following (platform must redirect via DNS, not 3xx)
/// - No `danger_accept_invalid_certs` — ever
/// - Bearer auth via `Authorization` header; key never appears in logs
/// - Retries on transient failures only (network / 5xx), max 3 attempts
pub struct HttpSink {
    config: PublishConfig,
    client: reqwest::Client,
    /// Optional injection point for deterministic sleeps in tests. In
    /// production this is `None` and real `tokio::time::sleep` is used.
    #[cfg(test)]
    sleep_hook: Option<std::sync::Arc<dyn Fn(Duration) + Send + Sync>>,
}

impl HttpSink {
    pub fn new(config: PublishConfig, cli_version: &str) -> Result<Self, PublishError> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .redirect(Policy::none())
            .user_agent(format!("lmn-cli/{cli_version}"))
            .build()
            .map_err(PublishError::Network)?;

        Ok(Self {
            config,
            client,
            #[cfg(test)]
            sleep_hook: None,
        })
    }

    fn target_url(&self) -> &str {
        &self.config.base_url
    }

    async fn post_once(&self, body: &[u8]) -> Result<PublishOutcome, PublishError> {
        let url = self.target_url();

        let mut req = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json");

        if let Some(ref api_key) = self.config.api_key {
            req = req.header(AUTHORIZATION, format!("Bearer {}", api_key.as_str()));
        }

        let resp = req
            .body(body.to_vec())
            .send()
            .await
            .map_err(PublishError::Network)?;

        let status = resp.status();
        let status_u16 = status.as_u16();

        if status.is_success() {
            let maybe_view = parse_response_view_url(resp).await;
            return Ok(PublishOutcome {
                // run_id is filled in by the outer caller (it already has it
                // from the envelope and we want the response parse to never
                // fail the publish).
                run_id: uuid::Uuid::nil(),
                view_url: maybe_view,
                attempts: 0, // filled in by caller
            });
        }

        match status_u16 {
            401 | 403 => Err(PublishError::AuthFailed { status: status_u16 }),
            413 => Err(PublishError::PayloadTooLarge {
                size_bytes: body.len(),
                limit_bytes: MAX_PAYLOAD_BYTES,
            }),
            400..=499 => {
                let snippet = read_bounded_body(resp).await;
                Err(PublishError::BadRequest {
                    status: status_u16,
                    body_snippet: snippet,
                })
            }
            500..=599 => Err(PublishError::ServerError { status: status_u16 }),
            _ => Err(PublishError::UnexpectedStatus { status: status_u16 }),
        }
    }

    async fn sleep(&self, d: Duration) {
        #[cfg(test)]
        {
            if let Some(hook) = &self.sleep_hook {
                hook(d);
                return;
            }
        }
        tokio::time::sleep(d).await;
    }

    #[cfg(test)]
    pub fn with_sleep_hook(mut self, hook: std::sync::Arc<dyn Fn(Duration) + Send + Sync>) -> Self {
        self.sleep_hook = Some(hook);
        self
    }
}

impl ResultSink for HttpSink {
    #[allow(clippy::manual_async_fn)]
    fn publish<'a>(
        &'a self,
        envelope: &'a PublishEnvelope<'a>,
    ) -> impl Future<Output = Result<PublishOutcome, PublishError>> + Send + 'a {
        async move {
            // ── Serialize once up-front — retries reuse the same bytes ───
            let body = serde_json::to_vec(envelope).map_err(PublishError::Serialization)?;
            if body.len() > MAX_PAYLOAD_BYTES {
                return Err(PublishError::PayloadTooLarge {
                    size_bytes: body.len(),
                    limit_bytes: MAX_PAYLOAD_BYTES,
                });
            }

            // ── Retry loop ───────────────────────────────────────────────
            let mut last_err: Option<PublishError> = None;
            for attempt in 1..=MAX_ATTEMPTS {
                match self.post_once(&body).await {
                    Ok(mut outcome) => {
                        outcome.run_id = envelope.run_id;
                        outcome.attempts = attempt;
                        return Ok(outcome);
                    }
                    Err(e) => {
                        let retryable = e.is_retryable();
                        last_err = Some(e);
                        if !retryable || attempt == MAX_ATTEMPTS {
                            break;
                        }
                        self.sleep(backoff_for_attempt(attempt)).await;
                    }
                }
            }
            Err(last_err.expect("at least one attempt executed"))
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn backoff_for_attempt(attempt: u32) -> Duration {
    let exp = attempt.saturating_sub(1);
    let factor = 1u32.checked_shl(exp).unwrap_or(u32::MAX);
    let raw = BASE_BACKOFF.saturating_mul(factor);
    std::cmp::min(raw, MAX_BACKOFF)
}

/// Maximum bytes to buffer from a success response (for `view_url` parsing).
/// The expected response is small JSON (`{"view_url":"..."}`); anything larger
/// is anomalous and discarded.
const MAX_SUCCESS_BODY_BYTES: usize = 4096;

/// Reads at most `limit` bytes from a response, returning them as a UTF-8
/// string. Prevents memory exhaustion from oversized or malicious responses.
async fn read_bounded(resp: reqwest::Response, limit: usize) -> Option<String> {
    // Content-Length is advisory but a cheap early bail-out.
    if resp.content_length().unwrap_or(0) > limit as u64 * 2 {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    if bytes.len() > limit {
        return None;
    }
    String::from_utf8(bytes.to_vec()).ok()
}

async fn read_bounded_body(resp: reqwest::Response) -> String {
    match read_bounded(resp, MAX_ERROR_BODY_SNIPPET + 64).await {
        Some(s) if s.len() <= MAX_ERROR_BODY_SNIPPET => s,
        Some(s) => {
            // Truncate on char boundary to avoid slicing mid UTF-8 codepoint.
            let mut end = MAX_ERROR_BODY_SNIPPET;
            while end > 0 && !s.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}... (truncated)", &s[..end])
        }
        None => "<unreadable response body>".into(),
    }
}

async fn parse_response_view_url(resp: reqwest::Response) -> Option<String> {
    let text = read_bounded(resp, MAX_SUCCESS_BODY_BYTES).await?;
    if text.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("view_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::config::SensitiveString;
    use crate::output::{LatencyStats, RequestSummary, RunMeta, RunReport};
    use crate::publish::envelope::PublishEnvelope;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn test_config(base_url: &str) -> PublishConfig {
        PublishConfig {
            base_url: base_url.to_string(),
            api_key: Some(SensitiveString::new("test-api-key".into())),
            timeout: Duration::from_secs(5),
        }
    }

    fn sample_report() -> RunReport {
        RunReport {
            version: 2,
            run: RunMeta {
                mode: "fixed".to_string(),
                elapsed_ms: 100.0,
                curve_duration_ms: None,
                template_generation_ms: None,
            },
            requests: RequestSummary {
                total: 10,
                ok: 10,
                failed: 0,
                skipped: 0,
                error_rate: 0.0,
                throughput_rps: 100.0,
            },
            latency: LatencyStats {
                min_ms: 1.0,
                p10_ms: 1.0,
                p25_ms: 1.0,
                p50_ms: 2.0,
                p75_ms: 3.0,
                p90_ms: 4.0,
                p95_ms: 5.0,
                p99_ms: 8.0,
                max_ms: 10.0,
                avg_ms: 2.5,
            },
            status_codes: BTreeMap::new(),
            response_stats: None,
            curve_stages: None,
            scenarios: None,
            thresholds: None,
        }
    }

    fn noop_sleep_hook() -> std::sync::Arc<dyn Fn(Duration) + Send + Sync> {
        std::sync::Arc::new(|_| {})
    }

    // ── Unit tests ───────────────────────────────────────────────────────────

    #[test]
    fn backoff_grows_exponentially_and_caps() {
        assert_eq!(backoff_for_attempt(1), BASE_BACKOFF);
        assert_eq!(backoff_for_attempt(2), BASE_BACKOFF * 2);
        assert_eq!(backoff_for_attempt(3), BASE_BACKOFF * 4);
        assert_eq!(backoff_for_attempt(50), MAX_BACKOFF);
    }

    #[test]
    fn new_accepts_cli_version() {
        let config = test_config("https://api.example.com");
        let _sink = HttpSink::new(config, "1.2.3").unwrap();
    }

    // ── Integration tests (wiremock) ─────────────────────────────────────────

    #[tokio::test]
    async fn publish_happy_path_200() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("authorization", "Bearer test-api-key"))
            .and(header("content-type", "application/json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"view_url": "https://app.example.com/runs/1"}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0").unwrap();
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let outcome = sink.publish(&envelope).await.unwrap();
        assert_eq!(outcome.run_id, envelope.run_id);
        assert_eq!(outcome.attempts, 1);
        assert_eq!(
            outcome.view_url.as_deref(),
            Some("https://app.example.com/runs/1")
        );
    }

    #[tokio::test]
    async fn publish_200_empty_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0").unwrap();
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let outcome = sink.publish(&envelope).await.unwrap();
        assert!(outcome.view_url.is_none());
    }

    #[tokio::test]
    async fn publish_401_returns_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0")
            .unwrap()
            .with_sleep_hook(noop_sleep_hook());
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let err = sink.publish(&envelope).await.unwrap_err();
        assert!(matches!(err, PublishError::AuthFailed { status: 401 }));
        // 401 is not retryable — only 1 attempt.
        assert!(!err.is_retryable());
    }

    #[tokio::test]
    async fn publish_403_returns_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0")
            .unwrap()
            .with_sleep_hook(noop_sleep_hook());
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let err = sink.publish(&envelope).await.unwrap_err();
        assert!(matches!(err, PublishError::AuthFailed { status: 403 }));
    }

    #[tokio::test]
    async fn publish_400_returns_bad_request_with_snippet() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(400).set_body_string("invalid schema version"))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0")
            .unwrap()
            .with_sleep_hook(noop_sleep_hook());
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let err = sink.publish(&envelope).await.unwrap_err();
        match err {
            PublishError::BadRequest {
                status,
                body_snippet,
            } => {
                assert_eq!(status, 400);
                assert!(body_snippet.contains("invalid schema version"));
            }
            other => panic!("expected BadRequest, got: {other}"),
        }
    }

    #[tokio::test]
    async fn publish_5xx_retries_then_fails() {
        let call_count = std::sync::Arc::new(AtomicU32::new(0));
        let call_count_clone = call_count.clone();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(503))
            .expect(3) // MAX_ATTEMPTS = 3
            .mount(&server)
            .await;

        let sleep_count = std::sync::Arc::new(AtomicU32::new(0));
        let sleep_count_clone = sleep_count.clone();

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0")
            .unwrap()
            .with_sleep_hook(std::sync::Arc::new(move |_d| {
                sleep_count_clone.fetch_add(1, Ordering::SeqCst);
            }));
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let err = sink.publish(&envelope).await.unwrap_err();
        assert!(matches!(err, PublishError::ServerError { status: 503 }));
        // 3 attempts means 2 sleeps (sleep happens between retries).
        assert_eq!(sleep_count.load(Ordering::SeqCst), 2);
        let _ = call_count_clone; // suppress unused
    }

    #[tokio::test]
    async fn publish_5xx_then_success_on_retry() {
        let server = MockServer::start().await;

        // First two calls return 503, third returns 200.
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0")
            .unwrap()
            .with_sleep_hook(noop_sleep_hook());
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let outcome = sink.publish(&envelope).await.unwrap();
        assert_eq!(outcome.attempts, 3);
        assert_eq!(outcome.run_id, envelope.run_id);
    }

    #[tokio::test]
    async fn publish_413_returns_payload_too_large() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(413))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0")
            .unwrap()
            .with_sleep_hook(noop_sleep_hook());
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let err = sink.publish(&envelope).await.unwrap_err();
        assert!(matches!(err, PublishError::PayloadTooLarge { .. }));
    }

    #[tokio::test]
    async fn publish_unexpected_status_no_retry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(302))
            .expect(1) // should NOT retry
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0")
            .unwrap()
            .with_sleep_hook(noop_sleep_hook());
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let err = sink.publish(&envelope).await.unwrap_err();
        assert!(matches!(
            err,
            PublishError::UnexpectedStatus { status: 302 }
        ));
    }

    #[tokio::test]
    async fn publish_200_malformed_json_view_url_is_none() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(&server.uri());
        let sink = HttpSink::new(config, "0.3.0").unwrap();
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let outcome = sink.publish(&envelope).await.unwrap();
        assert!(outcome.view_url.is_none());
    }

    #[tokio::test]
    async fn publish_without_api_key_sends_no_auth_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let config = PublishConfig {
            base_url: server.uri(),
            api_key: None,
            timeout: Duration::from_secs(5),
        };
        let sink = HttpSink::new(config, "0.3.0").unwrap();
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let outcome = sink.publish(&envelope).await.unwrap();
        assert_eq!(outcome.attempts, 1);
    }

    #[tokio::test]
    async fn publish_with_path_in_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/runs"))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(&format!("{}/v1/runs", server.uri()));
        let sink = HttpSink::new(config, "0.3.0").unwrap();
        let report = sample_report();
        let envelope = PublishEnvelope::new("0.3.0", &report);

        let outcome = sink.publish(&envelope).await.unwrap();
        assert_eq!(outcome.attempts, 1);
    }
}
