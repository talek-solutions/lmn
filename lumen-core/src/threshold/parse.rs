use serde::Deserialize;

use super::error::ThresholdError;
use super::types::{Metric, Threshold};

/// Internal wrapper for deserializing the `{ "thresholds": [...] }` envelope.
#[derive(Deserialize)]
struct ThresholdEnvelope {
    thresholds: Vec<Threshold>,
}

/// Parses a list of thresholds from a JSON or YAML string.
///
/// The input must be an object with a `"thresholds"` key whose value is an
/// array of threshold objects:
///
/// ```json
/// { "thresholds": [{ "metric": "latency_p99", "operator": "lt", "value": 200.0 }] }
/// ```
///
/// or equivalently in YAML:
///
/// ```yaml
/// thresholds:
///   - metric: latency_p99
///     operator: lt
///     value: 200.0
/// ```
///
/// JSON is attempted first; if that fails, YAML is attempted. If both fail,
/// the JSON error is returned wrapped in `ThresholdError::ParseError`.
///
/// Each threshold is then validated:
/// - `value` must be finite
/// - For `error_rate`, `value` must be in [0.0, 1.0]
pub fn parse_thresholds(json_or_yaml: &str) -> Result<Vec<Threshold>, ThresholdError> {
    let envelope: ThresholdEnvelope = serde_json::from_str(json_or_yaml)
        .or_else(|json_err| {
            serde_yaml::from_str(json_or_yaml)
                .map_err(|_yaml_err| ThresholdError::ParseError(json_err.to_string()))
        })?;

    validate_thresholds(envelope.thresholds)
}

fn validate_thresholds(thresholds: Vec<Threshold>) -> Result<Vec<Threshold>, ThresholdError> {
    for t in &thresholds {
        if !t.value.is_finite() {
            return Err(ThresholdError::ValidationError(format!(
                "threshold value must be finite, got: {}",
                t.value
            )));
        }

        if t.metric == Metric::ErrorRate && !(0.0..=1.0).contains(&t.value) {
            return Err(ThresholdError::ValidationError(format!(
                "error_rate threshold value must be in [0.0, 1.0], got: {}",
                t.value
            )));
        }
    }

    Ok(thresholds)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threshold::types::{Metric, Operator};

    #[test]
    fn parse_valid_json_thresholds() {
        let json = r#"{
            "thresholds": [
                { "metric": "latency_p99", "operator": "lt", "value": 200.0 },
                { "metric": "error_rate", "operator": "lte", "value": 0.05 }
            ]
        }"#;
        let thresholds = parse_thresholds(json).expect("should parse");
        assert_eq!(thresholds.len(), 2);
        assert_eq!(thresholds[0].metric, Metric::LatencyP99);
        assert_eq!(thresholds[0].operator, Operator::Lt);
        assert!((thresholds[0].value - 200.0).abs() < f64::EPSILON);
        assert_eq!(thresholds[1].metric, Metric::ErrorRate);
        assert_eq!(thresholds[1].operator, Operator::Lte);
    }

    #[test]
    fn parse_valid_yaml_thresholds() {
        let yaml = "thresholds:\n  - metric: latency_p99\n    operator: lt\n    value: 200.0\n  - metric: throughput_rps\n    operator: gte\n    value: 10.0\n";
        let thresholds = parse_thresholds(yaml).expect("should parse YAML");
        assert_eq!(thresholds.len(), 2);
        assert_eq!(thresholds[0].metric, Metric::LatencyP99);
        assert_eq!(thresholds[1].metric, Metric::ThroughputRps);
    }

    #[test]
    fn parse_invalid_metric_returns_error() {
        let json = r#"{ "thresholds": [{ "metric": "does_not_exist", "operator": "lt", "value": 100.0 }] }"#;
        let result = parse_thresholds(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_operator_returns_error() {
        let json = r#"{ "thresholds": [{ "metric": "latency_p99", "operator": "not_an_op", "value": 100.0 }] }"#;
        let result = parse_thresholds(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_error_rate_above_1_returns_error() {
        let json = r#"{ "thresholds": [{ "metric": "error_rate", "operator": "lte", "value": 1.5 }] }"#;
        let result = parse_thresholds(json);
        assert!(matches!(result, Err(ThresholdError::ValidationError(_))));
    }

    #[test]
    fn parse_infinite_value_returns_error() {
        // We can't express infinity in JSON directly, but we can test via a crafted f64.
        // Build thresholds manually and call the private validator.
        // Instead, test via YAML which supports .inf
        let yaml = "thresholds:\n  - metric: latency_p99\n    operator: lt\n    value: .inf\n";
        let result = parse_thresholds(yaml);
        assert!(matches!(result, Err(ThresholdError::ValidationError(_))));
    }

    #[test]
    fn parse_empty_thresholds_array_ok() {
        let json = r#"{ "thresholds": [] }"#;
        let thresholds = parse_thresholds(json).expect("empty array is valid");
        assert!(thresholds.is_empty());
    }
}
