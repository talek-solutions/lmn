//! Run Publishing — pushes completed [`RunReport`]s to the Lumen platform.
//!
//! This module is the transport layer for feature 3a (Run Publishing). It
//! wraps a run report in a [`PublishEnvelope`] and POSTs the JSON to the
//! configured endpoint.
//!
//! # Architecture
//!
//! - [`config`] — [`PublishConfig`] and the [`PublishConfigBuilder`] that
//!   resolves CLI flag / env var / YAML / default precedence.
//! - [`envelope`] — [`PublishEnvelope`] wrapping the [`RunReport`] with
//!   transport metadata (run id, CLI version, published_at, source).
//! - [`sink`] — [`ResultSink`] trait allowing alternative sinks (webhooks,
//!   S3) to slot in without engine changes.
//! - [`http_sink`] — the default [`HttpSink`] implementation: reqwest +
//!   rustls, bounded retries, no redirect following.
//! - [`error`] — [`PublishError`] enum with user-facing messages.
//!
//! # Usage
//!
//! ```ignore
//! use lmn_core::publish::{
//!     HttpSink, PublishConfigBuilder, PublishEnvelope, ResultSink,
//! };
//!
//! let config = PublishConfigBuilder {
//!     env_api_key: std::env::var("LUMEN_API_KEY").ok(),
//!     ..Default::default()
//! }
//! .build()?;
//!
//! let sink = HttpSink::new(config, env!("CARGO_PKG_VERSION"))?;
//! let envelope = PublishEnvelope::new(env!("CARGO_PKG_VERSION"), &report);
//! let outcome = sink.publish(&envelope).await?;
//! println!("published run {} in {} attempts", outcome.run_id, outcome.attempts);
//! # Ok::<_, lmn_core::publish::PublishError>(())
//! ```
//!
//! [`RunReport`]: crate::output::report::RunReport

pub mod config;
pub mod envelope;
pub mod error;
pub mod http_sink;
pub mod sink;

pub use config::{
    DEFAULT_PUBLISH_TIMEOUT, DEFAULT_PUBLISH_URL, MAX_PAYLOAD_BYTES, PublishConfig,
    PublishConfigBuilder, PublishConfigYaml,
};
pub use envelope::{ENVELOPE_VERSION, PublishEnvelope, PublishSource};
pub use error::PublishError;
pub use http_sink::HttpSink;
pub use sink::{PublishOutcome, ResultSink};
