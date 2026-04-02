use lmn_core::execution::{RunMode, RunStats};
use lmn_core::http::RequestResult;
use lmn_core::response_template::stats::ResponseStats;
use lmn_core::stats::{Distribution, LatencyDistribution};
use lmn_core::threshold::ThresholdReport;
use std::collections::BTreeMap;
use std::time::Duration;

// ── PrintStatsParams ──────────────────────────────────────────────────────────

/// Parameters for `print_stats`.
///
/// Wraps the previously ad-hoc positional arguments into a struct so that the
/// function signature can grow (e.g., adding threshold report, config path)
/// without breaking all call sites.
pub struct PrintStatsParams<'a> {
    pub results: &'a [RequestResult],
    pub stats: &'a RunStats,
    /// Optional threshold evaluation report produced by `lumen_core::config::evaluate`.
    /// When `Some`, a "Thresholds" section is appended to the table output and
    /// the caller should use `threshold_report.passed` to determine the exit code.
    pub threshold_report: Option<&'a ThresholdReport>,
}

// ── print_stats ───────────────────────────────────────────────────────────────

pub fn print_stats(params: PrintStatsParams<'_>) {
    let PrintStatsParams {
        results,
        stats,
        threshold_report,
    } = params;

    let total = stats.total_requests;
    let ok = total.saturating_sub(stats.total_failures);
    let fail = stats.total_failures;
    let throughput = if stats.elapsed.as_secs_f64() > 0.0 {
        total as f64 / stats.elapsed.as_secs_f64()
    } else {
        0.0
    };

    let lat_dist = LatencyDistribution::from_results(results);

    let min = Duration::from_secs_f64(lat_dist.min_ms() / 1000.0);
    let max = Duration::from_secs_f64(lat_dist.max_ms() / 1000.0);
    let avg = Duration::from_secs_f64(lat_dist.mean_ms() / 1000.0);

    // Keep a sorted Duration slice for the histogram below.
    let mut durations: Vec<Duration> = results.iter().map(|r| r.duration).collect();
    durations.sort();

    let lat_rows: Vec<(&str, String)> = {
        let mut rows = vec![("min", fmt_latency(min))];
        for (p, label) in [
            (0.10, "p10"),
            (0.25, "p25"),
            (0.50, "p50"),
            (0.75, "p75"),
            (0.90, "p90"),
            (0.95, "p95"),
            (0.99, "p99"),
        ] {
            let ms = lat_dist.quantile_ms(p);
            rows.push((label, fmt_latency(Duration::from_secs_f64(ms / 1000.0))));
        }
        rows.push(("max", fmt_latency(max)));
        rows.push(("avg", fmt_latency(avg)));
        rows
    };

    let val_width = lat_rows.iter().map(|(_, v)| v.len()).max().unwrap_or(0);

    let code_counts: Vec<(String, usize)> = {
        let mut map = std::collections::BTreeMap::new();
        for r in results {
            let key = match r.status_code {
                Some(c) => c.to_string(),
                None => "err".to_string(),
            };
            *map.entry(key).or_insert(0usize) += 1;
        }
        map.into_iter().collect()
    };
    let bar_max = code_counts.iter().map(|(_, n)| *n).max().unwrap_or(1);
    let bar_width = 28usize;
    let count_width = code_counts
        .iter()
        .map(|(_, n)| n.to_string().len())
        .max()
        .unwrap_or(1);

    let rule = "─".repeat(34);

    println!();
    println!(" Results {rule}");
    match stats.mode {
        RunMode::Curve => println!("  mode       curve"),
        RunMode::Fixed => println!("  mode       fixed"),
    }
    println!("  requests   {total}  ({ok} ok · {fail} failed)");
    println!("  duration   {}", fmt_total_duration(stats.elapsed));
    if let Some(cd) = stats.curve_duration {
        println!("  curve      {}", fmt_total_duration(cd));
    }
    if let Some(td) = stats.template_duration {
        println!("  template   {}", fmt_total_duration(td));
    }
    println!("  throughput {throughput:.1} req/s");
    if stats.min_sample_rate < 1.0 {
        let inverse = (1.0_f64 / stats.min_sample_rate).round() as usize;
        println!("  sampling  ~1-in-{inverse} (latency percentiles are approximate)");
    }
    println!();
    println!(" Latency {rule}");
    for (label, val) in &lat_rows {
        println!("  {label:<4}  {val:>val_width$}");
    }
    println!();
    println!(" Histogram {rule}");
    print_latency_histogram(&durations, min, max, bar_width);
    println!();
    println!(" Status codes {rule}");
    for (code, count) in &code_counts {
        let bar = frac_bar(*count, bar_max, bar_width);
        println!("  {code:<5}  {count:>count_width$}  {bar}");
    }
    println!();

    if let Some(ref rs) = stats.response_stats {
        print_response_stats(rs, &rule);
    }

    if let Some(tr) = threshold_report {
        print_threshold_report(tr, &rule);
    }
}

fn print_latency_histogram(durations: &[Duration], min: Duration, max: Duration, bar_width: usize) {
    const BUCKETS: usize = 10;
    if durations.is_empty() || min == max {
        return;
    }
    let min_us = min.as_micros() as f64;
    let max_us = max.as_micros() as f64;
    let step = (max_us - min_us) / BUCKETS as f64;

    let mut counts = [0usize; BUCKETS];
    for d in durations {
        let us = d.as_micros() as f64;
        let idx = ((us - min_us) / step) as usize;
        counts[idx.min(BUCKETS - 1)] += 1;
    }

    let bucket_max = *counts.iter().max().unwrap_or(&1);
    let label_width = fmt_latency(max).len().max(fmt_latency(min).len()) + 1;

    for (i, &count) in counts.iter().enumerate() {
        let bucket_start = Duration::from_micros((min_us + step * i as f64) as u64);
        let label = fmt_latency(bucket_start);
        let bar = frac_bar(count, bucket_max, bar_width);
        println!("  {label:>label_width$}  {bar}  {count}");
    }
}

fn frac_bar(value: usize, max: usize, width: usize) -> String {
    const BLOCKS: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
    if max == 0 {
        return " ".repeat(width);
    }
    let units = value * width * 8 / max;
    let full = units / 8;
    let rem = units % 8;
    let mut s = String::with_capacity(width);
    for _ in 0..full {
        s.push('█');
    }
    if full < width {
        s.push(BLOCKS[rem]);
        for _ in (full + 1)..width {
            s.push(' ');
        }
    }
    s
}

fn print_response_stats(rs: &ResponseStats, rule: &str) {
    let sorted_strings: BTreeMap<_, _> = rs.string_distributions.iter().collect();
    for (path, dist) in &sorted_strings {
        println!(" Response: {path} {rule}");
        let mut entries: Vec<_> = dist.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        let count_width = entries
            .iter()
            .map(|(_, n)| n.to_string().len())
            .max()
            .unwrap_or(1);
        let bar_max = entries.iter().map(|(_, n)| **n).max().unwrap_or(1);
        let bar_width = 28usize;
        for (val, count) in &entries {
            let bar = frac_bar(**count, bar_max, bar_width);
            println!("  {val:<20}  {count:>count_width$}  {bar}");
        }
        println!();
    }

    let sorted_floats: BTreeMap<_, _> = rs.float_fields.iter().collect();
    for (path, field) in &sorted_floats {
        if field.values.is_empty() {
            continue;
        }
        let dist = Distribution::from_unsorted(field.values.clone());
        let min = dist.min();
        let max = dist.max();
        let avg = dist.mean();
        let p50 = dist.quantile(0.50);
        let p95 = dist.quantile(0.95);
        let p99 = dist.quantile(0.99);

        println!(" Response: {path} {rule}");
        println!("  min   {min:.4}");
        println!("  avg   {avg:.4}");
        println!("  p50   {p50:.4}");
        println!("  p95   {p95:.4}");
        println!("  p99   {p99:.4}");
        println!("  max   {max:.4}");
        println!();
    }

    if !rs.mismatch_counts.is_empty() {
        let sorted_mismatches: BTreeMap<_, _> = rs.mismatch_counts.iter().collect();
        println!(" Response mismatches {rule}");
        for (path, count) in &sorted_mismatches {
            println!("  {path:<30}  {count}");
        }
        println!();
    }
}

// ── print_threshold_report ────────────────────────────────────────────────────

/// Renders the threshold evaluation summary produced by `lumen_core::config::evaluate`.
///
/// Each rule is printed on its own line with a PASS/FAIL indicator. The overall
/// verdict ("all thresholds passed" / "X threshold(s) failed") is printed last.
fn print_threshold_report(tr: &ThresholdReport, rule: &str) {
    println!(" Thresholds {rule}");
    for result in &tr.results {
        let indicator = if result.passed { "pass" } else { "FAIL" };
        let symbol = result.threshold.operator.symbol();
        println!(
            "  [{indicator}]  {:?} {} {:.4}  (actual: {:.4})",
            result.threshold.metric, symbol, result.threshold.value, result.actual
        );
    }
    if tr.all_passed() {
        println!("  PASSED — all thresholds satisfied");
    } else {
        println!("  FAILED — {} threshold(s) not satisfied", tr.failed);
    }
    println!();
}

fn fmt_latency(d: Duration) -> String {
    let micros = d.as_micros();
    if micros < 1_000 {
        format!("{}µs", micros)
    } else if micros < 1_000_000 {
        format!("{:.1}ms", micros as f64 / 1_000.0)
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

fn fmt_total_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 60.0 {
        format!("{:.2}s", secs)
    } else {
        let m = d.as_secs() / 60;
        let s = secs % 60.0;
        format!("{}m {:.1}s", m, s)
    }
}

#[cfg(test)]
mod output_tests {
    use lmn_core::stats::Distribution;

    #[test]
    fn distribution_quantile_p50_of_100() {
        // 100-element uniform distribution: index = floor(100 * 0.50) = 50 → value 51.0
        let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let dist = Distribution::from_sorted(values);
        assert_eq!(dist.quantile(0.50), 51.0);
    }

    #[test]
    fn distribution_quantile_p99_of_100() {
        // index = floor(100 * 0.99) = 99 → value 100.0
        let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let dist = Distribution::from_sorted(values);
        assert_eq!(dist.quantile(0.99), 100.0);
    }

    #[test]
    fn distribution_quantile_p0() {
        // index = floor(100 * 0.0) = 0 → value 1.0
        let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let dist = Distribution::from_sorted(values);
        assert_eq!(dist.quantile(0.0), 1.0);
    }

    #[test]
    fn distribution_quantile_p100_clamps_to_last() {
        // index = floor(100 * 1.0) = 100, clamped to 99 → value 100.0
        let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let dist = Distribution::from_sorted(values);
        assert_eq!(dist.quantile(1.0), 100.0);
    }

    #[test]
    fn distribution_quantile_empty_returns_zero() {
        let dist = Distribution::from_sorted(vec![]);
        assert_eq!(dist.quantile(0.50), 0.0);
    }

    #[test]
    fn distribution_quantile_single_element() {
        let dist = Distribution::from_sorted(vec![42.0]);
        assert_eq!(dist.quantile(0.99), 42.0);
        assert_eq!(dist.quantile(0.0), 42.0);
    }
}
