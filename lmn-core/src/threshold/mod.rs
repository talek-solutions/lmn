pub mod error;
pub(crate) mod parse;
mod types;

pub use error::ThresholdError;
pub use parse::parse_thresholds;
pub use types::{EvaluateParams, Metric, Operator, Threshold, ThresholdReport, ThresholdResult};

/// Evaluates all thresholds in `params.thresholds` against `params.report`.
///
/// Returns a `ThresholdReport` summarising the total, passed, and failed
/// counts along with per-threshold results.
pub fn evaluate(params: EvaluateParams<'_>) -> ThresholdReport {
    let mut results = Vec::with_capacity(params.thresholds.len());

    for threshold in params.thresholds {
        let actual = threshold.metric.resolve(params.report);
        let passed = threshold.operator.evaluate(actual, threshold.value);
        results.push(ThresholdResult {
            threshold: threshold.clone(),
            actual,
            passed,
        });
    }

    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = total - passed;

    ThresholdReport {
        total,
        passed,
        failed,
        results,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::execution::{RunMode, RunStats};
    use crate::histogram::{LatencyHistogram, StatusCodeHistogram};
    use crate::output::{RunReport, RunReportParams};

    use super::*;

    fn make_report_with_latency(latency_ms: u64, error_rate: f64) -> RunReport {
        let total = 100u64;
        let failed = (total as f64 * error_rate).round() as u64;
        let mut latency = LatencyHistogram::new();
        latency.record(Duration::from_millis(latency_ms));
        let mut status_codes = StatusCodeHistogram::new();
        status_codes.record(Some(200));

        let stats = RunStats {
            elapsed: Duration::from_secs(10),
            mode: RunMode::Fixed,
            latency,
            status_codes,
            total_requests: total,
            total_failures: failed,
            total_skipped: 0,
            template_stats: None,
            response_stats: None,
            curve_stats: None,
            scenario_stats: None,
        };
        RunReport::from_params(RunReportParams { stats: &stats })
    }

    #[test]
    fn evaluate_all_pass() {
        let report = make_report_with_latency(50, 0.01);
        let thresholds = vec![
            Threshold {
                metric: Metric::LatencyP99,
                operator: Operator::Lt,
                value: 500.0,
            },
            Threshold {
                metric: Metric::ErrorRate,
                operator: Operator::Lte,
                value: 0.05,
            },
        ];
        let result = evaluate(EvaluateParams {
            report: &report,
            thresholds: &thresholds,
        });
        assert_eq!(result.total, 2);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 0);
        assert!(result.all_passed());
    }

    #[test]
    fn evaluate_all_fail() {
        let report = make_report_with_latency(500, 0.5);
        let thresholds = vec![
            Threshold {
                metric: Metric::LatencyP99,
                operator: Operator::Lt,
                value: 10.0,
            },
            Threshold {
                metric: Metric::ErrorRate,
                operator: Operator::Lt,
                value: 0.01,
            },
        ];
        let result = evaluate(EvaluateParams {
            report: &report,
            thresholds: &thresholds,
        });
        assert_eq!(result.total, 2);
        assert_eq!(result.passed, 0);
        assert_eq!(result.failed, 2);
        assert!(!result.all_passed());
    }

    #[test]
    fn evaluate_mixed_results() {
        let report = make_report_with_latency(50, 0.5);
        let thresholds = vec![
            // passes: p99 < 500ms
            Threshold {
                metric: Metric::LatencyP99,
                operator: Operator::Lt,
                value: 500.0,
            },
            // fails: error_rate < 0.01 but actual is 0.5
            Threshold {
                metric: Metric::ErrorRate,
                operator: Operator::Lt,
                value: 0.01,
            },
        ];
        let result = evaluate(EvaluateParams {
            report: &report,
            thresholds: &thresholds,
        });
        assert_eq!(result.total, 2);
        assert_eq!(result.passed, 1);
        assert_eq!(result.failed, 1);
        assert!(!result.all_passed());
    }

    #[test]
    fn evaluate_empty_thresholds_returns_empty_report() {
        let report = make_report_with_latency(50, 0.01);
        let thresholds: Vec<Threshold> = vec![];
        let result = evaluate(EvaluateParams {
            report: &report,
            thresholds: &thresholds,
        });
        assert_eq!(result.total, 0);
        assert_eq!(result.passed, 0);
        assert_eq!(result.failed, 0);
        assert!(result.results.is_empty());
        assert!(result.all_passed());
    }
}
