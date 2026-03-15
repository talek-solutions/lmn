use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;

use tracing::info_span;

use crate::command::method::HttpMethod;
use crate::command::{Command, Body};
use crate::monitoring::SpanName;
use crate::response_template::extractor;
use crate::response_template::field::TrackedField;
use crate::response_template::stats::ResponseStats;
use crate::response_template::ResponseTemplate;
use crate::template::Template;

pub struct RunStats {
    pub elapsed: Duration,
    pub template_duration: Option<Duration>,
    pub response_stats: Option<ResponseStats>,
    pub results: Vec<RequestResult>,
}

pub struct RequestResult {
    pub duration: Duration,
    pub success: bool,
    pub status_code: Option<u16>,
    pub response_body: Option<String>,
}

pub enum BodyFormat {
    Json,
}

pub struct RunCommand {
    pub host: String,
    pub threads: usize,
    pub request_count: usize,
    pub concurrency: usize,
    pub method: HttpMethod,
    pub body: Option<Body>,
    pub template_path: Option<PathBuf>,
    pub response_template_path: Option<PathBuf>,
}


struct RequestConfig {
    client: reqwest::Client,
    host: Arc<String>,
    method: HttpMethod,
    body: Arc<Option<Body>>,
    tracked_fields: Option<Arc<Vec<TrackedField>>>,
}

struct WorkerConfig {
    request: Arc<RequestConfig>,
    count: usize,
    concurrency: usize,
    shutdown: Arc<AtomicBool>,
    /// Pre-generated template bodies for this worker's slice of requests.
    /// When `Some`, takes priority over `body` for request construction.
    bodies: Option<Vec<String>>,
}

struct ResolvedBody {
    content: String,
    content_type: &'static str,
}

impl Command for RunCommand {
    fn execute(self) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
        let threads = self.threads;
        let total = self.request_count;
        let concurrency = self.concurrency;

        let per_thread = total / threads;
        let remainder = total % threads;

        // Pre-generate all template bodies before any requests fire
        let gen_start = Instant::now();
        let all_bodies: Option<Vec<String>> = self
            .template_path
            .map(|path| {
                let template = Template::parse(&path)?;
                let bodies = template.pre_generate(total);
                Ok::<Vec<String>, Box<dyn std::error::Error>>(bodies)
            })
            .transpose()?;
        let template_duration = all_bodies.as_ref().map(|_| gen_start.elapsed());

        let tracked_fields: Option<Arc<Vec<TrackedField>>> = self
            .response_template_path
            .map(|path| {
                ResponseTemplate::parse(&path)
                    .map(|rt| Arc::new(rt.fields))
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
            })
            .transpose()?;

        let request = Arc::new(RequestConfig {
            client: reqwest::Client::new(),
            host: Arc::new(self.host),
            method: self.method,
            body: Arc::new(self.body),
            tracked_fields: tracked_fields.clone(),
        });

        let shutdown = Arc::new(AtomicBool::new(false));

        let shutdown_signal = Arc::clone(&shutdown);
        thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .expect("failed to create signal runtime")
                .block_on(async {
                    tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
                    eprintln!(
                        "\nShutdown signal received — waiting for in-flight requests to finish..."
                    );
                    shutdown_signal.store(true, Ordering::Relaxed);
                });
        });

        let started_at = Instant::now();

        let all_results: Vec<RequestResult> = info_span!(SpanName::REQUESTS, total, threads)
            .in_scope(|| {
                let mut handles = Vec::new();

                for i in 0..threads {
                    let worker_count = per_thread + if i < remainder { 1 } else { 0 };
                    let start = i * per_thread + i.min(remainder);

                    let worker_bodies = all_bodies
                        .as_ref()
                        .map(|bodies| bodies[start..start + worker_count].to_vec());

                    let config = WorkerConfig {
                        request: Arc::clone(&request),
                        count: worker_count,
                        concurrency: (concurrency / threads).max(1),
                        shutdown: Arc::clone(&shutdown),
                        bodies: worker_bodies,
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

                let mut results = Vec::new();
                for handle in handles {
                    results.extend(handle.join().expect("worker thread panicked"));
                }
                results
            });

        let response_stats = request.tracked_fields.as_ref().map(|fields| {
            let mut rs = ResponseStats::new();
            for result in &all_results {
                if let Some(ref body_str) = result.response_body {
                    if let Ok(body_val) = serde_json::from_str(body_str) {
                        rs.record(extractor::extract(&body_val, fields));
                    }
                }
            }
            rs
        });

        let stats = RunStats {
            elapsed: started_at.elapsed(),
            template_duration,
            response_stats,
            results: all_results,
        };
        Ok(Some(stats))
    }
}

async fn run_concurrent_requests(config: WorkerConfig) -> Vec<RequestResult> {
    let WorkerConfig { request, count, concurrency, shutdown, bodies } = config;
    let sem = Arc::new(Semaphore::new(concurrency));
    let mut tasks = Vec::with_capacity(count);

    for i in 0..count {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Template bodies take priority; fall back to static body
        let resolved: Option<ResolvedBody> = if let Some(ref bs) = bodies {
            Some(ResolvedBody { content: bs[i].clone(), content_type: "application/json" })
        } else {
            request.body.as_ref().as_ref().map(|b| match b {
                Body::Formatted { content, format } => ResolvedBody {
                    content: content.clone(),
                    content_type: match format {
                        BodyFormat::Json => "application/json",
                    },
                },
            })
        };

        let client = request.client.clone();
        let url = request.host.as_str().to_string();
        let method = request.method;
        let capture_body = request.tracked_fields.is_some();
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
            if let Some(rb) = resolved {
                req = req.header("Content-Type", rb.content_type).body(rb.content);
            }
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let response_body = if capture_body {
                        resp.text().await.ok()
                    } else {
                        None
                    };
                    RequestResult {
                        duration: start.elapsed(),
                        success: status.is_success(),
                        status_code: Some(status.as_u16()),
                        response_body,
                    }
                }
                Err(_) => RequestResult {
                    duration: start.elapsed(),
                    success: false,
                    status_code: None,
                    response_body: None,
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

