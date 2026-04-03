pub mod curve;
pub mod fixed;

use std::sync::Arc;
use std::path::PathBuf;

use crate::command::{Body, HttpMethod};
use crate::config::secret::SensitiveString;
use crate::http::RequestConfig;
use crate::load_curve::LoadCurve;
use crate::response_template::ResponseTemplate;
use crate::response_template::extractor;
use crate::response_template::field::TrackedField;
use crate::response_template::stats::ResponseStats;
use crate::http::RequestResult;

// ── RunMode ───────────────────────────────────────────────────────────────────

/// Indicates which execution strategy produced the run results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Classic semaphore-based fixed-count mode.
    Fixed,
    /// Time-based dynamic VU mode driven by a `LoadCurve`.
    Curve,
}

// ── SamplingStats ─────────────────────────────────────────────────────────────

/// Aggregated sampling counters produced at the end of a run.
#[derive(Debug, Clone)]
pub struct SamplingStats {
    /// Actual (unsampled) total request count.
    pub total_requests: usize,
    /// Actual (unsampled) failure count.
    pub total_failures: usize,
    /// Final VU-threshold sample rate at end of run (1.0 = no threshold sampling).
    pub sample_rate: f64,
    /// Lowest sample rate observed at any point during the run.
    pub min_sample_rate: f64,
}

// ── TemplateStats ─────────────────────────────────────────────────────────────

/// Timing information for request template generation.
#[derive(Debug, Clone)]
pub struct TemplateStats {
    pub generation_duration: std::time::Duration,
}

// ── CurveStats ────────────────────────────────────────────────────────────────

/// Curve-specific metadata captured at the end of a curve run.
#[derive(Debug, Clone)]
pub struct CurveStats {
    pub duration: std::time::Duration,
    pub stages: Vec<crate::load_curve::Stage>,
}

// ── RunStats ──────────────────────────────────────────────────────────────────

pub struct RunStats {
    pub elapsed: std::time::Duration,
    pub mode: RunMode,
    pub request_results: Vec<RequestResult>,
    pub sampling_stats: SamplingStats,
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

// ── SamplingConfig ────────────────────────────────────────────────────────────

/// Sampling and reservoir parameters for a run.
pub struct SamplingConfig {
    pub sample_threshold: usize,
    pub result_buffer: usize,
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
) -> Arc<RequestConfig> {
    Arc::new(RequestConfig {
        client: reqwest::Client::new(),
        host: Arc::new(host),
        method,
        body: Arc::new(body),
        tracked_fields,
        headers: Arc::new(headers),
    })
}

pub(crate) fn compute_response_stats(
    results: &[RequestResult],
    tracked_fields: &Option<Arc<Vec<TrackedField>>>,
) -> Option<ResponseStats> {
    tracked_fields.as_ref().map(|fields| {
        let mut rs = ResponseStats::new();
        for result in results {
            if let Some(ref body_str) = result.response_body
                && let Ok(body_val) = serde_json::from_str(body_str)
            {
                rs.record(extractor::extract(&body_val, fields));
            }
        }
        rs
    })
}
