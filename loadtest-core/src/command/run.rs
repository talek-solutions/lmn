use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Semaphore};
use tracing::Instrument;
use tracing::info_span;

use crate::command::method::HttpMethod;
use crate::command::{Command, Body};
use crate::monitoring::SpanName;
use crate::response_template::extractor;
use crate::response_template::field::TrackedField;
use crate::response_template::stats::ResponseStats;
use crate::response_template::ResponseTemplate;
use crate::request_template::Template;

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

struct ResolvedBody {
    content: String,
    content_type: &'static str,
}

impl Command for RunCommand {
    async fn execute(self) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
        let total = self.request_count;
        let concurrency = self.concurrency;

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
            tracked_fields,
        });

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_signal = Arc::clone(&shutdown);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
            eprintln!("\nShutdown signal received — waiting for in-flight requests to finish...");
            shutdown_signal.store(true, Ordering::Relaxed);
        });

        let started_at = Instant::now();

        let all_results = async {
            let sem = Arc::new(Semaphore::new(concurrency));
            let (tx, mut rx) = mpsc::channel::<RequestResult>(concurrency);

            for i in 0..total {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }

                let resolved: Option<ResolvedBody> = if let Some(ref bs) = all_bodies {
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
                let tx = tx.clone();

                tokio::spawn(async move {
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
                    let result = match req.send().await {
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
                    };
                    let _ = tx.send(result).await;
                });
            }

            // Close the last sender — rx drains once all tasks have finished
            drop(tx);

            let mut results = Vec::with_capacity(total);
            while let Some(result) = rx.recv().await {
                results.push(result);
            }
            results
        }
        .instrument(info_span!(SpanName::REQUESTS, total))
        .await;

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

        Ok(Some(RunStats {
            elapsed: started_at.elapsed(),
            template_duration,
            response_stats,
            results: all_results,
        }))
    }
}
