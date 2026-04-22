use std::time::Duration;

use serde::Deserialize;

use crate::config::SensitiveString;
use crate::publish::error::PublishError;

// ── Constants ─────────────────────────────────────────────────────────────────

/// The default platform endpoint used when no other source specifies one.
///
/// This is intentionally a `const &str` rather than a build-time env var:
/// self-hosted operators override via `--publish-url` or `LUMEN_API_URL`.
pub const DEFAULT_PUBLISH_URL: &str = "https://api.talek.cloud";

/// Maximum serialized payload size accepted before publishing is aborted.
/// Chosen to protect the platform from unbounded growth in edge cases.
pub const MAX_PAYLOAD_BYTES: usize = 5 * 1024 * 1024; // 5 MiB

/// Per-request HTTP timeout. 30s is long enough for slow CI egress yet short
/// enough that a hung connection does not stall the whole run.
pub const DEFAULT_PUBLISH_TIMEOUT: Duration = Duration::from_secs(30);

// ── PublishConfigYaml ─────────────────────────────────────────────────────────

/// The YAML-facing shape of the `publish:` section.
///
/// Deliberately kept separate from [`PublishConfig`] (the resolved struct) so
/// YAML parsing is a pure data-shape concern, independent of env/flag
/// precedence resolution. API keys are intentionally NOT a field here — the
/// API key is always read from `LUMEN_API_KEY` env, never from YAML.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct PublishConfigYaml {
    /// When `true`, publishing is enabled without requiring `--publish` on the
    /// command line. The CLI flag, if present, still takes precedence.
    pub enabled: Option<bool>,
    /// Optional override of the platform URL. Host-only; the client appends
    /// the path (`/v1/runs`) at send time.
    pub url: Option<String>,
}

// ── PublishConfig ─────────────────────────────────────────────────────────────

/// Fully-resolved publish configuration ready to hand to a [`ResultSink`].
///
/// Construction goes through [`PublishConfigBuilder`] which applies the
/// precedence rules documented in `.docs/cloud/TECH.md`:
///
/// - **enabled:** `--publish` flag > `publish.enabled` YAML > `false`
/// - **url:** `--publish-url` flag > `LUMEN_API_URL` env > `publish.url` YAML > default
/// - **api_key:** `LUMEN_API_KEY` env only (never a flag, never in YAML)
///
/// [`ResultSink`]: crate::publish::ResultSink
#[derive(Debug, Clone)]
pub struct PublishConfig {
    /// Validated, host-only base URL. No trailing slash.
    pub base_url: String,
    /// API key read from `LUMEN_API_KEY` env.
    pub api_key: SensitiveString,
    /// Per-request HTTP timeout.
    pub timeout: Duration,
}

// ── PublishConfigBuilder ──────────────────────────────────────────────────────

/// Resolves all configuration sources into a [`PublishConfig`].
///
/// The builder follows the struct-based parameter convention used throughout
/// lmn-core: each source contributes a field, and `build()` applies the
/// precedence and validation rules.
#[derive(Debug, Default)]
pub struct PublishConfigBuilder {
    pub flag_enabled: bool,
    pub flag_url: Option<String>,
    pub env_url: Option<String>,
    pub env_api_key: Option<String>,
    pub yaml: Option<PublishConfigYaml>,
    pub timeout: Option<Duration>,
}

impl PublishConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consumes the builder and applies precedence + validation.
    ///
    /// Returns `Ok(None)` when publishing is not enabled (no POST will be
    /// attempted and no API key is required). Returns `Ok(Some(config))`
    /// when publishing is enabled and fully configured. Returns `Err` when
    /// publishing is enabled but misconfigured (missing API key, invalid URL).
    pub fn build(self) -> Result<Option<PublishConfig>, PublishError> {
        // ── enabled resolution ────────────────────────────────────────────
        let enabled = if self.flag_enabled {
            true
        } else {
            self.yaml.as_ref().and_then(|y| y.enabled).unwrap_or(false)
        };

        if !enabled {
            return Ok(None);
        }

        // ── url resolution ────────────────────────────────────────────────
        let raw_url = self
            .flag_url
            .or(self.env_url)
            .or_else(|| self.yaml.as_ref().and_then(|y| y.url.clone()))
            .unwrap_or_else(|| DEFAULT_PUBLISH_URL.to_string());

        let base_url = normalize_and_validate_url(&raw_url)?;

        // ── api_key resolution ────────────────────────────────────────────
        let api_key_raw = self
            .env_api_key
            .filter(|s| !s.trim().is_empty())
            .ok_or(PublishError::MissingApiKey)?;

        Ok(Some(PublishConfig {
            base_url,
            api_key: SensitiveString::new(api_key_raw),
            timeout: self.timeout.unwrap_or(DEFAULT_PUBLISH_TIMEOUT),
        }))
    }
}

// ── URL normalization & validation ────────────────────────────────────────────

/// Validates that `raw` is a well-formed URL, enforces HTTPS (with a loopback
/// exception for local development), and strips any trailing slash so the
/// client can cleanly append the path version.
///
/// The loopback exception covers `localhost`, `127.0.0.1`, and `[::1]`. No
/// other hosts are permitted over plain HTTP, and there is no
/// `--insecure` escape hatch.
pub fn normalize_and_validate_url(raw: &str) -> Result<String, PublishError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(PublishError::InvalidUrl("URL is empty".into()));
    }

    // Reject URLs that include a path, query, or fragment — config is host-only.
    // The client owns the path (`/v1/runs`).
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|e| PublishError::InvalidUrl(format!("could not parse '{trimmed}': {e}")))?;

    let scheme = parsed.scheme();
    let is_https = scheme == "https";
    let is_http = scheme == "http";
    if !is_https && !is_http {
        return Err(PublishError::InvalidUrl(format!(
            "scheme '{scheme}' is not supported — use https:// (or http:// for localhost only)"
        )));
    }

    let host = parsed.host_str().unwrap_or("");
    let is_loopback = matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]");

    if is_http && !is_loopback {
        return Err(PublishError::InvalidUrl(format!(
            "HTTPS is required for non-loopback hosts; got http://{host} — \
             use https:// or an allowed loopback address (localhost, 127.0.0.1, ::1)"
        )));
    }

    // Require host-only URLs: the client appends path versioning itself.
    let path = parsed.path();
    if !path.is_empty() && path != "/" {
        return Err(PublishError::InvalidUrl(format!(
            "URL must be host-only (no path); got path '{path}' — the CLI owns the /v1/runs path"
        )));
    }
    if parsed.query().is_some() {
        return Err(PublishError::InvalidUrl(
            "URL must not contain a query string".into(),
        ));
    }
    if parsed.fragment().is_some() {
        return Err(PublishError::InvalidUrl(
            "URL must not contain a fragment".into(),
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(PublishError::InvalidUrl(
            "URL must not contain userinfo (user:password@...)".into(),
        ));
    }

    // Canonicalize: drop trailing slash so path append is clean.
    let mut normalized = parsed.to_string();
    if normalized.ends_with('/') {
        normalized.pop();
    }
    Ok(normalized)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── URL validation ────────────────────────────────────────────────────────

    #[test]
    fn accepts_https_url() {
        let u = normalize_and_validate_url("https://api.talek.cloud").unwrap();
        assert_eq!(u, "https://api.talek.cloud");
    }

    #[test]
    fn strips_trailing_slash() {
        let u = normalize_and_validate_url("https://api.talek.cloud/").unwrap();
        assert_eq!(u, "https://api.talek.cloud");
    }

    #[test]
    fn allows_http_localhost() {
        let u = normalize_and_validate_url("http://localhost:3000").unwrap();
        assert_eq!(u, "http://localhost:3000");
    }

    #[test]
    fn allows_http_127() {
        let u = normalize_and_validate_url("http://127.0.0.1:8080").unwrap();
        assert_eq!(u, "http://127.0.0.1:8080");
    }

    #[test]
    fn rejects_http_public_host() {
        let err = normalize_and_validate_url("http://api.example.com").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("HTTPS"), "expected HTTPS error, got: {msg}");
    }

    #[test]
    fn rejects_bad_scheme() {
        let err = normalize_and_validate_url("ftp://example.com").unwrap_err();
        assert!(err.to_string().contains("scheme"));
    }

    #[test]
    fn rejects_url_with_path() {
        let err = normalize_and_validate_url("https://api.talek.cloud/some/path").unwrap_err();
        assert!(err.to_string().contains("host-only"));
    }

    #[test]
    fn rejects_url_with_query() {
        let err = normalize_and_validate_url("https://api.talek.cloud?x=1").unwrap_err();
        assert!(err.to_string().contains("query"));
    }

    #[test]
    fn rejects_url_with_userinfo() {
        let err = normalize_and_validate_url("https://user:pass@api.talek.cloud").unwrap_err();
        assert!(err.to_string().contains("userinfo"));
    }

    #[test]
    fn rejects_empty_url() {
        let err = normalize_and_validate_url("").unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn rejects_malformed_url() {
        let err = normalize_and_validate_url("not a url").unwrap_err();
        assert!(err.to_string().contains("parse"));
    }

    #[test]
    fn trims_whitespace() {
        let u = normalize_and_validate_url("  https://api.talek.cloud  ").unwrap();
        assert_eq!(u, "https://api.talek.cloud");
    }

    // ── Builder precedence ────────────────────────────────────────────────────

    #[test]
    fn builder_disabled_returns_none() {
        let result = PublishConfigBuilder::new().build().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn builder_yaml_enabled_returns_config() {
        let result = PublishConfigBuilder {
            yaml: Some(PublishConfigYaml {
                enabled: Some(true),
                url: None,
            }),
            env_api_key: Some("test-key".into()),
            ..Default::default()
        }
        .build()
        .unwrap()
        .expect("expected Some");
        assert_eq!(result.base_url, DEFAULT_PUBLISH_URL);
    }

    #[test]
    fn builder_flag_overrides_yaml_disabled() {
        let result = PublishConfigBuilder {
            flag_enabled: true,
            yaml: Some(PublishConfigYaml {
                enabled: Some(false),
                url: None,
            }),
            env_api_key: Some("k".into()),
            ..Default::default()
        }
        .build()
        .unwrap();
        assert!(result.is_some(), "flag should override yaml disabled");
    }

    #[test]
    fn builder_flag_url_wins_over_env() {
        let result = PublishConfigBuilder {
            flag_enabled: true,
            flag_url: Some("https://flag.example.com".into()),
            env_url: Some("https://env.example.com".into()),
            env_api_key: Some("k".into()),
            ..Default::default()
        }
        .build()
        .unwrap()
        .expect("expected Some");
        assert_eq!(result.base_url, "https://flag.example.com");
    }

    #[test]
    fn builder_env_url_wins_over_yaml() {
        let result = PublishConfigBuilder {
            flag_enabled: true,
            env_url: Some("https://env.example.com".into()),
            yaml: Some(PublishConfigYaml {
                enabled: Some(true),
                url: Some("https://yaml.example.com".into()),
            }),
            env_api_key: Some("k".into()),
            ..Default::default()
        }
        .build()
        .unwrap()
        .expect("expected Some");
        assert_eq!(result.base_url, "https://env.example.com");
    }

    #[test]
    fn builder_yaml_url_wins_over_default() {
        let result = PublishConfigBuilder {
            flag_enabled: true,
            yaml: Some(PublishConfigYaml {
                enabled: Some(true),
                url: Some("https://yaml.example.com".into()),
            }),
            env_api_key: Some("k".into()),
            ..Default::default()
        }
        .build()
        .unwrap()
        .expect("expected Some");
        assert_eq!(result.base_url, "https://yaml.example.com");
    }

    #[test]
    fn builder_default_url_when_none_configured() {
        let result = PublishConfigBuilder {
            flag_enabled: true,
            env_api_key: Some("k".into()),
            ..Default::default()
        }
        .build()
        .unwrap()
        .expect("expected Some");
        assert_eq!(result.base_url, DEFAULT_PUBLISH_URL);
    }

    #[test]
    fn builder_missing_api_key_errors() {
        let err = PublishConfigBuilder {
            flag_enabled: true,
            ..Default::default()
        }
        .build()
        .unwrap_err();
        assert!(matches!(err, PublishError::MissingApiKey));
    }

    #[test]
    fn builder_empty_api_key_errors() {
        let err = PublishConfigBuilder {
            flag_enabled: true,
            env_api_key: Some("   ".into()),
            ..Default::default()
        }
        .build()
        .unwrap_err();
        assert!(matches!(err, PublishError::MissingApiKey));
    }

    #[test]
    fn builder_invalid_url_errors() {
        let err = PublishConfigBuilder {
            flag_enabled: true,
            flag_url: Some("http://public.example.com".into()),
            env_api_key: Some("k".into()),
            ..Default::default()
        }
        .build()
        .unwrap_err();
        assert!(matches!(err, PublishError::InvalidUrl(_)));
    }

    // ── YAML parsing (via serde_norway) ───────────────────────────────────────

    #[test]
    fn parse_publish_yaml_full() {
        let yaml = "enabled: true\nurl: https://api.example.com\n";
        let parsed: PublishConfigYaml = serde_norway::from_str(yaml).unwrap();
        assert_eq!(parsed.enabled, Some(true));
        assert_eq!(parsed.url.as_deref(), Some("https://api.example.com"));
    }

    #[test]
    fn parse_publish_yaml_enabled_only() {
        let yaml = "enabled: true\n";
        let parsed: PublishConfigYaml = serde_norway::from_str(yaml).unwrap();
        assert_eq!(parsed.enabled, Some(true));
        assert!(parsed.url.is_none());
    }

    #[test]
    fn parse_publish_yaml_empty() {
        let yaml = "{}";
        let parsed: PublishConfigYaml = serde_norway::from_str(yaml).unwrap();
        assert!(parsed.enabled.is_none());
        assert!(parsed.url.is_none());
    }
}
