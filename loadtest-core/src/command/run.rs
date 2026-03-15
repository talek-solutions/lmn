use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Semaphore};
use tracing::Instrument;
use tracing::info_span;

use crate::command::{Command, Body};
use crate::http::{Request, RequestConfig, RequestResult};
use crate::monitoring::SpanName;
use crate::response_template::extractor;
use crate::response_template::stats::ResponseStats;
use crate::response_template::ResponseTemplate;
use crate::request_template::Template;

pub struct RunStats {
    pub elapsed: Duration,
    pub template_duration: Option<Duration>,
    pub response_stats: Option<ResponseStats>,
    pub results: Vec<RequestResult>,
}

pub struct RunCommand {
    pub host: String,
    pub request_count: usize,
    pub concurrency: usize,
    pub method: crate::command::HttpMethod,
    pub body: Option<Body>,
    pub template_path: Option<PathBuf>,
    pub response_template_path: Option<PathBuf>,
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

        let tracked_fields = self
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

        let started_at = Instant::now();

        let all_results = async {
            let sem = Arc::new(Semaphore::new(concurrency));
            let (tx, mut rx) = mpsc::channel::<RequestResult>(concurrency);
            let mut shutdown = false;

            for i in 0..total {
                if shutdown { break; }

                let resolved = request.resolve_body(
                    all_bodies.as_ref().map(|bs| bs[i].clone()),
                );

                let client = request.client.clone();
                let url = request.host.as_str().to_string();
                let method = request.method;
                let capture_body = request.tracked_fields.is_some();
                let tx = tx.clone();

                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        eprintln!("\nShutdown signal received — waiting for in-flight requests to finish...");
                        shutdown = true;
                    }
                    permit = sem.clone().acquire_owned() => {
                        let permit = permit.unwrap();
                        tokio::spawn(async move {
                            let _permit = permit;
                            let mut req = Request::new(client, url, method);
                            if let Some((content, content_type)) = resolved {
                                req = req.body(content, content_type);
                            }
                            if capture_body {
                                req = req.read_response();
                            }
                            let _ = tx.send(req.execute().await).await;
                        });
                    }
                }
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
