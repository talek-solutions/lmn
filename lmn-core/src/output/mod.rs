mod compute;
mod report;

pub use report::{
    FloatFieldSummary, LatencyStats, RequestSummary, ResponseStatsReport, RunMeta, RunReport,
    SamplingInfo, StageReport,
};

use std::time::Instant;

use crate::execution::{RunMode, RunStats};

use compute::{
    error_rate, latency_stats, per_stage_reports, response_stats_report, status_code_map,
    throughput,
};

// ── RunReportParams ───────────────────────────────────────────────────────────

/// Parameters required to build a `RunReport` from a completed `RunStats`.
///
/// `reservoir_size` and `run_start` are not stored on `RunStats` (they are inputs
/// to the run, not results). They are threaded through here to avoid polluting
/// the core run type with output-layer concerns.
///
/// # Invariant
/// `run_start` must be the `Instant` captured immediately before the first
/// request was dispatched, so that `completed_at - run_start` correctly maps
/// each result to its stage window.
pub struct RunReportParams<'a> {
    pub stats: &'a RunStats,
    /// Configured reservoir cap (`--result-buffer`). Passed from CLI args.
    pub reservoir_size: usize,
    /// Wall-clock instant at which the run started (before first request fired).
    ///
    /// **Why this field exists beyond the original spec:** `per_stage_reports` buckets
    /// each `RequestResult` into a stage window by computing
    /// `completed_at.checked_duration_since(run_start)` and comparing it against the
    /// cumulative `[stage_start, stage_end)` offsets. Without this anchor, wall-clock
    /// timestamps cannot be mapped to stage windows.
    ///
    /// **Why `from_params` (fixed mode) accepts it but discards it:** API uniformity.
    /// Both fixed and curve callers construct a single `RunReportParams` and pass it to
    /// `from_params` or `from_params_with_curve` without needing to know which variant
    /// requires `run_start`. Keeping one param struct prevents callers from branching on
    /// mode before building params.
    pub run_start: Instant,
}

// ── RunReport ─────────────────────────────────────────────────────────────────

impl RunReport {
    /// Constructs a `RunReport` from a completed `RunStats` and supplementary
    /// parameters that are not stored on `RunStats`.
    ///
    /// All derived metrics (percentiles, throughput, error rate, status code map,
    /// response stats, per-stage breakdowns) are computed here via the compute
    /// module functions.
    pub fn from_params(params: RunReportParams<'_>) -> Self {
        let RunReportParams {
            stats,
            reservoir_size,
            run_start: _,
        } = params;

        let total = stats.total_requests;
        let failed = stats.total_failures;
        let ok = total.saturating_sub(failed);

        let mode_str = match stats.mode {
            RunMode::Fixed => "fixed".to_string(),
            RunMode::Curve => "curve".to_string(),
        };

        let run = RunMeta {
            mode: mode_str,
            elapsed_ms: stats.elapsed.as_secs_f64() * 1000.0,
            curve_duration_ms: stats.curve_duration.map(|d: std::time::Duration| d.as_secs_f64() * 1000.0),
            template_generation_ms: stats.template_duration.map(|d: std::time::Duration| d.as_secs_f64() * 1000.0),
        };

        let requests = RequestSummary {
            total,
            ok,
            failed,
            error_rate: error_rate(total, failed),
            throughput_rps: throughput(total, stats.elapsed),
        };

        let latency = latency_stats(&stats.results);
        let status_codes = status_code_map(&stats.results);

        let sampling = SamplingInfo {
            sampled: stats.min_sample_rate < 1.0,
            final_sample_rate: stats.sample_rate,
            min_sample_rate: stats.min_sample_rate,
            reservoir_size,
            results_collected: stats.results.len(),
        };

        let response_stats = stats.response_stats.as_ref().map(response_stats_report);

        let curve_stages = match stats.mode {
            RunMode::Curve => {
                // curve_duration is always Some in Curve mode, but we need the stages.
                // RunStats does not carry the LoadCurve directly — the stages are needed
                // for per-stage attribution. Since RunStats does not store them, we can
                // only produce per-stage reports when the curve is available.
                // The caller must supply the curve via the load_curve field when present.
                // For now, if RunStats has no curve_stages attached, return None.
                // This will be wired via RunReportParams.load_curve in a follow-up step
                // once the CLI layer threads it through (Step 6).
                None
            }
            RunMode::Fixed => None,
        };

        RunReport {
            version: 1,
            run,
            requests,
            latency,
            status_codes,
            sampling,
            response_stats,
            curve_stages,
            thresholds: None,
        }
    }

    /// Constructs a `RunReport` from a completed `RunStats` with curve stages.
    ///
    /// This variant is used in curve mode when the `LoadCurve` is available at the
    /// output site. `run_start` is required for per-stage result attribution.
    pub fn from_params_with_curve(
        params: RunReportParams<'_>,
        stages: &[crate::load_curve::Stage],
    ) -> Self {
        let mut report = Self::from_params(RunReportParams {
            stats: params.stats,
            reservoir_size: params.reservoir_size,
            run_start: params.run_start,
        });

        if report.run.curve_duration_ms.is_some() {
            report.curve_stages = Some(per_stage_reports(
                &params.stats.results,
                stages,
                params.run_start,
            ));
        }

        report
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::execution::{RunMode, RunStats};
    use crate::http::RequestResult;
    use crate::load_curve::{LoadCurve, RampType, Stage};
    use crate::output::{RunReport, RunReportParams};

    fn make_run_stats(
        mode: RunMode,
        results: Vec<RequestResult>,
        total_requests: usize,
        total_failures: usize,
        sample_rate: f64,
        min_sample_rate: f64,
    ) -> RunStats {
        RunStats {
            elapsed: Duration::from_secs(5),
            template_duration: None,
            response_stats: None,
            results,
            mode,
            curve_duration: if mode == RunMode::Curve {
                Some(Duration::from_secs(5))
            } else {
                None
            },
            curve_stages: None,
            total_requests,
            total_failures,
            sample_rate,
            min_sample_rate,
        }
    }

    fn make_result(duration_ms: u64, success: bool, status: Option<u16>) -> RequestResult {
        RequestResult::new(Duration::from_millis(duration_ms), success, status, None)
    }

    // ── run_report_fixed_mode_no_response_stats ────────────────────────────────

    #[test]
    fn run_report_fixed_mode_no_response_stats() {
        let stats = make_run_stats(
            RunMode::Fixed,
            vec![make_result(10, true, Some(200))],
            100,
            5,
            1.0,
            1.0,
        );
        let report = RunReport::from_params(RunReportParams {
            stats: &stats,
            reservoir_size: 100_000,
            run_start: Instant::now(),
        });

        assert_eq!(report.version, 1);
        assert_eq!(report.run.mode, "fixed");
        assert!(report.curve_stages.is_none());
        assert!(report.response_stats.is_none());
        assert!(report.run.curve_duration_ms.is_none());
    }

    // ── run_report_curve_mode_stages_populated ────────────────────────────────

    #[test]
    fn run_report_curve_mode_stages_populated() {
        // Two stages: 0..2s and 2..4s
        // We create results whose completed_at falls within each stage window.
        // Since RequestResult::new() sets completed_at to Instant::now(), we rely
        // on results being created after run_start. For test determinism, we create
        // results that will bucket correctly by faking that run_start was in the past.
        let past_start = Instant::now();

        // Sleep briefly is not ideal; instead we construct stats manually.
        // Results: 2 in stage 0 window and 2 in stage 1 window.
        // Since completed_at is set to Instant::now() inside new(), and the test
        // runs near-instantaneously, all results will fall in stage 0.
        // This is expected — the test validates structure, not exact bucketing.
        let results = vec![
            make_result(10, true, Some(200)),
            make_result(20, true, Some(200)),
            make_result(30, false, Some(503)),
            make_result(40, true, Some(200)),
        ];

        let stats = make_run_stats(RunMode::Curve, results, 4, 1, 1.0, 1.0);

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

        let curve = LoadCurve {
            stages: stages.clone(),
        };
        let _ = curve; // just to ensure it compiles

        let report = RunReport::from_params_with_curve(
            RunReportParams {
                stats: &stats,
                reservoir_size: 100_000,
                run_start: past_start,
            },
            &stages,
        );

        assert_eq!(report.version, 1);
        assert_eq!(report.run.mode, "curve");
        let stage_reports = report.curve_stages.expect("curve_stages must be Some");
        assert_eq!(stage_reports.len(), 2);
        assert_eq!(stage_reports[0].index, 0);
        assert_eq!(stage_reports[1].index, 1);
        assert_eq!(stage_reports[0].target_vus, 50);
        assert_eq!(stage_reports[1].target_vus, 100);
    }

    // ── run_report_sampling_fields_accurate ───────────────────────────────────

    #[test]
    fn run_report_sampling_fields_accurate_when_sampled() {
        let stats = make_run_stats(
            RunMode::Fixed,
            vec![make_result(10, true, Some(200))],
            10000,
            50,
            0.5,
            0.25,
        );
        let report = RunReport::from_params(RunReportParams {
            stats: &stats,
            reservoir_size: 50_000,
            run_start: Instant::now(),
        });

        assert!(
            report.sampling.sampled,
            "sampled must be true when min_sample_rate < 1.0"
        );
        assert_eq!(report.sampling.final_sample_rate, 0.5);
        assert_eq!(report.sampling.min_sample_rate, 0.25);
        assert_eq!(report.sampling.reservoir_size, 50_000);
    }

    #[test]
    fn run_report_sampling_fields_accurate_when_not_sampled() {
        let stats = make_run_stats(
            RunMode::Fixed,
            vec![make_result(10, true, Some(200))],
            100,
            0,
            1.0,
            1.0,
        );
        let report = RunReport::from_params(RunReportParams {
            stats: &stats,
            reservoir_size: 100_000,
            run_start: Instant::now(),
        });

        assert!(
            !report.sampling.sampled,
            "sampled must be false when min_sample_rate == 1.0"
        );
    }

    // ── run_report_serializes_to_valid_json ───────────────────────────────────

    #[test]
    fn run_report_serializes_to_valid_json() {
        let stats = make_run_stats(
            RunMode::Fixed,
            vec![
                make_result(10, true, Some(200)),
                make_result(20, true, Some(200)),
                make_result(15, false, None),
            ],
            3,
            1,
            1.0,
            1.0,
        );
        let report = RunReport::from_params(RunReportParams {
            stats: &stats,
            reservoir_size: 100_000,
            run_start: Instant::now(),
        });

        let json = serde_json::to_string(&report).expect("serialization must succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("output must be valid JSON");

        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["run"]["mode"], "fixed");
        assert!(parsed["requests"]["total"].is_number());
        assert!(parsed["latency"]["p50_ms"].is_number());
        assert!(parsed["sampling"]["sampled"].is_boolean());
    }
}
