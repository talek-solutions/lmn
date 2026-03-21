use serde_json::Value;

use crate::response_template::field::{ResponseFieldType, TrackedField};

#[derive(Debug)]
pub enum ExtractedValue {
    String(String),
    Float(f64),
}

#[derive(Debug)]
pub struct ExtractionResult {
    pub values: Vec<(String, ExtractedValue)>,
    pub mismatches: Vec<String>,
}

/// Extracts tracked field values from a response body.
/// Fields that are missing or have an unexpected type are recorded as mismatches.
pub fn extract(body: &Value, fields: &[TrackedField]) -> ExtractionResult {
    let mut values = Vec::new();
    let mut mismatches = Vec::new();

    for field in fields {
        let path_label = field.path.join(".");

        match resolve_path(body, &field.path) {
            None => {
                mismatches.push(path_label);
            }
            Some(value) => match (&field.field_type, value) {
                (ResponseFieldType::String, Value::String(s)) => {
                    values.push((path_label, ExtractedValue::String(s.clone())));
                }
                (ResponseFieldType::Float, Value::Number(n)) => {
                    if let Some(f) = n.as_f64() {
                        values.push((path_label, ExtractedValue::Float(f)));
                    } else {
                        mismatches.push(path_label);
                    }
                }
                _ => {
                    mismatches.push(path_label);
                }
            },
        }
    }

    ExtractionResult { values, mismatches }
}

fn resolve_path<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_path_returns_none_for_missing_key() {
        let body = json!({ "a": { "b": 1 } });
        let path: Vec<String> = vec!["a".into(), "c".into()];
        assert!(resolve_path(&body, &path).is_none());
    }

    #[test]
    fn resolve_path_returns_deeply_nested_value() {
        let body = json!({ "a": { "b": { "c": "deep" } } });
        let path: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(resolve_path(&body, &path), Some(&json!("deep")));
    }
}
