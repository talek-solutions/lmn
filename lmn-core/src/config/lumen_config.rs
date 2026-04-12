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

// ── ScenarioStepConfig ────────────────────────────────────────────────────────

/// Configuration for a single step within a scenario.
#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioStepConfig {
    pub name: String,
    pub host: String,
    #[serde(default = "default_get_method")]
    pub method: String,
    pub headers: Option<HashMap<String, String>>,
    pub request_template: Option<String>,
    pub response_template: Option<String>,
}

fn default_get_method() -> String {
    "get".to_string()
}

// ── ScenarioConfig ────────────────────────────────────────────────────────────

/// Configuration for a named scenario with one or more steps.
#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioConfig {
    pub name: String,
    pub weight: Option<u32>,
    pub on_step_failure: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub steps: Vec<ScenarioStepConfig>,
}

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
    pub scenarios: Option<Vec<ScenarioConfig>>,
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

    // Validate scenarios if present.
    if let Some(ref scenarios) = config.scenarios {
        // Scenarios are mutually exclusive with run.host, run.method, top-level templates.
        if config.run.as_ref().and_then(|r| r.host.as_ref()).is_some() {
            return Err(ConfigError::ValidationError(
                "'scenarios' and 'run.host' are mutually exclusive — \
                 use scenarios for multi-step runs or run.host for single-request runs"
                    .to_string(),
            ));
        }
        if config
            .run
            .as_ref()
            .and_then(|r| r.method.as_ref())
            .is_some()
        {
            return Err(ConfigError::ValidationError(
                "'scenarios' and 'run.method' are mutually exclusive — \
                 each scenario step defines its own method"
                    .to_string(),
            ));
        }
        if config.request_template.is_some() {
            return Err(ConfigError::ValidationError(
                "'scenarios' and top-level 'request_template' are mutually exclusive — \
                 each scenario step defines its own request_template"
                    .to_string(),
            ));
        }
        if config.response_template.is_some() {
            return Err(ConfigError::ValidationError(
                "'scenarios' and top-level 'response_template' are mutually exclusive — \
                 each scenario step defines its own response_template"
                    .to_string(),
            ));
        }

        // Validate each scenario.
        let mut seen_names: Vec<String> = Vec::new();
        for scenario in scenarios {
            if scenario.name.trim().is_empty() {
                return Err(ConfigError::ValidationError(
                    "scenario name must not be empty".to_string(),
                ));
            }
            if seen_names
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&scenario.name))
            {
                return Err(ConfigError::ValidationError(format!(
                    "duplicate scenario name '{}' — scenario names must be unique",
                    scenario.name
                )));
            }
            seen_names.push(scenario.name.clone());

            if let Some(w) = scenario.weight
                && w < 1
            {
                return Err(ConfigError::ValidationError(format!(
                    "scenario '{}': weight must be >= 1, got {w}",
                    scenario.name
                )));
            }

            if let Some(ref osf) = scenario.on_step_failure
                && osf != "continue"
                && osf != "abort_iteration"
            {
                return Err(ConfigError::ValidationError(format!(
                    "scenario '{}': on_step_failure must be 'continue' or \
                         'abort_iteration', got '{osf}'",
                    scenario.name
                )));
            }

            if scenario.steps.is_empty() {
                return Err(ConfigError::ValidationError(format!(
                    "scenario '{}': must have at least one step",
                    scenario.name
                )));
            }

            let mut seen_step_names: Vec<String> = Vec::new();
            for step in &scenario.steps {
                if step.name.trim().is_empty() {
                    return Err(ConfigError::ValidationError(format!(
                        "scenario '{}': step name must not be empty",
                        scenario.name
                    )));
                }
                if seen_step_names
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(&step.name))
                {
                    return Err(ConfigError::ValidationError(format!(
                        "scenario '{}': duplicate step name '{}' — \
                         step names must be unique within a scenario",
                        scenario.name, step.name
                    )));
                }
                seen_step_names.push(step.name.clone());

                if step.host.trim().is_empty() {
                    return Err(ConfigError::ValidationError(format!(
                        "scenario '{}', step '{}': host must not be empty",
                        scenario.name, step.name
                    )));
                }
            }
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

    // ── scenario tests ────────────────────────────────────────────────────────

    #[test]
    fn parse_minimal_scenario_config() {
        let yaml = r#"
scenarios:
  - name: browse
    steps:
      - name: list_products
        host: https://api.example.com/products
execution:
  request_count: 100
  concurrency: 10
"#;
        let config = parse_config(yaml).expect("should parse");
        let scenarios = config.scenarios.expect("scenarios must be Some");
        assert_eq!(scenarios.len(), 1);
        assert_eq!(scenarios[0].name, "browse");
        assert_eq!(scenarios[0].steps.len(), 1);
        assert_eq!(scenarios[0].steps[0].name, "list_products");
        assert_eq!(
            scenarios[0].steps[0].host,
            "https://api.example.com/products"
        );
        assert_eq!(scenarios[0].steps[0].method, "get");
    }

    #[test]
    fn parse_scenario_with_weight_and_on_step_failure() {
        let yaml = r#"
scenarios:
  - name: checkout
    weight: 3
    on_step_failure: abort_iteration
    steps:
      - name: login
        host: https://api.example.com/auth/login
        method: post
"#;
        let config = parse_config(yaml).expect("should parse");
        let scenarios = config.scenarios.expect("scenarios must be Some");
        assert_eq!(scenarios[0].weight, Some(3));
        assert_eq!(
            scenarios[0].on_step_failure.as_deref(),
            Some("abort_iteration")
        );
    }

    #[test]
    fn parse_scenario_step_default_method_is_get() {
        let yaml = r#"
scenarios:
  - name: browse
    steps:
      - name: list
        host: https://api.example.com/items
"#;
        let config = parse_config(yaml).expect("should parse");
        let step = &config.scenarios.unwrap()[0].steps[0];
        assert_eq!(step.method, "get");
    }

    #[test]
    fn parse_scenario_with_headers_and_templates() {
        let yaml = r#"
scenarios:
  - name: checkout
    headers:
      X-Session: abc
    steps:
      - name: login
        host: https://api.example.com/auth
        method: post
        headers:
          Content-Type: application/json
        request_template: templates/login.json
        response_template: templates/login_resp.json
"#;
        let config = parse_config(yaml).expect("should parse");
        let scenario = &config.scenarios.unwrap()[0];
        assert_eq!(
            scenario.headers.as_ref().unwrap().get("X-Session").unwrap(),
            "abc"
        );
        let step = &scenario.steps[0];
        assert_eq!(
            step.request_template.as_deref(),
            Some("templates/login.json")
        );
        assert_eq!(
            step.response_template.as_deref(),
            Some("templates/login_resp.json")
        );
    }

    #[test]
    fn parse_multiple_scenarios_with_weights() {
        let yaml = r#"
scenarios:
  - name: checkout
    weight: 3
    steps:
      - name: login
        host: https://api.example.com/auth/login
        method: post
  - name: browse
    weight: 1
    steps:
      - name: list_products
        host: https://api.example.com/products
"#;
        let config = parse_config(yaml).expect("should parse");
        let scenarios = config.scenarios.expect("scenarios must be Some");
        assert_eq!(scenarios.len(), 2);
        assert_eq!(scenarios[0].name, "checkout");
        assert_eq!(scenarios[0].weight, Some(3));
        assert_eq!(scenarios[1].name, "browse");
        assert_eq!(scenarios[1].weight, Some(1));
    }

    #[test]
    fn scenarios_with_run_host_is_error() {
        let yaml = r#"
run:
  host: https://api.example.com
scenarios:
  - name: browse
    steps:
      - name: list
        host: https://api.example.com/products
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("mutually exclusive"),
            "expected mutual exclusion error, got: {msg}"
        );
    }

    #[test]
    fn scenarios_with_run_method_is_error() {
        let yaml = r#"
run:
  method: post
scenarios:
  - name: browse
    steps:
      - name: list
        host: https://api.example.com/products
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn scenarios_with_top_level_request_template_is_error() {
        let yaml = r#"
request_template: templates/request.json
scenarios:
  - name: browse
    steps:
      - name: list
        host: https://api.example.com/products
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn scenarios_with_top_level_response_template_is_error() {
        let yaml = r#"
response_template: templates/response.json
scenarios:
  - name: browse
    steps:
      - name: list
        host: https://api.example.com/products
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn scenario_empty_name_is_error() {
        let yaml = r#"
scenarios:
  - name: ""
    steps:
      - name: list
        host: https://api.example.com/products
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn scenario_duplicate_name_is_error() {
        let yaml = r#"
scenarios:
  - name: browse
    steps:
      - name: list
        host: https://api.example.com/products
  - name: browse
    steps:
      - name: search
        host: https://api.example.com/search
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("duplicate scenario name"),
            "expected duplicate name error, got: {msg}"
        );
    }

    #[test]
    fn scenario_weight_zero_is_error() {
        let yaml = r#"
scenarios:
  - name: browse
    weight: 0
    steps:
      - name: list
        host: https://api.example.com/products
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("weight must be >= 1"),
            "expected weight error, got: {msg}"
        );
    }

    #[test]
    fn scenario_invalid_on_step_failure_is_error() {
        let yaml = r#"
scenarios:
  - name: browse
    on_step_failure: skip
    steps:
      - name: list
        host: https://api.example.com/products
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("on_step_failure"),
            "expected on_step_failure error, got: {msg}"
        );
    }

    #[test]
    fn scenario_empty_steps_is_error() {
        let yaml = r#"
scenarios:
  - name: browse
    steps: []
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("at least one step"),
            "expected steps error, got: {msg}"
        );
    }

    #[test]
    fn scenario_step_empty_name_is_error() {
        let yaml = r#"
scenarios:
  - name: browse
    steps:
      - name: ""
        host: https://api.example.com/products
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn scenario_step_duplicate_name_is_error() {
        let yaml = r#"
scenarios:
  - name: browse
    steps:
      - name: list
        host: https://api.example.com/products
      - name: list
        host: https://api.example.com/search
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("duplicate step name"),
            "expected duplicate step error, got: {msg}"
        );
    }

    #[test]
    fn scenario_step_empty_host_is_error() {
        let yaml = r#"
scenarios:
  - name: browse
    steps:
      - name: list
        host: ""
"#;
        let result = parse_config(yaml);
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }
}
