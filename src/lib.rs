use crate::cli::command::RunArgs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub mod cli;

struct RequestResult {
    duration: Duration,
    success: bool,
}

pub fn process_run_command(run_args: RunArgs) {
    let host = Arc::new(run_args.host);
    let threads = run_args.threads as usize;
    let total = run_args.request_count as usize;

    let per_thread = total / threads;
    let remainder = total % threads;

    let mut handles = Vec::new();

    for i in 0..threads {
        let host = Arc::clone(&host);
        let count = per_thread + if i < remainder { 1 } else { 0 };

        let handle = thread::Builder::new()
            .name(format!("worker-{}", i))
            .spawn(move || {
                tokio::runtime::Runtime::new()
                    .expect("failed to create tokio runtime")
                    .block_on(run_concurrent_requests(&host, count))
            })
            .expect("failed to spawn thread");

        handles.push(handle);
    }

    let mut all_results: Vec<RequestResult> = Vec::new();
    for handle in handles {
        let results = handle.join().expect("worker thread panicked");
        all_results.extend(results);
    }

    print_stats(&all_results);
}

async fn run_concurrent_requests(host: &str, count: usize) -> Vec<RequestResult> {
    let client = reqwest::Client::new();
    let mut tasks = Vec::with_capacity(count);

    for _ in 0..count {
        let client = client.clone();
        let url = host.to_string();
        tasks.push(tokio::spawn(async move {
            let start = Instant::now();
            match client.get(&url).send().await {
                Ok(resp) => RequestResult {
                    duration: start.elapsed(),
                    success: resp.status().is_success(),
                },
                Err(_) => RequestResult {
                    duration: start.elapsed(),
                    success: false,
                },
            }
        }));
    }

    let mut results = Vec::with_capacity(count);
    for task in tasks {
        if let Ok(result) = task.await {
            results.push(result);
        }
    }
    results
}

fn print_stats(results: &[RequestResult]) {
    let total = results.len();
    let successes = results.iter().filter(|r| r.success).count();

    let mut durations: Vec<Duration> = results.iter().map(|r| r.duration).collect();
    durations.sort();

    let sum: Duration = durations.iter().sum();
    let avg = if total > 0 { sum / total as u32 } else { Duration::ZERO };
    let min = durations.first().copied().unwrap_or(Duration::ZERO);
    let max = durations.last().copied().unwrap_or(Duration::ZERO);
    let p50 = percentile(&durations, 50);
    let p95 = percentile(&durations, 95);
    let p99 = percentile(&durations, 99);

    println!("\n=== Load Test Results ===");
    println!("Total:     {}", total);
    println!("Successes: {}", successes);
    println!("Failures:  {}", total - successes);
    println!("\nLatency:");
    println!("  min: {:?}", min);
    println!("  avg: {:?}", avg);
    println!("  p50: {:?}", p50);
    println!("  p95: {:?}", p95);
    println!("  p99: {:?}", p99);
    println!("  max: {:?}", max);
}

fn percentile(sorted: &[Duration], p: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = (sorted.len() * p / 100).min(sorted.len() - 1);
    sorted[idx]
}
