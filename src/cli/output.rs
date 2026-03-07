use crate::command::run::RequestResult;
use std::time::Duration;

pub fn print_stats(results: &[RequestResult], elapsed: Duration) {
    let total = results.len();
    let ok = results.iter().filter(|r| r.success).count();
    let fail = total - ok;
    let throughput = if elapsed.as_secs_f64() > 0.0 {
        total as f64 / elapsed.as_secs_f64()
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
    let rule = "─".repeat(34);

    println!();
    println!(" Results {rule}");
    println!("  requests   {total}  ({ok} ok · {fail} failed)");
    println!("  duration   {}", fmt_total_duration(elapsed));
    println!("  throughput {throughput:.1} req/s");
    println!();
    println!(" Latency {rule}");
    for (label, val) in &lat_rows {
        println!("  {label:<4}  {val:>val_width$}");
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

fn percentile(sorted: &[Duration], p: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = (sorted.len() * p / 100).min(sorted.len() - 1);
    sorted[idx]
}