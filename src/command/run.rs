use crate::cli::command::{HttpMethod, RunArgs};
use crate::cli::output::print_stats;
use crate::command::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

pub struct RequestResult {
    pub duration: Duration,
    pub success: bool,
    pub status_code: Option<u16>,
}

pub enum BodyFormat {
    Json,
}

pub enum RequestBody {
    Formatted { content: String, format: BodyFormat },
}

pub struct RunCommand {
    pub host: String,
    pub threads: usize,
    pub request_count: usize,
    pub concurrency: usize,
    pub method: HttpMethod,
    pub body: Option<RequestBody>,
}

impl From<RunArgs> for RunCommand {
    fn from(args: RunArgs) -> Self {
        RunCommand {
            host: args.host,
            threads: args.threads as usize,
            request_count: args.request_count as usize,
            concurrency: args.concurrency as usize,
            method: args.method,
            body: args.body.map(|s| RequestBody::Formatted {
                content: s,
                format: BodyFormat::Json,
            }),
        }
    }
}

struct WorkerConfig {
    host: Arc<String>,
    count: usize,
    concurrency: usize,
    shutdown: Arc<AtomicBool>,
    method: HttpMethod,
    body: Arc<Option<RequestBody>>,
}

impl Command for RunCommand {
    fn execute(self) {
        let host = Arc::new(self.host);
        let threads = self.threads;
        let total = self.request_count;
        let concurrency = self.concurrency;
        let method = self.method;
        let body = Arc::new(self.body);

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
                method,
                body: Arc::clone(&body),
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
}

async fn run_concurrent_requests(config: WorkerConfig) -> Vec<RequestResult> {
    let WorkerConfig { host, count, concurrency, shutdown, method, body } = config;
    let client = reqwest::Client::new();
    let sem = Arc::new(Semaphore::new(concurrency));
    let mut tasks = Vec::with_capacity(count);

    for _ in 0..count {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let client = client.clone();
        let url = host.as_str().to_string();
        let body = Arc::clone(&body);
        let permit = sem.clone().acquire_owned().await.unwrap();
        tasks.push(tokio::spawn(async move {
            let _permit = permit;
            let start = Instant::now();
            let mut req = match method {
                HttpMethod::Get    => client.get(&url),
                HttpMethod::Post   => client.post(&url),
                HttpMethod::Put    => client.put(&url),
                HttpMethod::Patch  => client.patch(&url),
                HttpMethod::Delete => client.delete(&url),
            };
            if let Some(b) = body.as_ref() {
                match b {
                    RequestBody::Formatted { content, format } => {
                        let content_type = match format {
                            BodyFormat::Json => "application/json",
                        };
                        req = req.header("Content-Type", content_type).body(content.clone());
                    }
                }
            }
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    RequestResult {
                        duration: start.elapsed(),
                        success: status.is_success(),
                        status_code: Some(status.as_u16()),
                    }
                }
                Err(_) => RequestResult {
                    duration: start.elapsed(),
                    success: false,
                    status_code: None,
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
