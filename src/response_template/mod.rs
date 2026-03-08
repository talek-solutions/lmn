pub mod error;
pub mod extractor;
pub mod field;
pub mod stats;

use std::path::Path;

use serde_json::Value;

use error::ResponseTemplateError;
use field::TrackedField;

pub struct ResponseTemplate {
    pub fields: Vec<TrackedField>,
}

impl ResponseTemplate {
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
