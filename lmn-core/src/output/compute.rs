use std::collections::BTreeMap;
use std::time::Duration;

use crate::execution::StageStats;
use crate::histogram::{LatencyHistogram, StatusCodeHistogram};
use crate::load_curve::{RampType, Stage};
use crate::response_template::stats::ResponseStats;

use super::report::{FloatFieldSummary, LatencyStats, ResponseStatsReport, StageReport};

// ── latency_stats ─────────────────────────────────────────────────────────────

/// Snapshots the standard latency percentiles from a `LatencyHistogram` into a
/// `LatencyStats` struct for the output schema.
pub fn latency_stats(hist: &LatencyHistogram) -> LatencyStats {
    LatencyStats {
        min_ms: hist.min_ms(),
        p10_ms: hist.quantile_ms(0.10),
        p25_ms: hist.quantile_ms(0.25),
        p50_ms: hist.quantile_ms(0.50),
        p75_ms: hist.quantile_ms(0.75),
        p90_ms: hist.quantile_ms(0.90),
        p95_ms: hist.quantile_ms(0.95),
        p99_ms: hist.quantile_ms(0.99),
        max_ms: hist.max_ms(),
        avg_ms: hist.mean_ms(),
    }
}

// ── status_code_map ───────────────────────────────────────────────────────────

/// Builds a `BTreeMap` of HTTP status code strings to counts.
///
/// `None` status (connection errors with no HTTP response) maps to the key `"error"`.
/// Uses `BTreeMap` for stable JSON key ordering in the output schema.
pub fn status_code_map(hist: &StatusCodeHistogram) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    for (code, count) in hist.counts() {
        map.insert(code.to_string(), *count);
    }
    if hist.error_count() > 0 {
        map.insert("error".to_string(), hist.error_count());
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
/// - `string_fields` and `mismatch_counts` are promoted from `HashMap` to
///   `BTreeMap` for stable JSON key ordering.
/// - Each `NumericHistogram` is converted to a `Distribution` and the
///   min/avg/p50/p95/p99/max summary is computed. Empty histograms are omitted.
pub fn response_stats_report(rs: &ResponseStats) -> ResponseStatsReport {
    // string_fields: promote HashMap → BTreeMap at both levels
    let string_fields: BTreeMap<String, BTreeMap<String, u64>> = rs
        .string_fields
        .iter()
        .map(|(k, hist)| {
            let inner: BTreeMap<String, u64> =
                hist.entries().iter().map(|(v, c)| (v.clone(), *c)).collect();
            (k.clone(), inner)
        })
        .collect();

    // float_fields: derive Distribution from NumericHistogram reservoir, compute summary
    let float_fields: BTreeMap<String, FloatFieldSummary> = rs
        .float_fields
        .iter()
        .filter_map(|(k, hist)| {
            if hist.is_empty() {
                return None;
            }
            let dist = hist.distribution();
            let n = dist.len();
            // Integer-index formula: (n * p / 100).min(n-1) matches the ASCII table renderer
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
    let mismatch_counts: BTreeMap<String, u64> = rs
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

/// Builds per-stage metrics from the pre-collected `StageStats` histograms.
pub fn per_stage_reports(stages: &[Stage], stage_stats: &[StageStats]) -> Vec<StageReport> {
    stages
        .iter()
        .enumerate()
        .map(|(i, stage)| {
            let ss = &stage_stats[i];
            let lat = latency_stats(&ss.latency);
            let stage_elapsed = stage.duration;
            let stage_total = ss.total_requests as usize;
            let stage_failed = ss.total_failures as usize;
            let stage_ok = stage_total.saturating_sub(stage_failed);

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
                latency: lat,
            }
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response_template::stats::ResponseStats;
    use std::time::Duration;

    // ── latency_stats ─────────────────────────────────────────────────────────

    #[test]
    fn latency_stats_correct_for_known_input() {
        let mut hist = LatencyHistogram::new();
        for i in 1u64..=100 {
            hist.record(Duration::from_millis(i));
        }
        let stats = latency_stats(&hist);
        // HDR histogram has bounded precision — check values are in reasonable range
        assert!(stats.min_ms >= 1.0 && stats.min_ms <= 2.0, "min_ms={}", stats.min_ms);
        assert!(stats.max_ms >= 99.0 && stats.max_ms <= 101.0, "max_ms={}", stats.max_ms);
        // p99 of 100 values (1ms-100ms) should be near 100ms
        assert!(stats.p99_ms >= 98.0 && stats.p99_ms <= 101.0, "p99_ms={}", stats.p99_ms);
    }

    #[test]
    fn latency_stats_empty_input() {
        let hist = LatencyHistogram::new();
        let stats = latency_stats(&hist);
        // Empty HDR histogram returns 0 for all
        assert_eq!(stats.min_ms, 0.0);
        assert_eq!(stats.max_ms, 0.0);
        assert_eq!(stats.avg_ms, 0.0);
    }

    // ── status_code_map ───────────────────────────────────────────────────────

    #[test]
    fn status_code_map_groups_correctly() {
        let mut hist = StatusCodeHistogram::new();
        hist.record(Some(200));
        hist.record(Some(200));
        hist.record(Some(404));
        hist.record(None);
        hist.record(None);
        let map = status_code_map(&hist);
        assert_eq!(map["200"], 2);
        assert_eq!(map["404"], 1);
        assert_eq!(map["error"], 2);
    }

    #[test]
    fn status_code_map_empty_input() {
        let hist = StatusCodeHistogram::new();
        let map = status_code_map(&hist);
        assert!(map.is_empty());
    }

    #[test]
    fn status_code_map_keys_are_sorted() {
        let mut hist = StatusCodeHistogram::new();
        hist.record(Some(503));
        hist.record(Some(200));
        hist.record(None);
        hist.record(Some(404));
        let map = status_code_map(&hist);
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
        assert_eq!(report.responses_parsed, 7);
    }

    #[test]
    fn response_stats_report_empty_float_field_omitted() {
        use crate::histogram::{NumericHistogram, NumericHistogramParams};
        let mut rs = ResponseStats::new();
        // Manually insert an empty float field
        rs.float_fields.insert(
            "empty".to_string(),
            NumericHistogram::new(NumericHistogramParams { max_samples: 10 }),
        );
        let report = response_stats_report(&rs);
        assert!(!report.float_fields.contains_key("empty"));
    }
}
