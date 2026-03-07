use crate::cli::output::print_stats;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

pub mod cli;

pub struct RequestResult {
    pub duration: Duration,
    pub success: bool,
}

struct WorkerConfig {
    host: Arc<String>,
    count: usize,
    concurrency: usize,
    shutdown: Arc<AtomicBool>,
}

pub fn process_run_command(run_args: cli::command::RunArgs) {
    let host = Arc::new(run_args.host);
    let threads = run_args.threads as usize;
    let total = run_args.request_count as usize;
    let concurrency = run_args.concurrency as usize;

    let per_thread = total / threads;
    let remainder = total % threads;

    let shutdown = Arc::new(AtomicBool::new(false));

    let shutdown_signal = Arc::clone(&shutdown);
    thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .expect("failed to create signal runtime")
            .block_on(async {
                tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
                eprintln!("\nShutdown signal received — waiting for in-flight requests to finish...");
                shutdown_signal.store(true, Ordering::Relaxed);
            });
    });

    let mut handles = Vec::new();
    let started_at = Instant::now();

    for i in 0..threads {
        let config = WorkerConfig {
            host: Arc::clone(&host),
            count: per_thread + if i < remainder { 1 } else { 0 },
            concurrency: (concurrency / threads).max(1),
            shutdown: Arc::clone(&shutdown),
        };

        let handle = thread::Builder::new()
            .name(format!("worker-{}", i))
            .spawn(move || {
                tokio::runtime::Runtime::new()
                    .expect("failed to create tokio runtime")
                    .block_on(run_concurrent_requests(config))
            })
            .expect("failed to spawn thread");

        handles.push(handle);
    }

    let mut all_results: Vec<RequestResult> = Vec::new();
    for handle in handles {
        let results = handle.join().expect("worker thread panicked");
        all_results.extend(results);
    }

    print_stats(&all_results, started_at.elapsed());
}

async fn run_concurrent_requests(config: WorkerConfig) -> Vec<RequestResult> {
    let WorkerConfig { host, count, concurrency, shutdown } = config;
    let client = reqwest::Client::new();
    let sem = Arc::new(Semaphore::new(concurrency));
    let mut tasks = Vec::with_capacity(count);

    for _ in 0..count {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let client = client.clone();
        let url = host.as_str().to_string();
        let permit = sem.clone().acquire_owned().await.unwrap();
        tasks.push(tokio::spawn(async move {
            let _permit = permit;
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