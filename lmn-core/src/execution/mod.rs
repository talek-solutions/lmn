pub mod curve;
pub mod error;
pub mod fixed;

pub use error::RunError;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::capture::CaptureDefinition;
use crate::command::{Body, HttpMethod};
use crate::config::secret::SensitiveString;
use crate::histogram::{LatencyHistogram, StatusCodeHistogram};
use crate::http::{RequestConfig, RequestRecord};
use crate::load_curve::LoadCurve;
use crate::request_template::Template;
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
    pub total_skipped: u64,
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

    pub fn record_skipped(&mut self) {
        self.total_requests += 1;
        self.total_skipped += 1;
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
    steps: HashMap<Arc<str>, RequestStats>,
}

/// Internal scenario/step metrics accumulator shared across executors.
#[derive(Default)]
struct ScenarioMetricsAccumulator {
    scenarios: HashMap<Arc<str>, ScenarioAccumulator>,
}

impl ScenarioMetricsAccumulator {
    fn record(
        &mut self,
        scenario: Option<&Arc<str>>,
        step: Option<&Arc<str>>,
        duration: Duration,
        success: bool,
        status_code: Option<u16>,
        skipped: bool,
    ) {
        let Some(scenario_name) = scenario else {
            return;
        };

        let scenario = self.scenarios.entry(Arc::clone(scenario_name)).or_default();

        if skipped {
            scenario.requests.record_skipped();
            if let Some(step_name) = step {
                scenario
                    .steps
                    .entry(Arc::clone(step_name))
                    .or_default()
                    .record_skipped();
            }
        } else {
            scenario.requests.record(duration, success, status_code);
            if let Some(step_name) = step {
                scenario
                    .steps
                    .entry(Arc::clone(step_name))
                    .or_default()
                    .record(duration, success, status_code);
            }
        }
    }

    /// Returns scenarios sorted by name, with steps sorted by name within each scenario.
    fn into_stats(self) -> Option<Vec<ScenarioStats>> {
        let mut scenario_stats: Vec<ScenarioStats> = self
            .scenarios
            .into_iter()
            .map(|(name, acc)| {
                let mut steps: Vec<ScenarioStepStats> = acc
                    .steps
                    .into_iter()
                    .map(|(name, requests)| ScenarioStepStats {
                        name: name.to_string(),
                        requests,
                    })
                    .collect();
                steps.sort_by(|a, b| a.name.cmp(&b.name));
                ScenarioStats {
                    name: name.to_string(),
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
    pub total_skipped: u64,
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
            total_skipped: 0,
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

        if record.skipped {
            self.total_skipped += 1;
        } else {
            if !record.success {
                self.total_failures += 1;
            }
            self.latency.record(record.duration);
            self.status_codes.record(record.status_code);
        }

        self.scenario_metrics.record(
            record.scenario.as_ref(),
            record.step.as_ref(),
            record.duration,
            record.success,
            record.status_code,
            record.skipped,
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
    pub total_skipped: u64,
    pub template_stats: Option<TemplateStats>,
    pub response_stats: Option<ResponseStats>,
    pub curve_stats: Option<CurveStats>,
    /// Present when scenario/step attribution data was emitted by VUs.
    pub scenario_stats: Option<Vec<ScenarioStats>>,
}

// ── OnStepFailure ─────────────────────────────────────────────────────────────

/// Controls what happens when a step fails within a scenario iteration.
#[derive(Debug, Clone, Copy, Default)]
pub enum OnStepFailure {
    /// Continue executing remaining steps in the iteration even after a failure.
    #[default]
    Continue,
    /// Abort the current iteration and skip remaining steps on any step failure.
    AbortIteration,
}

// ── ResolvedStep ──────────────────────────────────────────────────────────────

/// A fully resolved scenario step, ready to be executed by a `ScenarioVu`.
pub struct ResolvedStep {
    pub name: Arc<str>,
    pub request_config: Arc<RequestConfig>,
    pub plain_headers: Arc<Vec<(String, String)>>,
    pub request_template: Option<Arc<Template>>,
    pub response_template: Option<Arc<Vec<TrackedField>>>,
    /// Capture definitions to extract from the response body.
    pub captures: Vec<CaptureDefinition>,
    /// Inline request body (mutually exclusive with `request_template`).
    pub inline_body: Option<Arc<str>>,
    /// True if any header value contains `{{capture.` references.
    pub has_capture_headers: bool,
}

// ── ResolvedScenario ──────────────────────────────────────────────────────────

/// A fully resolved scenario with its steps, ready for VU assignment.
pub struct ResolvedScenario {
    pub name: Arc<str>,
    pub weight: u32,
    pub on_step_failure: OnStepFailure,
    pub steps: Vec<ResolvedStep>,
}

// ── RequestSpec ───────────────────────────────────────────────────────────────

/// All request-level parameters for a run.
pub enum RequestSpec {
    /// Single-endpoint mode — all VUs hit the same host/method.
    Single {
        host: String,
        method: HttpMethod,
        body: Option<Body>,
        template_path: Option<PathBuf>,
        response_template_path: Option<PathBuf>,
        /// Custom HTTP headers to send with every request in this run.
        headers: Vec<(String, SensitiveString)>,
    },
    /// Scenario mode — VUs execute multi-step sequences.
    Scenarios(Vec<ResolvedScenario>),
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

/// Maps a VU index to a scenario index using weighted round-robin assignment.
///
/// VUs are assigned to scenarios proportionally to each scenario's `weight`.
/// For example, with weights `[3, 1]` and 8 VUs: VUs 0–2 → scenario 0,
/// VU 3 → scenario 1, VU 4–6 → scenario 0, VU 7 → scenario 1.
///
/// Panics if `scenarios` is empty — callers must ensure at least one scenario exists.
pub fn assign_scenario(vu_index: usize, scenarios: &[ResolvedScenario]) -> usize {
    let total_weight: u32 = scenarios.iter().map(|s| s.weight).sum();
    let slot = (vu_index as u32) % total_weight;
    let mut cumulative = 0u32;
    for (i, s) in scenarios.iter().enumerate() {
        cumulative += s.weight;
        if slot < cumulative {
            return i;
        }
    }
    scenarios.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    // ── ScenarioMetricsAccumulator ────────────────────────────────────────────

    #[test]
    fn scenario_record_none_stays_empty() {
        let mut acc = ScenarioMetricsAccumulator::default();
        acc.record(
            None,
            None,
            Duration::from_millis(10),
            true,
            Some(200),
            false,
        );
        assert!(acc.into_stats().is_none());
    }

    #[test]
    fn scenario_empty_into_stats_returns_none() {
        let acc = ScenarioMetricsAccumulator::default();
        assert!(acc.into_stats().is_none());
    }

    #[test]
    fn two_scenarios_same_step_name_independent() {
        let mut acc = ScenarioMetricsAccumulator::default();

        let scenario_a: Arc<str> = Arc::from("A");
        let scenario_b: Arc<str> = Arc::from("B");
        let step_login: Arc<str> = Arc::from("login");

        // 2 requests for scenario A / step login
        acc.record(
            Some(&scenario_a),
            Some(&step_login),
            Duration::from_millis(10),
            true,
            Some(200),
            false,
        );
        acc.record(
            Some(&scenario_a),
            Some(&step_login),
            Duration::from_millis(15),
            true,
            Some(200),
            false,
        );

        // 3 requests for scenario B / step login
        acc.record(
            Some(&scenario_b),
            Some(&step_login),
            Duration::from_millis(20),
            true,
            Some(200),
            false,
        );
        acc.record(
            Some(&scenario_b),
            Some(&step_login),
            Duration::from_millis(25),
            true,
            Some(200),
            false,
        );
        acc.record(
            Some(&scenario_b),
            Some(&step_login),
            Duration::from_millis(30),
            true,
            Some(200),
            false,
        );

        let stats = acc.into_stats().expect("should have scenario stats");
        assert_eq!(stats.len(), 2);

        // into_stats() sorts by name, so A comes before B
        assert_eq!(stats[0].name, "A");
        assert_eq!(stats[1].name, "B");

        let a_login = stats[0]
            .steps
            .iter()
            .find(|s| s.name == "login")
            .expect("A should have login step");
        assert_eq!(a_login.requests.total_requests, 2);

        let b_login = stats[1]
            .steps
            .iter()
            .find(|s| s.name == "login")
            .expect("B should have login step");
        assert_eq!(b_login.requests.total_requests, 3);
    }

    // ── DrainMetricsAccumulator ───────────────────────────────────────────────

    #[test]
    fn drain_accumulator_no_scenario_labels() {
        let mut acc = DrainMetricsAccumulator::new(false);

        let record = RequestRecord {
            duration: Duration::from_millis(50),
            completed_at: Instant::now(),
            success: true,
            status_code: Some(200),
            extraction: None,
            scenario: None,
            step: None,
            skipped: false,
        };

        acc.record_request(&record);

        assert_eq!(acc.total_requests, 1);
        assert!(acc.finalize_scenario_stats().is_none());
    }

    #[test]
    fn drain_accumulator_skipped_records() {
        let mut acc = DrainMetricsAccumulator::new(false);

        let scenario: Arc<str> = Arc::from("checkout");
        let step: Arc<str> = Arc::from("pay");

        // Normal record
        let normal = RequestRecord {
            duration: Duration::from_millis(50),
            completed_at: Instant::now(),
            success: true,
            status_code: Some(200),
            extraction: None,
            scenario: Some(Arc::clone(&scenario)),
            step: Some(Arc::clone(&step)),
            skipped: false,
        };
        acc.record_request(&normal);

        // Skipped record
        let skipped = RequestRecord {
            duration: Duration::ZERO,
            completed_at: Instant::now(),
            success: false,
            status_code: None,
            extraction: None,
            scenario: Some(Arc::clone(&scenario)),
            step: Some(Arc::clone(&step)),
            skipped: true,
        };
        acc.record_request(&skipped);

        assert_eq!(acc.total_requests, 2);
        assert_eq!(acc.total_failures, 0, "skipped records are not failures");
        assert_eq!(acc.total_skipped, 1);

        let scenarios = acc.finalize_scenario_stats().unwrap();
        let checkout = &scenarios[0];
        assert_eq!(checkout.requests.total_requests, 2);
        assert_eq!(checkout.requests.total_skipped, 1);
        assert_eq!(checkout.requests.total_failures, 0);

        let pay_step = checkout.steps.iter().find(|s| s.name == "pay").unwrap();
        assert_eq!(pay_step.requests.total_requests, 2);
        assert_eq!(pay_step.requests.total_skipped, 1);
    }

    // ── assign_scenario ───────────────────────────────────────────────────────

    fn make_scenario(name: &str, weight: u32) -> ResolvedScenario {
        ResolvedScenario {
            name: Arc::from(name),
            weight,
            on_step_failure: OnStepFailure::Continue,
            steps: vec![],
        }
    }

    #[test]
    fn assign_scenario_weighted() {
        // Weights [3, 1] → total 4. With 8 VUs the pattern repeats twice:
        // slot 0,1,2 → scenario 0; slot 3 → scenario 1.
        let scenarios = vec![make_scenario("A", 3), make_scenario("B", 1)];

        let assignments: Vec<usize> = (0..8).map(|i| assign_scenario(i, &scenarios)).collect();

        // First cycle (VUs 0–3)
        assert_eq!(assignments[0], 0);
        assert_eq!(assignments[1], 0);
        assert_eq!(assignments[2], 0);
        assert_eq!(assignments[3], 1);
        // Second cycle (VUs 4–7) — identical pattern
        assert_eq!(assignments[4], 0);
        assert_eq!(assignments[5], 0);
        assert_eq!(assignments[6], 0);
        assert_eq!(assignments[7], 1);
    }

    #[test]
    fn assign_scenario_single() {
        let scenarios = vec![make_scenario("only", 5)];
        for i in 0..10 {
            assert_eq!(assign_scenario(i, &scenarios), 0);
        }
    }

    #[test]
    fn assign_scenario_equal_weights() {
        let scenarios = vec![make_scenario("A", 1), make_scenario("B", 1)];
        // Alternating: 0, 1, 0, 1, ...
        for i in 0..8 {
            assert_eq!(assign_scenario(i, &scenarios), i % 2);
        }
    }
}

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
