use std::collections::HashMap;

use serde::Deserialize;

use crate::config::error::ConfigError;
use crate::threshold::Threshold;
use crate::threshold::parse::validate_thresholds;

const MAX_CONCURRENCY: usize = 10_000;
const MAX_REQUEST_COUNT: usize = 100_000_000;
const MAX_HEADERS: usize = 64;
const MAX_HEADER_NAME_LEN: usize = 256;
const MAX_HEADER_VALUE_LEN: usize = 8192;

// ── RunConfig ─────────────────────────────────────────────────────────────────

/// Optional run-level configuration. All fields are `Option` — CLI flags fill
/// in any values left unset.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RunConfig {
    pub host: Option<String>,
    pub method: Option<String>,
    pub output: Option<String>,
    pub output_file: Option<String>,
    /// Optional static HTTP headers to send with every request.
    /// Values may contain `${ENV_VAR}` placeholders resolved at run start.
    pub headers: Option<HashMap<String, String>>,
}

// ── ExecutionConfig ───────────────────────────────────────────────────────────

/// Configuration for the execution strategy.
///
/// When `stages` is present, a `LoadCurve` is built via `TryFrom<ExecutionConfig>`.
/// Otherwise `request_count` and `concurrency` are used for fixed-mode execution.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExecutionConfig {
    pub request_count: Option<usize>,
    pub concurrency: Option<usize>,
    pub stages: Option<Vec<crate::load_curve::Stage>>,
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
///
/// execution:
///   request_count: 1000
///   concurrency: 50
///
/// thresholds:
///   - metric: latency_p99
///     operator: lt
///     value: 200.0
/// ```
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LumenConfig {
    pub run: Option<RunConfig>,
    pub execution: Option<ExecutionConfig>,
    pub thresholds: Option<Vec<Threshold>>,
    pub request_template: Option<String>,
    pub response_template: Option<String>,
}

// ── parse_config ──────────────────────────────────────────────────────────────

/// Parses a `LumenConfig` from a YAML string.
///
/// Returns `ConfigError::YamlParseError` if the YAML is malformed.
/// Returns `ConfigError::ValidationError` if threshold values are invalid
/// (non-finite, or `error_rate` outside [0.0, 1.0]).
pub fn parse_config(yaml: &str) -> Result<LumenConfig, ConfigError> {
    let mut config: LumenConfig =
        serde_norway::from_str(yaml).map_err(ConfigError::YamlParseError)?;

    // Validate thresholds if present — serde_norway bypasses the validation
    // in parse_thresholds(), so we run it explicitly here.
    if let Some(thresholds) = config.thresholds.take() {
        config.thresholds = Some(
            validate_thresholds(thresholds)
                .map_err(|e| ConfigError::ValidationError(e.to_string()))?,
        );
    }

    // Validate execution: stages and request_count/concurrency are mutually exclusive.
    if let Some(ref exec) = config.execution {
        let has_stages = exec.stages.is_some();
        let has_fixed = exec.request_count.is_some() || exec.concurrency.is_some();
        if has_stages && has_fixed {
            return Err(ConfigError::ValidationError(
                "'execution.stages' and 'execution.request_count'/'execution.concurrency' \
                 are mutually exclusive — use stages for curve mode or \
                 request_count/concurrency for fixed mode"
                    .to_string(),
            ));
        }
    }

    // Validate numeric bounds.
    if let Some(ref run) = config.run {
        // Validate headers if present.
        if let Some(ref headers) = run.headers {
            if headers.len() > MAX_HEADERS {
                return Err(ConfigError::ValidationError(format!(
                    "headers count {} exceeds maximum ({MAX_HEADERS})",
                    headers.len()
                )));
            }
            for (name, value) in headers {
                if name.len() > MAX_HEADER_NAME_LEN {
                    return Err(ConfigError::ValidationError(format!(
                        "header name '{name}' exceeds maximum length ({MAX_HEADER_NAME_LEN})"
                    )));
                }
                if value.len() > MAX_HEADER_VALUE_LEN {
                    return Err(ConfigError::ValidationError(format!(
                        "header value for '{name}' exceeds maximum length ({MAX_HEADER_VALUE_LEN})"
                    )));
                }
            }
        }
    }
    if let Some(ref exec) = config.execution {
        if let Some(v) = exec.request_count
            && v > MAX_REQUEST_COUNT
        {
            return Err(ConfigError::ValidationError(format!(
                "request_count {v} exceeds maximum ({MAX_REQUEST_COUNT})"
            )));
        }
        if let Some(v) = exec.concurrency
            && v > MAX_CONCURRENCY
        {
            return Err(ConfigError::ValidationError(format!(
                "concurrency {v} exceeds maximum ({MAX_CONCURRENCY})"
            )));
        }
    }

    Ok(config)
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
        assert!(config.thresholds.is_none());
    }

    #[test]
    fn parse_full_config_all_sections() {
        let yaml = r#"
run:
  host: http://api.example.com
  method: POST
  output: json
  output_file: /tmp/report.json

execution:
  request_count: 500
  concurrency: 25

request_template: /templates/request.json
response_template: /templates/response.json
"#;
        let config = parse_config(yaml).expect("should parse");
        let run = config.run.expect("run must be Some");
        assert_eq!(run.host.as_deref(), Some("http://api.example.com"));
        assert_eq!(run.method.as_deref(), Some("POST"));
        assert_eq!(run.output.as_deref(), Some("json"));
        assert_eq!(run.output_file.as_deref(), Some("/tmp/report.json"));

        let exec = config.execution.expect("execution must be Some");
        assert_eq!(exec.request_count, Some(500));
        assert_eq!(exec.concurrency, Some(25));
        assert!(exec.stages.is_none());

        assert_eq!(
            config.request_template.as_deref(),
            Some("/templates/request.json")
        );
        assert_eq!(
            config.response_template.as_deref(),
            Some("/templates/response.json")
        );
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

    // ── execution section tests ───────────────────────────────────────────────

    #[test]
    fn parse_execution_with_request_count_and_concurrency() {
        let yaml = r#"
run:
  host: http://localhost:8080
execution:
  request_count: 1000
  concurrency: 50
"#;
        let config = parse_config(yaml).expect("should parse");
        let exec = config.execution.expect("execution must be Some");
        assert_eq!(exec.request_count, Some(1000));
        assert_eq!(exec.concurrency, Some(50));
        assert!(exec.stages.is_none());
    }

    #[test]
    fn parse_execution_with_stages() {
        let yaml = r#"
run:
  host: http://localhost:8080
execution:
  stages:
    - duration: 30s
      target_vus: 10
    - duration: 1m
      target_vus: 50
"#;
        let config = parse_config(yaml).expect("should parse");
        let exec = config.execution.expect("execution must be Some");
        let stages = exec.stages.expect("stages must be Some");
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].target_vus, 10);
        assert_eq!(stages[1].target_vus, 50);
        assert!(exec.request_count.is_none());
        assert!(exec.concurrency.is_none());
    }

    #[test]
    fn parse_run_with_output_format() {
        let yaml = r#"
run:
  host: http://localhost:8080
  output: json
  output_file: /tmp/results.json
"#;
        let config = parse_config(yaml).expect("should parse");
        let run = config.run.expect("run must be Some");
        assert_eq!(run.host.as_deref(), Some("http://localhost:8080"));
        assert_eq!(run.output.as_deref(), Some("json"));
        assert_eq!(run.output_file.as_deref(), Some("/tmp/results.json"));
    }

    #[test]
    fn parse_config_stages_and_request_count_is_error() {
        let yaml = r#"
execution:
  stages:
    - duration: 30s
      target_vus: 10
  request_count: 1000
"#;
        let result = parse_config(yaml);
        assert!(
            matches!(result, Err(ConfigError::ValidationError(_))),
            "expected ValidationError for stages + request_count, got: {result:?}"
        );
    }

    #[test]
    fn parse_config_stages_and_concurrency_is_error() {
        let yaml = r#"
execution:
  stages:
    - duration: 30s
      target_vus: 10
  concurrency: 50
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn parse_config_request_count_exceeds_max_is_error() {
        let yaml = "execution:\n  request_count: 100000001\n";
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn parse_config_concurrency_exceeds_max_is_error() {
        let yaml = "execution:\n  concurrency: 10001\n";
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }
}
