use std::future::Future;

use uuid::Uuid;

use crate::publish::envelope::PublishEnvelope;
use crate::publish::error::PublishError;

// ── PublishOutcome ────────────────────────────────────────────────────────────

/// Information returned on a successful publish. Designed to be lightweight so
/// the caller can log it or surface a link back to the platform.
#[derive(Debug, Clone)]
pub struct PublishOutcome {
    /// The run id that was published. Echoed from the envelope for caller
    /// convenience.
    pub run_id: Uuid,
    /// Optional platform URL the user can open to view the run. `None` when
    /// the sink does not surface a viewer URL.
    pub view_url: Option<String>,
    /// Number of attempts actually made (1 = succeeded on first try).
    pub attempts: u32,
}

// ── ResultSink ────────────────────────────────────────────────────────────────

/// A transport-agnostic destination for a completed run report.
///
/// The default implementation is [`HttpSink`]; additional sinks (webhooks,
/// S3, append-only log) can be added without changing the engine or CLI
/// wiring. The trait is intentionally minimal — richer lifecycle hooks
/// (progress, chunked upload) belong on future traits, not here.
///
/// Uses stable edition-2024 async-fn-in-trait. Methods return an anonymous
/// `impl Future + Send`, which is compatible with `tokio::spawn` and the
/// `#[tokio::main]` runtime without requiring the `async_trait` crate.
///
/// [`HttpSink`]: crate::publish::http_sink::HttpSink
pub trait ResultSink: Send + Sync {
    /// Publishes the envelope. Implementations are responsible for their
    /// own retry strategy; the outer caller invokes `publish` exactly once.
    fn publish<'a>(
        &'a self,
        envelope: &'a PublishEnvelope<'a>,
    ) -> impl Future<Output = Result<PublishOutcome, PublishError>> + Send + 'a;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_is_cloneable() {
        let o = PublishOutcome {
            run_id: Uuid::now_v7(),
            view_url: Some("https://app.example.com/runs/abc".into()),
            attempts: 1,
        };
        let _ = o.clone();
    }

    // A no-op sink used to exercise the trait surface at compile-time. If the
    // trait signature changes in a breaking way, this test catches it.
    struct NoopSink;

    impl ResultSink for NoopSink {
        #[allow(clippy::manual_async_fn)]
        fn publish<'a>(
            &'a self,
            envelope: &'a PublishEnvelope<'a>,
        ) -> impl Future<Output = Result<PublishOutcome, PublishError>> + Send + 'a {
            async move {
                Ok(PublishOutcome {
                    run_id: envelope.run_id,
                    view_url: None,
                    attempts: 1,
                })
            }
        }
    }

    #[tokio::test]
    async fn noop_sink_returns_ok() {
        use std::collections::BTreeMap;

        use crate::output::{LatencyStats, RequestSummary, RunMeta, RunReport};

        let report = RunReport {
            version: 2,
            run: RunMeta {
                mode: "fixed".to_string(),
                elapsed_ms: 1.0,
                curve_duration_ms: None,
                template_generation_ms: None,
            },
            requests: RequestSummary {
                total: 1,
                ok: 1,
                failed: 0,
                skipped: 0,
                error_rate: 0.0,
                throughput_rps: 1.0,
            },
            latency: LatencyStats {
                min_ms: 1.0,
                p10_ms: 1.0,
                p25_ms: 1.0,
                p50_ms: 1.0,
                p75_ms: 1.0,
                p90_ms: 1.0,
                p95_ms: 1.0,
                p99_ms: 1.0,
                max_ms: 1.0,
                avg_ms: 1.0,
            },
            status_codes: BTreeMap::new(),
            response_stats: None,
            curve_stages: None,
            scenarios: None,
            thresholds: None,
        };
        let env = PublishEnvelope::new("0.0.0", &report);
        let sink = NoopSink;
        let outcome = sink.publish(&env).await.unwrap();
        assert_eq!(outcome.attempts, 1);
        assert_eq!(outcome.run_id, env.run_id);
    }
}
