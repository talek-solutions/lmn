pub mod definition;
pub mod error;
pub mod generator;
pub mod renderer;
mod generators;
mod validators;

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;
use tracing::instrument;

pub use error::TemplateError;
use generator::GeneratorContext;

const METADATA_KEY: &str = "_loadtest_metadata_templates";

// ── Placeholder parsing ───────────────────────────────────────────────────────

pub struct PlaceholderRef {
    pub name: String,
    pub once: bool,
}

/// Parses `{{name}}` or `{{name:once}}` from a string.
/// Returns `None` if the string is not a placeholder.
pub fn parse_placeholder(s: &str) -> Option<PlaceholderRef> {
    let inner = s.trim().strip_prefix("{{")?.strip_suffix("}}")?;
    if inner.is_empty() {
        return None;
    }
    let (name, once) = match inner.strip_suffix(":once") {
        Some(n) => (n, true),
        None => (inner, false),
    };
    if name.is_empty() {
        return None;
    }
    Some(PlaceholderRef { name: name.to_string(), once })
}

// ── Template ──────────────────────────────────────────────────────────────────

pub struct Template {
    body: Value,
    context: GeneratorContext,
}

impl Template {
    /// Reads, parses, and fully validates a template file.
    /// Fails fast on any invalid configuration before any requests are made.
    #[instrument(fields(path = %path.display()))]
    pub fn parse(path: &Path) -> Result<Self, TemplateError> {
        let content = std::fs::read_to_string(path)?;
        let mut root: serde_json::Map<String, Value> = serde_json::from_str(&content)?;

        // Extract metadata — it must not appear in request bodies
        let metadata = root
            .remove(METADATA_KEY)
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        let raw_defs: HashMap<String, definition::RawTemplateDef> =
            serde_json::from_value(metadata)?;
        let defs = definition::validate_all(raw_defs)?;

        let body = Value::Object(root);

        // All body placeholders must have definitions
        renderer::validate_placeholders(&body, &defs)?;

        // Object compositions must not form cycles
        definition::check_circular_refs(&defs)?;

        // Pre-resolve :once placeholders — same value reused across all requests
        let ctx = GeneratorContext::new(defs);
        let mut rng = rand::thread_rng();

        let once_values: HashMap<String, Value> = renderer::collect_once_placeholder_names(&body)
            .into_iter()
            .map(|name| {
                let val = ctx.generate_by_name(&name, &mut rng);
                (name, val)
            })
            .collect();

        Ok(Template { body, context: ctx.with_once_values(once_values) })
    }

    /// Pre-generates `n` request bodies, each with independently rendered placeholders.
    /// `:once` placeholders share the same value across all `n` bodies.
    #[instrument(skip(self), fields(n))]
    pub fn pre_generate(&self, n: usize) -> Vec<String> {
        let mut rng = rand::thread_rng();
        (0..n)
            .map(|_| {
                let rendered = renderer::render(&self.body, &self.context, &mut rng);
                serde_json::to_string(&rendered).expect("rendered Value is always valid JSON")
            })
            .collect()
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
        assert!(Template::parse(Path::new("nonexistent.json")).is_err());
    }

    #[test]
    fn parse_fails_on_invalid_json() {
        let f = write_temp("not json");
        assert!(Template::parse(f.path()).is_err());
    }

    #[test]
    fn parse_fails_on_unknown_placeholder() {
        let f = write_temp(r#"{"field": "{{undefined}}"}"#);
        assert!(Template::parse(f.path()).is_err());
    }

    #[test]
    fn parse_fails_on_circular_reference() {
        let f = write_temp(r#"{
            "field": "{{a}}",
            "_loadtest_metadata_templates": {
                "a": { "type": "object", "composition": { "x": "{{b}}" } },
                "b": { "type": "object", "composition": { "y": "{{a}}" } }
            }
        }"#);
        assert!(Template::parse(f.path()).is_err());
    }

    #[test]
    fn parse_succeeds_with_no_placeholders() {
        let f = write_temp(r#"{"field": "static"}"#);
        assert!(Template::parse(f.path()).is_ok());
    }
}
