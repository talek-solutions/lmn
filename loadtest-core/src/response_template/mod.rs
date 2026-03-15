pub mod error;
pub mod extractor;
pub mod field;
pub mod stats;

use std::path::Path;

use serde_json::Value;
use tracing::instrument;

use error::ResponseTemplateError;
use field::TrackedField;

pub struct ResponseTemplate {
    pub fields: Vec<TrackedField>,
}

impl ResponseTemplate {
    #[instrument(name = "loadtest.response_template.parse", fields(path = %path.display()))]
    pub fn parse(path: &Path) -> Result<Self, ResponseTemplateError> {
        let content = std::fs::read_to_string(path)?;
        let root: Value = serde_json::from_str(&content)?;

        let fields = field::collect_tracked_fields(&root, &[])?;

        if fields.is_empty() {
            return Err(ResponseTemplateError::InvalidFieldType(
                "template contains no tracked fields".to_string(),
            ));
        }

        Ok(ResponseTemplate { fields })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parse_fails_on_missing_file() {
        assert!(ResponseTemplate::parse(Path::new("nonexistent.json")).is_err());
    }

    #[test]
    fn parse_fails_on_invalid_json() {
        let f = write_temp("not json");
        assert!(ResponseTemplate::parse(f.path()).is_err());
    }

    #[test]
    fn parse_fails_when_no_tracked_fields() {
        let f = write_temp(r#"{"status": "ok"}"#);
        assert!(ResponseTemplate::parse(f.path()).is_err());
    }

    #[test]
    fn parse_succeeds_with_valid_template() {
        let f = write_temp(r#"{"status": "{{STRING}}"}"#);
        let rt = ResponseTemplate::parse(f.path()).unwrap();
        assert_eq!(rt.fields.len(), 1);
    }
}
