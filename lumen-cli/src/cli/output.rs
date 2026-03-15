use lumen_core::command::run::RunStats;
use lumen_core::http::RequestResult;

use lumen_core::response_template::stats::ResponseStats;
use std::collections::BTreeMap;
use std::time::Duration;

pub fn print_stats(results: &[RequestResult], stats: &RunStats) {
    let total = results.len();
    let ok = results.iter().filter(|r| r.success).count();
    let fail = total - ok;
    let throughput = if stats.elapsed.as_secs_f64() > 0.0 {
        total as f64 / stats.elapsed.as_secs_f64()
    } else {
        0.0
    };

    let mut durations: Vec<Duration> = results.iter().map(|r| r.duration).collect();
    durations.sort();

    let min = durations.first().copied().unwrap_or(Duration::ZERO);
    let max = durations.last().copied().unwrap_or(Duration::ZERO);

    let lat_rows: Vec<(&str, String)> = {
        let mut rows = vec![("min", fmt_latency(min))];
        for (p, label) in [(10, "p10"), (25, "p25"), (50, "p50"), (75, "p75"), (90, "p90"), (95, "p95"), (99, "p99")] {
            rows.push((label, fmt_latency(percentile(&durations, p))));
        }
        rows.push(("max", fmt_latency(max)));
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
    let count_width = code_counts.iter().map(|(_, n)| n.to_string().len()).max().unwrap_or(1);

    let rule = "─".repeat(34);

    println!();
    println!(" Results {rule}");
    println!("  requests   {total}  ({ok} ok · {fail} failed)");
    println!("  duration   {}", fmt_total_duration(stats.elapsed));
    if let Some(td) = stats.template_duration {
        println!("  template   {}", fmt_total_duration(td));
    }
    println!("  throughput {throughput:.1} req/s");
    println!();
    println!(" Latency {rule}");
    for (label, val) in &lat_rows {
        println!("  {label:<4}  {val:>val_width$}");
    }
    println!();
    println!(" Status codes {rule}");
    for (code, count) in &code_counts {
        let filled = (count * bar_width) / bar_max;
        let bar = "█".repeat(filled);
        println!("  {code:<5}  {count:>count_width$}  {bar}");
    }
    println!();

    if let Some(ref rs) = stats.response_stats {
        print_response_stats(rs, &rule);
    }
}

fn print_response_stats(rs: &ResponseStats, rule: &str) {
    let sorted_strings: BTreeMap<_, _> = rs.string_distributions.iter().collect();
    for (path, dist) in &sorted_strings {
        println!(" Response: {path} {rule}");
        let mut entries: Vec<_> = dist.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        let count_width = entries.iter().map(|(_, n)| n.to_string().len()).max().unwrap_or(1);
        let bar_max = entries.iter().map(|(_, n)| **n).max().unwrap_or(1);
        let bar_width = 28usize;
        for (val, count) in &entries {
            let filled = (*count * bar_width) / bar_max;
            let bar = "█".repeat(filled);
            println!("  {val:<20}  {count:>count_width$}  {bar}");
        }
        println!();
    }

    let sorted_floats: BTreeMap<_, _> = rs.float_fields.iter().collect();
    for (path, field) in &sorted_floats {
        if field.values.is_empty() {
            continue;
        }
        let mut sorted = field.values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let min = sorted.first().unwrap();
        let max = sorted.last().unwrap();
        let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
        let p50 = sorted[sorted.len() * 50 / 100];
        let p95 = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
        let p99 = sorted[(sorted.len() * 99 / 100).min(sorted.len() - 1)];

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

fn percentile(sorted: &[Duration], p: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = (sorted.len() * p / 100).min(sorted.len() - 1);
    sorted[idx]
}