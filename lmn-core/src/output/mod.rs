mod compute;
mod report;

pub use report::{
    FloatFieldSummary, LatencyStats, RequestSummary, ResponseStatsReport, RunMeta, RunReport,
    StageReport,
};

use crate::execution::{RunMode, RunStats};

use compute::{
    error_rate, latency_stats, per_stage_reports, response_stats_report, status_code_map,
    throughput,
};

// ── RunReportParams ───────────────────────────────────────────────────────────

/// Parameters required to build a `RunReport` from a completed `RunStats`.
pub struct RunReportParams<'a> {
    pub stats: &'a RunStats,
}

// ── RunReport ─────────────────────────────────────────────────────────────────

impl RunReport {
    /// Constructs a `RunReport` from a completed `RunStats`.
    ///
    /// All derived metrics (percentiles, throughput, error rate, status code map,
    /// response stats, per-stage breakdowns) are computed here via the compute
    /// module functions.
    pub fn from_params(params: RunReportParams<'_>) -> Self {
        let RunReportParams { stats } = params;

        let total = stats.total_requests as usize;
        let failed = stats.total_failures as usize;
        let ok = total.saturating_sub(failed);

        let mode_str = match stats.mode {
            RunMode::Fixed => "fixed".to_string(),
            RunMode::Curve => "curve".to_string(),
        };

        let run = RunMeta {
            mode: mode_str,
            elapsed_ms: stats.elapsed.as_secs_f64() * 1000.0,
            curve_duration_ms: stats
                .curve_stats
                .as_ref()
                .map(|cs| cs.duration.as_secs_f64() * 1000.0),
            template_generation_ms: stats
                .template_stats
                .as_ref()
                .map(|ts| ts.generation_duration.as_secs_f64() * 1000.0),
        };

        let requests = RequestSummary {
            total,
            ok,
            failed,
            error_rate: error_rate(total, failed),
            throughput_rps: throughput(total, stats.elapsed),
        };

        let latency = latency_stats(&stats.latency);
        let status_codes = status_code_map(&stats.status_codes);
        let response_stats = stats.response_stats.as_ref().map(response_stats_report);

        let curve_stages = stats
            .curve_stats
            .as_ref()
            .map(|cs| per_stage_reports(&cs.stages, &cs.stage_stats));

        RunReport {
            version: 2,
            run,
            requests,
            latency,
            status_codes,
            response_stats,
            curve_stages,
            thresholds: None,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::execution::{CurveStats, RunMode, RunStats, StageStats};
    use crate::histogram::{LatencyHistogram, StatusCodeHistogram};
    use crate::load_curve::{LoadCurve, RampType, Stage};
    use crate::output::{RunReport, RunReportParams};

    fn make_run_stats(mode: RunMode, total_requests: u64, total_failures: u64) -> RunStats {
        RunStats {
            elapsed: Duration::from_secs(5),
            mode,
            latency: LatencyHistogram::new(),
            status_codes: StatusCodeHistogram::new(),
            total_requests,
            total_failures,
            template_stats: None,
            response_stats: None,
            curve_stats: if mode == RunMode::Curve {
                Some(CurveStats {
                    duration: Duration::from_secs(5),
                    stages: vec![],
                    stage_stats: vec![],
                })
            } else {
                None
            },
        }
    }

    // ── run_report_fixed_mode_no_response_stats ────────────────────────────────

    #[test]
    fn run_report_fixed_mode_no_response_stats() {
        let stats = make_run_stats(RunMode::Fixed, 100, 5);
        let report = RunReport::from_params(RunReportParams { stats: &stats });

        assert_eq!(report.version, 2);
        assert_eq!(report.run.mode, "fixed");
        assert!(report.curve_stages.is_none());
        assert!(report.response_stats.is_none());
        assert!(report.run.curve_duration_ms.is_none());
    }

    // ── run_report_curve_mode_stages_populated ────────────────────────────────

    #[test]
    fn run_report_curve_mode_stages_populated() {
        let stages = vec![
            Stage {
                duration: Duration::from_secs(2),
                target_vus: 50,
                ramp: RampType::Linear,
            },
            Stage {
                duration: Duration::from_secs(2),
                target_vus: 100,
                ramp: RampType::Linear,
            },
        ];

        let mut stats = make_run_stats(RunMode::Curve, 4, 1);
        // Override curve_stats with the actual stages
        stats.curve_stats = Some(CurveStats {
            duration: Duration::from_secs(4),
            stages: stages.clone(),
            stage_stats: stages
                .iter()
                .map(|_| StageStats {
                    latency: LatencyHistogram::new(),
                    status_codes: StatusCodeHistogram::new(),
                    total_requests: 2,
                    total_failures: 0,
                })
                .collect(),
        });

        let curve = LoadCurve {
            stages: stages.clone(),
        };
        let _ = curve; // just to ensure it compiles

        let report = RunReport::from_params(RunReportParams { stats: &stats });

        assert_eq!(report.version, 2);
        assert_eq!(report.run.mode, "curve");
        let stage_reports = report.curve_stages.expect("curve_stages must be Some");
        assert_eq!(stage_reports.len(), 2);
        assert_eq!(stage_reports[0].index, 0);
        assert_eq!(stage_reports[1].index, 1);
        assert_eq!(stage_reports[0].target_vus, 50);
        assert_eq!(stage_reports[1].target_vus, 100);
    }

    // ── run_report_serializes_to_valid_json ───────────────────────────────────

    #[test]
    fn run_report_serializes_to_valid_json() {
        let stats = make_run_stats(RunMode::Fixed, 3, 1);
        let report = RunReport::from_params(RunReportParams { stats: &stats });

        let json = serde_json::to_string(&report).expect("serialization must succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("output must be valid JSON");

        assert_eq!(parsed["version"], 2);
        assert_eq!(parsed["run"]["mode"], "fixed");
        assert!(parsed["requests"]["total"].is_number());
        assert!(parsed["latency"]["p50_ms"].is_number());
        // No sampling field in v2
        assert!(parsed["sampling"].is_null());
    }

    // ── run_report_error_rate_computed_correctly ──────────────────────────────

    #[test]
    fn run_report_error_rate_computed_correctly() {
        let stats = make_run_stats(RunMode::Fixed, 100, 5);
        let report = RunReport::from_params(RunReportParams { stats: &stats });
        // 5 failures out of 100
        assert!((report.requests.error_rate - 0.05).abs() < 1e-9);
        assert_eq!(report.requests.total, 100);
        assert_eq!(report.requests.failed, 5);
        assert_eq!(report.requests.ok, 95);
    }
}
