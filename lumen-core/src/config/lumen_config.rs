use serde::Deserialize;

use crate::config::error::ConfigError;
use crate::threshold::Threshold;

// ── RunConfig ─────────────────────────────────────────────────────────────────

/// Optional run-level configuration. All fields are `Option` — CLI flags fill
/// in any values left unset.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RunConfig {
    pub host: Option<String>,
    pub method: Option<String>,
    pub requests: Option<usize>,
    pub concurrency: Option<usize>,
    pub output: Option<String>,
    pub output_file: Option<String>,
}

// ── CurveConfig ───────────────────────────────────────────────────────────────

/// Configuration for a load curve run. Wraps the existing `Stage` type from
/// the `load_curve` domain to allow YAML deserialization.
#[derive(Debug, Clone, Deserialize)]
pub struct CurveConfig {
    pub stages: Vec<crate::load_curve::Stage>,
}

// ── LumenConfig ───────────────────────────────────────────────────────────────

/// Top-level YAML configuration for a lumen run.
///
/// All sections are optional — a minimal config may specify only `run.host`.
/// CLI flags always take precedence and fill in any missing values.
///
/// # Example YAML
/// ```yaml
/// run:
///   host: http://localhost:8080
///   requests: 1000
///   concurrency: 50
/// thresholds:
///   - metric: latency_p99
///     operator: lt
///     value: 200.0
/// ```
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LumenConfig {
    pub run: Option<RunConfig>,
    pub curve: Option<CurveConfig>,
    pub thresholds: Option<Vec<Threshold>>,
    pub request_template: Option<String>,
    pub response_template: Option<String>,
}

// ── parse_config ──────────────────────────────────────────────────────────────

/// Parses a `LumenConfig` from a YAML string.
///
/// Returns `ConfigError::YamlParseError` if the YAML is malformed.
pub fn parse_config(yaml: &str) -> Result<LumenConfig, ConfigError> {
    serde_yaml::from_str(yaml).map_err(ConfigError::YamlParseError)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threshold::{Metric, Operator};

    #[test]
    fn parse_minimal_config_host_only() {
        let yaml = "run:\n  host: http://localhost:8080\n";
        let config = parse_config(yaml).expect("should parse");
        let run = config.run.expect("run must be Some");
        assert_eq!(run.host.as_deref(), Some("http://localhost:8080"));
        assert!(run.requests.is_none());
        assert!(config.thresholds.is_none());
    }

    #[test]
    fn parse_full_config_all_sections() {
        let yaml = r#"
run:
  host: http://api.example.com
  method: POST
  requests: 500
  concurrency: 25
  output: json
  output_file: /tmp/report.json
curve:
  stages:
    - duration: 30s
      target_vus: 10
    - duration: 1m
      target_vus: 50
request_template: /templates/request.json
response_template: /templates/response.json
"#;
        let config = parse_config(yaml).expect("should parse");
        let run = config.run.expect("run must be Some");
        assert_eq!(run.host.as_deref(), Some("http://api.example.com"));
        assert_eq!(run.method.as_deref(), Some("POST"));
        assert_eq!(run.requests, Some(500));
        assert_eq!(run.concurrency, Some(25));
        assert_eq!(run.output.as_deref(), Some("json"));
        assert_eq!(run.output_file.as_deref(), Some("/tmp/report.json"));

        let curve = config.curve.expect("curve must be Some");
        assert_eq!(curve.stages.len(), 2);
        assert_eq!(curve.stages[0].target_vus, 10);
        assert_eq!(curve.stages[1].target_vus, 50);

        assert_eq!(config.request_template.as_deref(), Some("/templates/request.json"));
        assert_eq!(config.response_template.as_deref(), Some("/templates/response.json"));
    }

    #[test]
    fn parse_config_with_thresholds() {
        let yaml = r#"
run:
  host: http://localhost:3000
thresholds:
  - metric: latency_p99
    operator: lt
    value: 200.0
  - metric: error_rate
    operator: lte
    value: 0.01
"#;
        let config = parse_config(yaml).expect("should parse");
        let thresholds = config.thresholds.expect("thresholds must be Some");
        assert_eq!(thresholds.len(), 2);
        assert_eq!(thresholds[0].metric, Metric::LatencyP99);
        assert_eq!(thresholds[0].operator, Operator::Lt);
        assert!((thresholds[0].value - 200.0).abs() < f64::EPSILON);
        assert_eq!(thresholds[1].metric, Metric::ErrorRate);
        assert_eq!(thresholds[1].operator, Operator::Lte);
    }

    #[test]
    fn parse_invalid_yaml_returns_error() {
        let bad = "run:\n  host: [\nnot valid yaml";
        let result = parse_config(bad);
        assert!(matches!(result, Err(ConfigError::YamlParseError(_))));
    }

    #[test]
    fn parse_config_with_request_template() {
        let yaml = "request_template: /path/to/template.json\n";
        let config = parse_config(yaml).expect("should parse");
        assert_eq!(
            config.request_template.as_deref(),
            Some("/path/to/template.json")
        );
        assert!(config.run.is_none());
        assert!(config.thresholds.is_none());
    }
}
