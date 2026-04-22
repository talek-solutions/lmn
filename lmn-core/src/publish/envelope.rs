use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::output::RunReport;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Envelope schema version. Bump when the envelope shape changes in a breaking
/// way. Independent of the inner [`RunReport`] schema version.
///
/// Envelope vs report versioning is intentional: transport metadata can evolve
/// without forcing a report schema bump, and vice versa.
pub const ENVELOPE_VERSION: u32 = 1;

// ── PublishSource ─────────────────────────────────────────────────────────────

/// Where the run was initiated from. Always `"cli"` in v1; reserved values for
/// future-proofing so the envelope can distinguish CLI runs from platform-
/// triggered runs (scheduled jobs, webhook replays) without an envelope bump.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PublishSource {
    Cli,
    #[allow(dead_code)] // reserved for v2 scheduled runs
    Scheduled,
    #[allow(dead_code)] // reserved for v2 webhook replays
    Webhook,
}

// ── PublishEnvelope ───────────────────────────────────────────────────────────

/// The full payload sent in the POST body.
///
/// The envelope wraps a [`RunReport`] unchanged, layering transport metadata
/// (run id, CLI version, publish timestamp, source) on top. This keeps the
/// report schema (owned by the Structured Result Export feature) orthogonal
/// to the publish transport layer.
///
/// # Field order note
/// Fields appear in a stable order for snapshot-testing purposes; the JSON
/// serialization uses that order so diffs remain human-readable.
#[derive(Serialize, Debug)]
pub struct PublishEnvelope<'a> {
    /// Envelope schema version. See [`ENVELOPE_VERSION`].
    pub envelope_version: u32,
    /// Client-minted run identifier. UUIDv7 so it is time-ordered, which
    /// aids manual log triage. Client minting preserves optionality for
    /// future self-hosted deployments where the CLI may not have an
    /// always-available central ID authority.
    pub run_id: Uuid,
    /// CLI version string (from `CARGO_PKG_VERSION`). Helps the platform
    /// correlate schema variants to specific CLI builds.
    pub cli_version: &'a str,
    /// Wall-clock timestamp when publish was initiated, RFC 3339 / ISO 8601.
    #[serde(serialize_with = "serialize_rfc3339")]
    pub published_at: OffsetDateTime,
    /// The origin of the run. Always `Cli` in v1.
    pub source: PublishSource,
    /// The unmodified [`RunReport`] produced by the engine.
    pub report: &'a RunReport,
}

impl<'a> PublishEnvelope<'a> {
    /// Constructs a new envelope with a freshly-minted UUIDv7 and
    /// `published_at` set to the current time.
    pub fn new(cli_version: &'a str, report: &'a RunReport) -> Self {
        Self {
            envelope_version: ENVELOPE_VERSION,
            run_id: Uuid::now_v7(),
            cli_version,
            published_at: OffsetDateTime::now_utc(),
            source: PublishSource::Cli,
            report,
        }
    }

    /// Test / fixture constructor with explicit run_id and timestamp. Not
    /// intended for production code paths.
    #[cfg(test)]
    pub fn with_fixed_id_and_time(
        cli_version: &'a str,
        report: &'a RunReport,
        run_id: Uuid,
        published_at: OffsetDateTime,
    ) -> Self {
        Self {
            envelope_version: ENVELOPE_VERSION,
            run_id,
            cli_version,
            published_at,
            source: PublishSource::Cli,
            report,
        }
    }
}

// ── Serialization helpers ─────────────────────────────────────────────────────

fn serialize_rfc3339<S: serde::Serializer>(ts: &OffsetDateTime, ser: S) -> Result<S::Ok, S::Error> {
    let formatted = ts
        .format(&Rfc3339)
        .map_err(|e| serde::ser::Error::custom(format!("rfc3339 format: {e}")))?;
    ser.serialize_str(&formatted)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::output::{LatencyStats, RequestSummary, RunMeta};

    fn sample_report() -> RunReport {
        RunReport {
            version: 2,
            run: RunMeta {
                mode: "fixed".to_string(),
                elapsed_ms: 1234.5,
                curve_duration_ms: None,
                template_generation_ms: None,
            },
            requests: RequestSummary {
                total: 100,
                ok: 99,
                failed: 1,
                skipped: 0,
                error_rate: 0.01,
                throughput_rps: 50.0,
            },
            latency: LatencyStats {
                min_ms: 1.0,
                p10_ms: 2.0,
                p25_ms: 3.0,
                p50_ms: 5.0,
                p75_ms: 8.0,
                p90_ms: 12.0,
                p95_ms: 20.0,
                p99_ms: 50.0,
                max_ms: 100.0,
                avg_ms: 6.5,
            },
            status_codes: {
                let mut m = BTreeMap::new();
                m.insert("200".to_string(), 99);
                m.insert("500".to_string(), 1);
                m
            },
            response_stats: None,
            curve_stages: None,
            scenarios: None,
            thresholds: None,
        }
    }

    #[test]
    fn envelope_new_mints_uuidv7() {
        let r = sample_report();
        let env = PublishEnvelope::new("0.3.0", &r);
        // UUIDv7 has version bits = 7.
        assert_eq!(env.run_id.get_version_num(), 7);
        assert_eq!(env.envelope_version, ENVELOPE_VERSION);
        assert_eq!(env.source, PublishSource::Cli);
    }

    #[test]
    fn envelope_new_timestamps_are_recent() {
        let r = sample_report();
        let before = OffsetDateTime::now_utc();
        let env = PublishEnvelope::new("0.3.0", &r);
        let after = OffsetDateTime::now_utc();
        assert!(env.published_at >= before);
        assert!(env.published_at <= after);
    }

    #[test]
    fn envelope_serializes_to_json_with_expected_fields() {
        let r = sample_report();
        let fixed_id = Uuid::parse_str("0190d8a3-9c00-7000-8000-000000000000").unwrap();
        let fixed_ts =
            OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("valid unix timestamp");
        let env = PublishEnvelope::with_fixed_id_and_time("9.9.9", &r, fixed_id, fixed_ts);

        let json = serde_json::to_value(&env).expect("serializes");
        assert_eq!(json["envelope_version"], 1);
        assert_eq!(json["run_id"], fixed_id.to_string());
        assert_eq!(json["cli_version"], "9.9.9");
        assert_eq!(json["source"], "cli");
        assert_eq!(json["report"]["version"], 2);
        assert_eq!(json["report"]["requests"]["total"], 100);
        // RFC 3339 timestamp.
        assert!(json["published_at"].as_str().unwrap().contains("2023-11-"));
    }

    #[test]
    fn envelope_source_serializes_lowercase() {
        assert_eq!(
            serde_json::to_value(PublishSource::Cli).unwrap(),
            serde_json::Value::String("cli".into())
        );
        assert_eq!(
            serde_json::to_value(PublishSource::Scheduled).unwrap(),
            serde_json::Value::String("scheduled".into())
        );
    }

    #[test]
    fn consecutive_envelope_ids_are_ordered() {
        let r = sample_report();
        let a = PublishEnvelope::new("x", &r).run_id;
        let b = PublishEnvelope::new("x", &r).run_id;
        assert!(a <= b, "UUIDv7 should be monotonically non-decreasing");
    }
}
