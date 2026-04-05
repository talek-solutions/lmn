pub mod curve;
pub mod fixed;

use std::sync::Arc;
use std::path::PathBuf;

use crate::command::{Body, HttpMethod};
use crate::config::secret::SensitiveString;
use crate::histogram::{LatencyHistogram, StatusCodeHistogram};
use crate::http::RequestConfig;
use crate::load_curve::LoadCurve;
use crate::response_template::ResponseTemplate;
use crate::response_template::field::TrackedField;
use crate::response_template::stats::ResponseStats;

// ── RunMode ───────────────────────────────────────────────────────────────────

/// Indicates which execution strategy produced the run results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Worker-pool fixed-count mode: N long-lived VUs share an atomic request budget.
    Fixed,
    /// Time-based dynamic VU mode driven by a `LoadCurve`.
    Curve,
}

// ── TemplateStats ─────────────────────────────────────────────────────────────

/// Timing information for request template generation.
#[derive(Debug, Clone)]
pub struct TemplateStats {
    pub generation_duration: std::time::Duration,
}

// ── StageStats ────────────────────────────────────────────────────────────────

/// Per-stage latency and request counts for curve-mode runs.
pub struct StageStats {
    pub latency: LatencyHistogram,
    pub status_codes: StatusCodeHistogram,
    pub total_requests: u64,
    pub total_failures: u64,
}

// ── CurveStats ────────────────────────────────────────────────────────────────

/// Curve-specific metadata captured at the end of a curve run.
pub struct CurveStats {
    pub duration: std::time::Duration,
    pub stages: Vec<crate::load_curve::Stage>,
    /// Per-stage histogram data — one entry per stage in the load curve.
    pub stage_stats: Vec<StageStats>,
}

// ── RunStats ──────────────────────────────────────────────────────────────────

pub struct RunStats {
    pub elapsed: std::time::Duration,
    pub mode: RunMode,
    pub latency: LatencyHistogram,
    pub status_codes: StatusCodeHistogram,
    pub total_requests: u64,
    pub total_failures: u64,
    pub template_stats: Option<TemplateStats>,
    pub response_stats: Option<ResponseStats>,
    pub curve_stats: Option<CurveStats>,
}

// ── RequestSpec ───────────────────────────────────────────────────────────────

/// All request-level parameters for a run.
pub struct RequestSpec {
    pub host: String,
    pub method: HttpMethod,
    pub body: Option<Body>,
    pub template_path: Option<PathBuf>,
    pub response_template_path: Option<PathBuf>,
    /// Custom HTTP headers to send with every request in this run.
    pub headers: Vec<(String, SensitiveString)>,
}

// ── ExecutionMode ─────────────────────────────────────────────────────────────

/// Determines the execution strategy for a run.
pub enum ExecutionMode {
    /// Classic semaphore-based fixed-count execution.
    Fixed {
        request_count: usize,
        concurrency: usize,
    },
    /// Time-based dynamic VU execution driven by a `LoadCurve`.
    Curve(LoadCurve),
}

// ── Shared helpers ────────────────────────────────────────────────────────────

pub(crate) fn resolve_tracked_fields(
    path: Option<PathBuf>,
) -> Result<Option<Arc<Vec<TrackedField>>>, Box<dyn std::error::Error>> {
    path.map(|p| {
        ResponseTemplate::parse(&p)
            .map(|rt| Arc::new(rt.fields))
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    })
    .transpose()
}

pub(crate) fn build_request_config(
    host: String,
    method: HttpMethod,
    body: Option<Body>,
    tracked_fields: Option<Arc<Vec<TrackedField>>>,
    headers: Vec<(String, SensitiveString)>,
    concurrency: usize,
) -> Arc<RequestConfig> {
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(concurrency)
        .build()
        .expect("failed to build HTTP client");
    Arc::new(RequestConfig {
        client,
        host: Arc::new(host),
        method,
        body: Arc::new(body),
        tracked_fields,
        headers: Arc::new(headers),
    })
}
