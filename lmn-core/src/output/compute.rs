use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::http::RequestResult;
use crate::load_curve::{RampType, Stage};
use crate::response_template::stats::ResponseStats;
use crate::stats::Distribution;
use crate::stats::LatencyDistribution;

use super::report::{FloatFieldSummary, LatencyStats, ResponseStatsReport, StageReport};

// ── latency_stats ─────────────────────────────────────────────────────────────

/// Constructs a `LatencyDistribution` from a `RequestResult` slice and snapshots
/// the specific quantiles required by the output schema into a `LatencyStats` struct.
///
/// The quantile logic lives entirely in `LatencyDistribution`; this function only
/// decides which quantiles to snapshot.
pub fn latency_stats(results: &[RequestResult]) -> LatencyStats {
    let dist = LatencyDistribution::from_results(results);
    LatencyStats {
        min_ms: dist.min_ms(),
        p10_ms: dist.quantile_ms(0.10),
        p25_ms: dist.quantile_ms(0.25),
        p50_ms: dist.quantile_ms(0.50),
        p75_ms: dist.quantile_ms(0.75),
        p90_ms: dist.quantile_ms(0.90),
        p95_ms: dist.quantile_ms(0.95),
        p99_ms: dist.quantile_ms(0.99),
        max_ms: dist.max_ms(),
        avg_ms: dist.mean_ms(),
    }
}

// ── latency_stats_from_subset ─────────────────────────────────────────────────

/// Same as `latency_stats` but operates on a pre-filtered subset of results.
/// Used by per-stage computation to avoid re-constructing the full distribution.
fn latency_stats_from_subset(subset: &[&RequestResult]) -> LatencyStats {
    let ms_values: Vec<f64> = subset
        .iter()
        .map(|r| r.duration.as_secs_f64() * 1000.0)
        .collect();
    let dist = Distribution::from_unsorted(ms_values);
    LatencyStats {
        min_ms: dist.min(),
        p10_ms: dist.quantile(0.10),
        p25_ms: dist.quantile(0.25),
        p50_ms: dist.quantile(0.50),
        p75_ms: dist.quantile(0.75),
        p90_ms: dist.quantile(0.90),
        p95_ms: dist.quantile(0.95),
        p99_ms: dist.quantile(0.99),
        max_ms: dist.max(),
        avg_ms: dist.mean(),
    }
}

// ── status_code_map ───────────────────────────────────────────────────────────

/// Builds a `BTreeMap` of HTTP status code strings to counts.
///
/// `None` status (connection errors with no HTTP response) maps to the key `"error"`.
/// Uses `BTreeMap` for stable JSON key ordering in the output schema.
pub fn status_code_map(results: &[RequestResult]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for r in results {
        let key = match r.status_code {
            Some(code) => code.to_string(),
            None => "error".to_string(),
        };
        *map.entry(key).or_insert(0usize) += 1;
    }
    map
}

// ── throughput ────────────────────────────────────────────────────────────────

/// Computes throughput as requests per second.
///
/// Returns `0.0` when elapsed is zero (avoids divide-by-zero).
pub fn throughput(total_requests: usize, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        0.0
    } else {
        total_requests as f64 / secs
    }
}

// ── error_rate ────────────────────────────────────────────────────────────────

/// Computes the fraction of failed requests: `failed / total`.
///
/// Returns `0.0` when `total == 0` (avoids divide-by-zero).
pub fn error_rate(total_requests: usize, total_failures: usize) -> f64 {
    if total_requests == 0 {
        0.0
    } else {
        (total_failures as f64 / total_requests as f64).min(1.0)
    }
}

// ── response_stats_report ─────────────────────────────────────────────────────

/// Converts `ResponseStats` into a serialization-ready `ResponseStatsReport`.
///
/// - `string_distributions` and `mismatch_counts` are promoted from `HashMap` to
///   `BTreeMap` for stable JSON key ordering.
/// - Each `FloatFieldStats` accumulator is converted to a `Distribution` and the
///   min/avg/p50/p95/p99/max summary is computed. Fields with no values are omitted.
pub fn response_stats_report(rs: &ResponseStats) -> ResponseStatsReport {
    // string_fields: promote HashMap → BTreeMap at both levels
    let string_fields: BTreeMap<String, BTreeMap<String, usize>> = rs
        .string_distributions
        .iter()
        .map(|(k, dist)| {
            let inner: BTreeMap<String, usize> =
                dist.iter().map(|(v, c)| (v.clone(), *c)).collect();
            (k.clone(), inner)
        })
        .collect();

    // float_fields: sort values, compute summary, promote to BTreeMap; skip empty fields
    let float_fields: BTreeMap<String, FloatFieldSummary> = rs
        .float_fields
        .iter()
        .filter_map(|(k, ffs)| {
            if ffs.values.is_empty() {
                return None;
            }
            let dist = Distribution::from_unsorted(ffs.values.clone());
            let n = dist.len();
            // Integer-index formula: (n * p / 100).min(n-1) matches the ASCII table
            // renderer in lumen-cli/src/cli/output.rs. value_at() avoids the float round-trip.
            let p50_idx = (n * 50 / 100).min(n - 1);
            let p95_idx = (n * 95 / 100).min(n - 1);
            let p99_idx = (n * 99 / 100).min(n - 1);
            Some((
                k.clone(),
                FloatFieldSummary {
                    min: dist.min(),
                    avg: dist.mean(),
                    p50: dist.value_at(p50_idx),
                    p95: dist.value_at(p95_idx),
                    p99: dist.value_at(p99_idx),
                    max: dist.max(),
                },
            ))
        })
        .collect();

    // mismatch_counts: promote HashMap → BTreeMap
    let mismatch_counts: BTreeMap<String, usize> = rs
        .mismatch_counts
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();

    ResponseStatsReport {
        responses_parsed: rs.total_responses,
        string_fields,
        float_fields,
        mismatch_counts,
    }
}

// ── per_stage_reports ─────────────────────────────────────────────────────────

/// Buckets results by `completed_at` into stage windows and builds per-stage metrics.
///
/// Each result is attributed to the stage whose wall-clock window contains the
/// result's `completed_at` timestamp. The stage window is `[stage_start, stage_end)`.
///
/// Since the reservoir is a uniform random sample, per-stage percentiles are
/// representative within the same caveats as global percentiles.
pub fn per_stage_reports(
    results: &[RequestResult],
    stages: &[Stage],
    run_start: Instant,
) -> Vec<StageReport> {
    // Pre-compute stage windows as (start_duration, end_duration) offsets from run_start
    let mut windows: Vec<(Duration, Duration)> = Vec::with_capacity(stages.len());
    let mut offset = Duration::ZERO;
    for stage in stages {
        let start = offset;
        let end = offset + stage.duration;
        windows.push((start, end));
        offset = end;
    }

    stages
        .iter()
        .enumerate()
        .map(|(i, stage)| {
            let (win_start, win_end) = windows[i];

            // Bucket results that completed within this stage's window
            let subset: Vec<&RequestResult> = results
                .iter()
                .filter(|r| {
                    let elapsed = r
                        .completed_at
                        .checked_duration_since(run_start)
                        .unwrap_or(Duration::ZERO);
                    elapsed >= win_start && elapsed < win_end
                })
                .collect();

            let stage_total = subset.len();
            let stage_failed = subset.iter().filter(|r| !r.success).count();
            let stage_ok = stage_total - stage_failed;

            let latency = latency_stats_from_subset(&subset);
            let stage_elapsed = stage.duration;

            StageReport {
                index: i,
                duration_ms: stage.duration.as_secs_f64() * 1000.0,
                target_vus: stage.target_vus,
                ramp: match stage.ramp {
                    RampType::Linear => "linear".to_string(),
                    RampType::Step => "step".to_string(),
                },
                requests: stage_total,
                ok: stage_ok,
                failed: stage_failed,
                error_rate: error_rate(stage_total, stage_failed),
                throughput_rps: throughput(stage_total, stage_elapsed),
                latency,
            }
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::RequestResult;
    use crate::response_template::stats::{FloatFieldStats, ResponseStats};

    fn make_result(duration_ms: u64, success: bool, status: Option<u16>) -> RequestResult {
        RequestResult::new(Duration::from_millis(duration_ms), success, status, None)
    }

    // ── latency_stats ─────────────────────────────────────────────────────────

    #[test]
    fn latency_stats_correct_for_known_input() {
        // 100 results: durations 1ms to 100ms
        let results: Vec<RequestResult> =
            (1..=100).map(|i| make_result(i, true, Some(200))).collect();
        let stats = latency_stats(&results);
        assert_eq!(stats.min_ms, 1.0);
        assert_eq!(stats.max_ms, 100.0);
        // p50: idx = floor(100 * 0.5) = 50 → 51ms
        assert_eq!(stats.p50_ms, 51.0);
        // p99: idx = floor(100 * 0.99) = 99 → 100ms
        assert_eq!(stats.p99_ms, 100.0);
    }

    #[test]
    fn latency_stats_empty_input() {
        let stats = latency_stats(&[]);
        assert_eq!(stats.min_ms, 0.0);
        assert_eq!(stats.p50_ms, 0.0);
        assert_eq!(stats.p99_ms, 0.0);
        assert_eq!(stats.max_ms, 0.0);
        assert_eq!(stats.avg_ms, 0.0);
    }

    // ── status_code_map ───────────────────────────────────────────────────────

    #[test]
    fn status_code_map_groups_correctly() {
        let results = vec![
            make_result(10, true, Some(200)),
            make_result(10, true, Some(200)),
            make_result(10, false, Some(404)),
            make_result(10, false, None),
            make_result(10, false, None),
        ];
        let map = status_code_map(&results);
        assert_eq!(map["200"], 2);
        assert_eq!(map["404"], 1);
        assert_eq!(map["error"], 2);
    }

    #[test]
    fn status_code_map_empty_input() {
        let map = status_code_map(&[]);
        assert!(map.is_empty());
    }

    #[test]
    fn status_code_map_keys_are_sorted() {
        let results = vec![
            make_result(10, false, Some(503)),
            make_result(10, true, Some(200)),
            make_result(10, false, None),
            make_result(10, false, Some(404)),
        ];
        let map = status_code_map(&results);
        let keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
        // BTreeMap sorts lexicographically; "200" < "404" < "503" < "error"
        assert_eq!(keys, vec!["200", "404", "503", "error"]);
    }

    // ── throughput ────────────────────────────────────────────────────────────

    #[test]
    fn throughput_correct() {
        let rps = throughput(1000, Duration::from_secs(10));
        assert!((rps - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn throughput_zero_elapsed() {
        assert_eq!(throughput(1000, Duration::ZERO), 0.0);
    }

    // ── error_rate ────────────────────────────────────────────────────────────

    #[test]
    fn error_rate_correct() {
        let rate = error_rate(100, 10);
        assert!((rate - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn error_rate_zero_requests() {
        assert_eq!(error_rate(0, 0), 0.0);
    }

    #[test]
    fn error_rate_no_failures() {
        assert_eq!(error_rate(100, 0), 0.0);
    }

    #[test]
    fn error_rate_cannot_exceed_one() {
        // failures > total is a data-corruption scenario; rate must be clamped to 1.0
        assert_eq!(error_rate(10, 20), 1.0);
    }

    // ── response_stats_report ─────────────────────────────────────────────────

    fn make_response_stats() -> ResponseStats {
        use crate::response_template::extractor::{ExtractedValue, ExtractionResult};
        let mut rs = ResponseStats::new();
        // Record 3 results with a string field and a float field
        for i in 0..3 {
            rs.record(ExtractionResult {
                values: vec![
                    (
                        "status".to_string(),
                        ExtractedValue::String("ok".to_string()),
                    ),
                    ("score".to_string(), ExtractedValue::Float((i + 1) as f64)),
                ],
                mismatches: vec![],
            });
        }
        rs
    }

    #[test]
    fn response_stats_report_float_percentiles() {
        // scores: [1.0, 2.0, 3.0] sorted
        // p50: idx = 3 * 50 / 100 = 1 → 2.0
        // p95: idx = (3 * 95 / 100).min(2) = (2).min(2) = 2 → 3.0
        // p99: idx = (3 * 99 / 100).min(2) = (2).min(2) = 2 → 3.0
        let rs = make_response_stats();
        let report = response_stats_report(&rs);
        let score = &report.float_fields["score"];
        assert_eq!(score.min, 1.0);
        assert_eq!(score.max, 3.0);
        assert!((score.avg - 2.0).abs() < f64::EPSILON);
        assert_eq!(score.p50, 2.0);
        assert_eq!(score.p95, 3.0);
        assert_eq!(score.p99, 3.0);
    }

    #[test]
    fn response_stats_report_string_fields_sorted() {
        use crate::response_template::extractor::{ExtractedValue, ExtractionResult};
        let mut rs = ResponseStats::new();
        // Insert string fields out of alphabetical order
        rs.record(ExtractionResult {
            values: vec![
                ("zebra".to_string(), ExtractedValue::String("z".to_string())),
                ("alpha".to_string(), ExtractedValue::String("a".to_string())),
                ("mango".to_string(), ExtractedValue::String("m".to_string())),
            ],
            mismatches: vec![],
        });
        let report = response_stats_report(&rs);
        let keys: Vec<&str> = report.string_fields.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["alpha", "mango", "zebra"]);
    }

    #[test]
    fn response_stats_report_mismatch_counts_sorted() {
        use crate::response_template::extractor::ExtractionResult;
        let mut rs = ResponseStats::new();
        rs.record(ExtractionResult {
            values: vec![],
            mismatches: vec!["z_field".to_string(), "a_field".to_string()],
        });
        let report = response_stats_report(&rs);
        let keys: Vec<&str> = report.mismatch_counts.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["a_field", "z_field"]);
    }

    #[test]
    fn response_stats_report_responses_parsed_reflects_sample() {
        use crate::response_template::extractor::{ExtractedValue, ExtractionResult};
        let mut rs = ResponseStats::new();
        for _ in 0..7 {
            rs.record(ExtractionResult {
                values: vec![("f".to_string(), ExtractedValue::String("v".to_string()))],
                mismatches: vec![],
            });
        }
        let report = response_stats_report(&rs);
        // responses_parsed must equal total_responses from the reservoir (7),
        // not any larger count such as total_requests
        assert_eq!(report.responses_parsed, 7);
    }

    #[test]
    fn response_stats_report_empty_float_field_omitted() {
        let mut rs = ResponseStats::new();
        // Manually insert an empty float field
        rs.float_fields
            .insert("empty".to_string(), FloatFieldStats { values: vec![] });
        let report = response_stats_report(&rs);
        assert!(!report.float_fields.contains_key("empty"));
    }
}
