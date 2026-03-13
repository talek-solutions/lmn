use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;

use tracing::{info_span, Instrument};

use crate::cli::command::{HttpMethod, RunArgs};
use crate::cli::output::print_stats;
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

impl From<RunArgs> for RunCommand {
    fn from(args: RunArgs) -> Self {
        RunCommand {
            host: args.host,
            threads: args.threads as usize,
            request_count: args.request_count as usize,
            concurrency: args.concurrency as usize,
            method: args.method,
            body: args.body.map(|s| Body::Formatted {
                content: s,
                format: BodyFormat::Json,
            }),
            template_path: args.template.or_else(|| args.request_alias.map(resolve_alias("requests"))),
            response_template_path: args.response_template.or_else(|| args.response_alias.map(resolve_alias("responses"))),
        }
    }
}

fn resolve_alias(sub_dir: &'static str) -> impl Fn(String) -> PathBuf {
    move |alias| {
        let mut path = PathBuf::from(alias);
        if path.extension().is_none() {
            path.set_extension("json");
        }
        PathBuf::from(".templates").join(sub_dir).join(path)
    }
}

struct WorkerConfig {
    host: Arc<String>,
    count: usize,
    concurrency: usize,
    shutdown: Arc<AtomicBool>,
    method: HttpMethod,
    body: Arc<Option<Body>>,
    /// Pre-generated template bodies for this worker's slice of requests.
    /// When `Some`, takes priority over `body` for request construction.
    bodies: Option<Vec<String>>,
    tracked_fields: Option<Arc<Vec<TrackedField>>>,
}

struct ResolvedBody {
    content: String,
    content_type: &'static str,
}

impl Command for RunCommand {
    fn execute(self) -> Result<(), Box<dyn std::error::Error>> {
        let host = Arc::new(self.host);
        let threads = self.threads;
        let total = self.request_count;
        let concurrency = self.concurrency;
        let method = self.method;
        let body = Arc::new(self.body);

        let per_thread = total / threads;
        let remainder = total % threads;

        // Pre-generate all template bodies before any requests fire
        let gen_start = Instant::now();
        let all_bodies: Option<Vec<String>> = self
            .template_path
            .map(|path| {
                let template = info_span!(SpanName::TEMPLATE_PARSE, path = %path.display())
                    .in_scope(|| Template::parse(&path))?;
                let bodies = info_span!(SpanName::TEMPLATE_RENDER, n = total)
                    .in_scope(|| template.pre_generate(total));
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

        let mut handles = Vec::new();
        let started_at = Instant::now();

        for i in 0..threads {
            let worker_count = per_thread + if i < remainder { 1 } else { 0 };
            let start = i * per_thread + i.min(remainder);

            let worker_bodies = all_bodies
                .as_ref()
                .map(|bodies| bodies[start..start + worker_count].to_vec());

            let config = WorkerConfig {
                host: Arc::clone(&host),
                count: worker_count,
                concurrency: (concurrency / threads).max(1),
                shutdown: Arc::clone(&shutdown),
                method,
                body: Arc::clone(&body),
                bodies: worker_bodies,
                tracked_fields: tracked_fields.clone(),
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

        let response_stats = tracked_fields.as_ref().map(|fields| {
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
        };
        print_stats(&all_results, &stats);
        Ok(())
    }
}

async fn run_concurrent_requests(config: WorkerConfig) -> Vec<RequestResult> {
    let WorkerConfig { host, count, concurrency, shutdown, method, body, bodies, tracked_fields } =
        config;
    let client = reqwest::Client::new();
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
            (*body).as_ref().map(|b| match b {
                Body::Formatted { content, format } => ResolvedBody {
                    content: content.clone(),
                    content_type: match format {
                        BodyFormat::Json => "application/json",
                    },
                },
            })
        };

        let client = client.clone();
        let url = host.as_str().to_string();
        let permit = sem.clone().acquire_owned().await.unwrap();
        let capture_body = tracked_fields.is_some();
        let method_str = method.as_str();
        let span = info_span!(SpanName::REQUEST, url = %url, method = method_str);
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
        }.instrument(span)));
    }

    let mut results = Vec::with_capacity(count);
    for task in tasks {
        if let Ok(result) = task.await {
            results.push(result);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_alias_appends_json_extension() {
        let path = resolve_alias("requests")("my-alias".to_string());
        assert_eq!(path, PathBuf::from(".templates/requests/my-alias.json"));
    }

    #[test]
    fn resolve_alias_preserves_existing_extension() {
        let path = resolve_alias("requests")("my-alias.json".to_string());
        assert_eq!(path, PathBuf::from(".templates/requests/my-alias.json"));
    }

    #[test]
    fn resolve_alias_uses_correct_subdir() {
        let path = resolve_alias("responses")("template".to_string());
        assert_eq!(path, PathBuf::from(".templates/responses/template.json"));
    }
}
