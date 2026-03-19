use serde::{Deserialize, Serialize};

use crate::output::RunReport;

// ── Metric ────────────────────────────────────────────────────────────────────

/// A measurable metric from a completed load test run.
///
/// Variants map directly to fields on `RunReport.latency` or `RunReport.requests`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    LatencyMin,
    LatencyP10,
    LatencyP25,
    LatencyP50,
    LatencyP75,
    LatencyP90,
    LatencyP95,
    LatencyP99,
    LatencyMax,
    LatencyAvg,
    ErrorRate,
    ThroughputRps,
}

impl Metric {
    /// Resolves the metric to its `f64` value from a completed `RunReport`.
    pub fn resolve(&self, report: &RunReport) -> f64 {
        match self {
            Self::LatencyMin => report.latency.min_ms,
            Self::LatencyP10 => report.latency.p10_ms,
            Self::LatencyP25 => report.latency.p25_ms,
            Self::LatencyP50 => report.latency.p50_ms,
            Self::LatencyP75 => report.latency.p75_ms,
            Self::LatencyP90 => report.latency.p90_ms,
            Self::LatencyP95 => report.latency.p95_ms,
            Self::LatencyP99 => report.latency.p99_ms,
            Self::LatencyMax => report.latency.max_ms,
            Self::LatencyAvg => report.latency.avg_ms,
            Self::ErrorRate => report.requests.error_rate,
            Self::ThroughputRps => report.requests.throughput_rps,
        }
    }
}

// ── Operator ──────────────────────────────────────────────────────────────────

/// Comparison operator for a threshold rule.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Lt,
    Lte,
    Gt,
    Gte,
    Eq,
}

impl Operator {
    /// Returns `true` when `actual` satisfies the operator against `threshold`.
    ///
    /// For `Eq`, an epsilon comparison is used to accommodate floating-point
    /// representation imprecision.
    pub fn evaluate(&self, actual: f64, threshold: f64) -> bool {
        // NaN/Inf actual values always fail — a non-finite metric cannot satisfy
        // any threshold. This guards against degenerate run data silently passing.
        if !actual.is_finite() {
            return false;
        }
        match self {
            Self::Lt => actual < threshold,
            Self::Lte => actual <= threshold,
            Self::Gt => actual > threshold,
            Self::Gte => actual >= threshold,
            Self::Eq => (actual - threshold).abs() < f64::EPSILON * threshold.abs().max(1.0),
        }
    }

    /// Returns the symbolic representation of the operator (e.g. `"<"`).
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Eq => "==",
        }
    }
}

// ── Threshold ─────────────────────────────────────────────────────────────────

/// A single pass/fail rule: `metric operator value`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Threshold {
    pub metric: Metric,
    pub operator: Operator,
    pub value: f64,
}

// ── ThresholdResult ───────────────────────────────────────────────────────────

/// The outcome of evaluating one `Threshold` against a `RunReport`.
#[derive(Debug, Clone, Serialize)]
pub struct ThresholdResult {
    pub threshold: Threshold,
    /// The actual metric value observed in the run.
    pub actual: f64,
    pub passed: bool,
}

// ── ThresholdReport ───────────────────────────────────────────────────────────

/// Aggregated results from evaluating all thresholds against a `RunReport`.
#[derive(Debug, Clone, Serialize)]
pub struct ThresholdReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<ThresholdResult>,
}

impl ThresholdReport {
    /// Returns `true` only when every threshold passed.
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

// ── EvaluateParams ────────────────────────────────────────────────────────────

/// Parameters required to evaluate a set of thresholds against a run report.
pub struct EvaluateParams<'a> {
    pub report: &'a RunReport,
    pub thresholds: &'a [Threshold],
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::command::run::{RunMode, RunStats};
    use crate::http::RequestResult;
    use crate::output::{RunReport, RunReportParams};

    use super::*;

    fn make_report() -> RunReport {
        let result = RequestResult::new(Duration::from_millis(99), true, Some(200), None);
        let stats = RunStats {
            elapsed: Duration::from_secs(10),
            template_duration: None,
            response_stats: None,
            results: vec![result],
            mode: RunMode::Fixed,
            curve_duration: None,
            curve_stages: None,
            total_requests: 100,
            total_failures: 5,
            sample_rate: 1.0,
            min_sample_rate: 1.0,
        };
        RunReport::from_params(RunReportParams {
            stats: &stats,
            reservoir_size: 100_000,
            run_start: std::time::Instant::now(),
        })
    }

    #[test]
    fn metric_resolve_latency_p99() {
        let report = make_report();
        let val = Metric::LatencyP99.resolve(&report);
        // p99 comes from the single 99ms result
        assert_eq!(val, report.latency.p99_ms);
    }

    #[test]
    fn metric_resolve_error_rate() {
        let report = make_report();
        let val = Metric::ErrorRate.resolve(&report);
        assert_eq!(val, report.requests.error_rate);
        // 5 failures out of 100
        assert!((val - 0.05).abs() < 1e-9);
    }

    #[test]
    fn metric_resolve_throughput_rps() {
        let report = make_report();
        let val = Metric::ThroughputRps.resolve(&report);
        assert_eq!(val, report.requests.throughput_rps);
        // 100 requests / 10 seconds = 10.0
        assert!((val - 10.0).abs() < 1e-6);
    }

    #[test]
    fn operator_lt_true() {
        assert!(Operator::Lt.evaluate(5.0, 10.0));
    }

    #[test]
    fn operator_lt_false() {
        assert!(!Operator::Lt.evaluate(10.0, 5.0));
    }

    #[test]
    fn operator_eq_epsilon() {
        // Values within epsilon should pass
        let base = 100.0_f64;
        let within = base + f64::EPSILON * 0.5;
        assert!(Operator::Eq.evaluate(within, base));

        // Values clearly outside epsilon should fail
        let outside = base + 1.0;
        assert!(!Operator::Eq.evaluate(outside, base));
    }

    #[test]
    fn threshold_report_all_passed_true() {
        let report = ThresholdReport {
            total: 2,
            passed: 2,
            failed: 0,
            results: vec![],
        };
        assert!(report.all_passed());
    }

    #[test]
    fn threshold_report_all_passed_false() {
        let report = ThresholdReport {
            total: 2,
            passed: 1,
            failed: 1,
            results: vec![],
        };
        assert!(!report.all_passed());
    }
}
