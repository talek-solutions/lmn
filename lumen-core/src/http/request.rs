use std::sync::Arc;
use std::time::Instant;
use std::time::Duration;

use crate::command::{Body, HttpMethod};
use crate::response_template::field::TrackedField;

// ── Body format ───────────────────────────────────────────────────────────────

pub enum BodyFormat {
    Json,
}

// ── Shared per-run config ─────────────────────────────────────────────────────

pub struct RequestConfig {
    pub client: reqwest::Client,
    pub host: Arc<String>,
    pub method: HttpMethod,
    pub body: Arc<Option<Body>>,
    pub tracked_fields: Option<Arc<Vec<TrackedField>>>,
}

impl RequestConfig {
    /// Resolves the body for a single request.
    /// Returns `(content, content_type)`, or `None` if no body is configured.
    /// `template_body` takes priority when provided (pre-generated template output).
    pub fn resolve_body(&self, template_body: Option<String>) -> Option<(String, &'static str)> {
        if let Some(content) = template_body {
            return Some((content, "application/json"));
        }
        self.body.as_ref().as_ref().map(|b| match b {
            Body::Formatted { content, format } => (
                content.clone(),
                match format {
                    BodyFormat::Json => "application/json",
                },
            ),
        })
    }
}

// ── Result ────────────────────────────────────────────────────────────────────

pub struct RequestResult {
    pub duration: Duration,
    /// Wall-clock instant at which the response was received (or the error occurred).
    /// Used by the output module to bucket results into per-stage windows when
    /// `mode == Curve`. Zero-cost in fixed mode where `curve_stages` is `None`.
    pub completed_at: Instant,
    pub success: bool,
    pub status_code: Option<u16>,
    pub response_body: Option<String>,
}

impl RequestResult {
    /// Constructs a `RequestResult` with all fields explicit.
    ///
    /// Use this constructor rather than struct literals so that adding fields in the
    /// future causes a compile error at every call site, preventing silent omissions.
    pub fn new(
        duration: Duration,
        success: bool,
        status_code: Option<u16>,
        response_body: Option<String>,
    ) -> Self {
        Self {
            duration,
            completed_at: Instant::now(),
            success,
            status_code,
            response_body,
        }
    }
}

// ── Request builder ───────────────────────────────────────────────────────────

pub struct Request {
    client: reqwest::Client,
    url: String,
    method: HttpMethod,
    body: Option<(String, &'static str)>,
    capture_response: bool,
}

impl Request {
    pub fn new(client: reqwest::Client, url: String, method: HttpMethod) -> Self {
        Self { client, url, method, body: None, capture_response: false }
    }

    pub fn body(mut self, content: String, content_type: &'static str) -> Self {
        self.body = Some((content, content_type));
        self
    }

    pub fn read_response(mut self) -> Self {
        self.capture_response = true;
        self
    }

    pub async fn execute(self) -> RequestResult {
        let start = Instant::now();
        let mut req = match self.method {
            HttpMethod::Get    => self.client.get(&self.url),
            HttpMethod::Post   => self.client.post(&self.url),
            HttpMethod::Put    => self.client.put(&self.url),
            HttpMethod::Patch  => self.client.patch(&self.url),
            HttpMethod::Delete => self.client.delete(&self.url),
        };
        if let Some((content, content_type)) = self.body {
            req = req.header("Content-Type", content_type).body(content);
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let response_body = if self.capture_response {
                    resp.text().await.ok()
                } else {
                    None
                };
                let duration = start.elapsed();
                let completed_at = Instant::now();
                RequestResult {
                    duration,
                    completed_at,
                    success: status.is_success(),
                    status_code: Some(status.as_u16()),
                    response_body,
                }
            }
            Err(_) => {
                let duration = start.elapsed();
                let completed_at = Instant::now();
                RequestResult {
                    duration,
                    completed_at,
                    success: false,
                    status_code: None,
                    response_body: None,
                }
            }
        }
    }
}
