use lmn_core::execution::{RunMode, RunStats, ScenarioStats};
use lmn_core::histogram::LatencyHistogram;
use lmn_core::response_template::stats::ResponseStats;
use lmn_core::threshold::ThresholdReport;
use std::collections::BTreeMap;
use std::time::Duration;

// ── PrintStatsParams ──��───────────────────────────────────────────────────────

/// Parameters for `print_stats`.
///
/// Wraps the previously ad-hoc positional arguments into a struct so that the
/// function signature can grow (e.g., adding threshold report, config path)
/// without breaking all call sites.
pub struct PrintStatsParams<'a> {
    pub stats: &'a RunStats,
    /// Optional threshold evaluation report produced by `lumen_core::config::evaluate`.
    /// When `Some`, a "Thresholds" section is appended to the table output and
    /// the caller should use `threshold_report.passed` to determine the exit code.
    pub threshold_report: Option<&'a ThresholdReport>,
}

// ── print_stats ───────────────────────────────────────────────────────────────

pub fn print_stats(params: PrintStatsParams<'_>) {
    let PrintStatsParams {
        stats,
        threshold_report,
    } = params;

    let total = stats.total_requests as usize;
    let ok = total.saturating_sub(stats.total_failures as usize);
    let fail = stats.total_failures as usize;
    let throughput = if stats.elapsed.as_secs_f64() > 0.0 {
        total as f64 / stats.elapsed.as_secs_f64()
    } else {
        0.0
    };

    let min = Duration::from_secs_f64(stats.latency.min_ms() / 1000.0);
    let max = Duration::from_secs_f64(stats.latency.max_ms() / 1000.0);
    let avg = Duration::from_secs_f64(stats.latency.mean_ms() / 1000.0);

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
            let ms = stats.latency.quantile_ms(p);
            rows.push((label, fmt_latency(Duration::from_secs_f64(ms / 1000.0))));
        }
        rows.push(("max", fmt_latency(max)));
        rows.push(("avg", fmt_latency(avg)));
        rows
    };

    let val_width = lat_rows.iter().map(|(_, v)| v.len()).max().unwrap_or(0);

    let code_counts: Vec<(String, u64)> = {
        let mut map = BTreeMap::new();
        for (code, count) in stats.status_codes.counts() {
            map.insert(code.to_string(), *count);
        }
        if stats.status_codes.error_count() > 0 {
            map.insert("err".to_string(), stats.status_codes.error_count());
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
    if let Some(ref cs) = stats.curve_stats {
        println!("  curve      {}", fmt_total_duration(cs.duration));
    }
    if let Some(ref ts) = stats.template_stats {
        println!(
            "  template   {}",
            fmt_total_duration(ts.generation_duration)
        );
    }
    println!("  throughput {throughput:.1} req/s");
    println!();
    println!(" Latency {rule}");
    for (label, val) in &lat_rows {
        println!("  {label:<4}  {val:>val_width$}");
    }
    println!();
    println!(" Histogram {rule}");
    print_latency_histogram(&stats.latency, bar_width);
    println!();
    println!(" Status codes {rule}");
    for (code, count) in &code_counts {
        let bar = frac_bar(*count as usize, bar_max as usize, bar_width);
        println!("  {code:<5}  {count:>count_width$}  {bar}");
    }
    println!();

    if let Some(ref scenarios) = stats.scenario_stats {
        print_scenario_stats(scenarios, stats.elapsed, &rule);
    }

    if let Some(ref rs) = stats.response_stats {
        print_response_stats(rs, &rule);
    }

    if let Some(tr) = threshold_report {
        print_threshold_report(tr, &rule);
    }
}

fn print_latency_histogram(hist: &LatencyHistogram, bar_width: usize) {
    const BUCKETS: usize = 10;
    if hist.is_empty() {
        return;
    }
    let min_us = (hist.min_ms() * 1000.0) as u64;
    let max_us = (hist.max_ms() * 1000.0) as u64;
    if min_us >= max_us {
        return;
    }
    let step = (max_us - min_us) as f64 / BUCKETS as f64;

    let mut counts = [0u64; BUCKETS];
    for (val_us, count) in hist.iter_recorded_us() {
        if val_us >= min_us && val_us <= max_us {
            let idx = ((val_us - min_us) as f64 / step) as usize;
            counts[idx.min(BUCKETS - 1)] += count;
        }
    }

    let bucket_max = *counts.iter().max().unwrap_or(&1);
    let label_width = fmt_latency(Duration::from_micros(max_us))
        .len()
        .max(fmt_latency(Duration::from_micros(min_us)).len())
        + 1;

    for (i, &count) in counts.iter().enumerate() {
        let bucket_start = Duration::from_micros(min_us + (step * i as f64) as u64);
        let label = fmt_latency(bucket_start);
        let bar = frac_bar(count as usize, bucket_max as usize, bar_width);
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
    let sorted_strings: BTreeMap<_, _> = rs.string_fields.iter().collect();
    for (path, hist) in &sorted_strings {
        println!(" Response: {path} {rule}");
        let mut entries: Vec<_> = hist.entries().iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        let count_width = entries
            .iter()
            .map(|(_, n)| n.to_string().len())
            .max()
            .unwrap_or(1);
        let bar_max = entries.iter().map(|(_, n)| **n).max().unwrap_or(1);
        let bar_width = 28usize;
        for (val, count) in &entries {
            let bar = frac_bar(**count as usize, bar_max as usize, bar_width);
            println!("  {val:<20}  {count:>count_width$}  {bar}");
        }
        println!();
    }

    let sorted_floats: BTreeMap<_, _> = rs.float_fields.iter().collect();
    for (path, hist) in &sorted_floats {
        if hist.is_empty() {
            continue;
        }
        let dist = hist.distribution();
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

fn print_scenario_stats(scenarios: &[ScenarioStats], elapsed: Duration, rule: &str) {
    let mut scenarios: Vec<&ScenarioStats> = scenarios.iter().collect();
    scenarios.sort_by(|a, b| a.name.cmp(&b.name));

    for scenario in scenarios {
        let total = scenario.requests.total_requests as usize;
        let failed = scenario.requests.total_failures as usize;
        let ok = total.saturating_sub(failed);
        let throughput = if elapsed.as_secs_f64() > 0.0 {
            total as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        let p50 = Duration::from_secs_f64(scenario.requests.latency.quantile_ms(0.50) / 1000.0);
        let p95 = Duration::from_secs_f64(scenario.requests.latency.quantile_ms(0.95) / 1000.0);
        let p99 = Duration::from_secs_f64(scenario.requests.latency.quantile_ms(0.99) / 1000.0);

        println!(" Scenario: {} {rule}", scenario.name);
        println!("  requests   {total}  ({ok} ok · {failed} failed)");
        println!("  throughput {throughput:.1} req/s");
        println!(
            "  latency    p50 {} · p95 {} · p99 {}",
            fmt_latency(p50),
            fmt_latency(p95),
            fmt_latency(p99)
        );

        if !scenario.steps.is_empty() {
            println!("  steps");
            let mut steps: Vec<_> = scenario.steps.iter().collect();
            steps.sort_by(|a, b| a.name.cmp(&b.name));
            for step in steps {
                let step_total = step.requests.total_requests as usize;
                let step_failed = step.requests.total_failures as usize;
                let step_error_rate = if step_total == 0 {
                    0.0
                } else {
                    step_failed as f64 / step_total as f64
                };
                let step_p95 =
                    Duration::from_secs_f64(step.requests.latency.quantile_ms(0.95) / 1000.0);
                println!(
                    "    {:<16} {:>6} req  {:>5.1}% err  p95 {}",
                    step.name,
                    step_total,
                    step_error_rate * 100.0,
                    fmt_latency(step_p95),
                );
            }
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
