use serde_json::Value;

use crate::response_template::error::ResponseTemplateError;

#[derive(Debug, Clone)]
pub enum ResponseFieldType {
    String,
    Float,
}

#[derive(Debug, Clone)]
pub struct TrackedField {
    pub path: Vec<String>,
    pub field_type: ResponseFieldType,
}

/// Walks a JSON template value and collects all `{{TYPE}}` leaf placeholders
/// along with their dot-separated paths.
pub fn collect_tracked_fields(
    value: &Value,
    prefix: &[String],
) -> Result<Vec<TrackedField>, ResponseTemplateError> {
    let mut fields = Vec::new();

    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let mut path = prefix.to_vec();
                path.push(key.clone());
                fields.extend(collect_tracked_fields(child, &path)?);
            }
        }
        Value::String(s) => {
            if let Some(field_type) = parse_field_type(s)? {
                fields.push(TrackedField {
                    path: prefix.to_vec(),
                    field_type,
                });
            }
        }
        _ => {}
    }

    Ok(fields)
}

fn parse_field_type(s: &str) -> Result<Option<ResponseFieldType>, ResponseTemplateError> {
    let trimmed = s.trim();
    let inner = match trimmed
        .strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))
    {
        Some(inner) => inner,
        None => return Ok(None),
    };

    match inner {
        "STRING" => Ok(Some(ResponseFieldType::String)),
        "FLOAT" => Ok(Some(ResponseFieldType::Float)),
        _ => Err(ResponseTemplateError::InvalidFieldType(inner.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_string_placeholder() {
        let result = parse_field_type("{{STRING}}").unwrap();
        assert!(matches!(result, Some(ResponseFieldType::String)));
    }

    #[test]
    fn parse_float_placeholder() {
        let result = parse_field_type("{{FLOAT}}").unwrap();
        assert!(matches!(result, Some(ResponseFieldType::Float)));
    }

    #[test]
    fn parse_non_placeholder_returns_none() {
        assert!(parse_field_type("plain").unwrap().is_none());
    }

    #[test]
    fn parse_unknown_type_returns_err() {
        assert!(parse_field_type("{{UNKNOWN}}").is_err());
    }
}
