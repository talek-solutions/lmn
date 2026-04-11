pub mod curve;
pub mod error;
pub mod fixed;

pub use error::RunError;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::command::{Body, HttpMethod};
use crate::config::secret::SensitiveString;
use crate::histogram::{LatencyHistogram, StatusCodeHistogram};
use crate::http::{RequestConfig, RequestRecord};
use crate::load_curve::LoadCurve;
use crate::response_template::ResponseTemplate;
use crate::response_template::extractor::ExtractionResult;
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

// ── RequestStats ──────────────────────────────────────────────────────────────

/// Generic request-metric container reused for scenario-level and step-level
/// request summaries.
#[derive(Default)]
pub struct RequestStats {
    pub latency: LatencyHistogram,
    pub status_codes: StatusCodeHistogram,
    pub total_requests: u64,
    pub total_failures: u64,
}

impl RequestStats {
    pub fn record(&mut self, duration: Duration, success: bool, status_code: Option<u16>) {
        self.total_requests += 1;
        if !success {
            self.total_failures += 1;
        }
        self.latency.record(duration);
        self.status_codes.record(status_code);
    }
}

// ── ScenarioStepStats ─────────────────────────────────────────────────────────

/// Per-step request metrics inside a scenario.
pub struct ScenarioStepStats {
    pub name: String,
    pub requests: RequestStats,
}

// ── ScenarioStats ─────────────────────────────────────────────────────────────

/// Per-scenario request metrics and nested step breakdowns.
pub struct ScenarioStats {
    pub name: String,
    pub requests: RequestStats,
    pub steps: Vec<ScenarioStepStats>,
}

// ── Shared drain accumulators ────────────────────────────────────────────────

/// Internal accumulator for one scenario while draining request records.
#[derive(Default)]
struct ScenarioAccumulator {
    requests: RequestStats,
    steps: HashMap<String, RequestStats>,
}

/// Internal scenario/step metrics accumulator shared across executors.
#[derive(Default)]
struct ScenarioMetricsAccumulator {
    scenarios: HashMap<String, ScenarioAccumulator>,
}

impl ScenarioMetricsAccumulator {
    fn record(
        &mut self,
        scenario: Option<&str>,
        step: Option<&str>,
        duration: Duration,
        success: bool,
        status_code: Option<u16>,
    ) {
        let Some(scenario_name) = scenario else {
            return;
        };

        let scenario = self.scenarios.entry(scenario_name.to_string()).or_default();
        scenario.requests.record(duration, success, status_code);

        if let Some(step_name) = step {
            scenario
                .steps
                .entry(step_name.to_string())
                .or_default()
                .record(duration, success, status_code);
        }
    }

    fn into_stats(self) -> Option<Vec<ScenarioStats>> {
        let mut scenario_stats: Vec<ScenarioStats> = self
            .scenarios
            .into_iter()
            .map(|(name, acc)| {
                let mut steps: Vec<ScenarioStepStats> = acc
                    .steps
                    .into_iter()
                    .map(|(name, requests)| ScenarioStepStats { name, requests })
                    .collect();
                steps.sort_by(|a, b| a.name.cmp(&b.name));
                ScenarioStats {
                    name,
                    requests: acc.requests,
                    steps,
                }
            })
            .collect();
        scenario_stats.sort_by(|a, b| a.name.cmp(&b.name));

        if scenario_stats.is_empty() {
            None
        } else {
            Some(scenario_stats)
        }
    }
}

/// Shared request-drain accumulator used by both fixed and curve executors.
pub(crate) struct DrainMetricsAccumulator {
    pub latency: LatencyHistogram,
    pub status_codes: StatusCodeHistogram,
    pub total_requests: u64,
    pub total_failures: u64,
    pub response_stats: Option<ResponseStats>,
    scenario_metrics: ScenarioMetricsAccumulator,
}

impl DrainMetricsAccumulator {
    pub fn new(has_tracked_fields: bool) -> Self {
        Self {
            latency: LatencyHistogram::new(),
            status_codes: StatusCodeHistogram::new(),
            total_requests: 0,
            total_failures: 0,
            response_stats: if has_tracked_fields {
                Some(ResponseStats::new())
            } else {
                None
            },
            scenario_metrics: ScenarioMetricsAccumulator::default(),
        }
    }

    pub fn record_request(&mut self, record: &RequestRecord) {
        self.total_requests += 1;
        if !record.success {
            self.total_failures += 1;
        }
        self.latency.record(record.duration);
        self.status_codes.record(record.status_code);

        self.scenario_metrics.record(
            record.scenario.as_deref(),
            record.step.as_deref(),
            record.duration,
            record.success,
            record.status_code,
        );
    }

    pub fn record_extraction(&mut self, extraction: Option<ExtractionResult>) {
        if let Some(extraction) = extraction
            && let Some(ref mut rs) = self.response_stats
        {
            rs.record(extraction);
        }
    }

    pub fn finalize_scenario_stats(&mut self) -> Option<Vec<ScenarioStats>> {
        std::mem::take(&mut self.scenario_metrics).into_stats()
    }
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
    /// Present when scenario/step attribution data was emitted by VUs.
    pub scenario_stats: Option<Vec<ScenarioStats>>,
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
) -> Result<Arc<RequestConfig>, RunError> {
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(concurrency)
        .build()?;
    Ok(Arc::new(RequestConfig {
        client,
        host: Arc::new(host),
        method,
        body: Arc::new(body),
        tracked_fields,
        headers: Arc::new(headers),
    }))
}
