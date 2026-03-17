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

const METADATA_KEY: &str = "_lumen_metadata_templates";

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
    #[instrument(name = "lumen.template.parse", fields(path = %path.display()))]
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
    #[instrument(name = "lumen.template.render", skip(self), fields(n))]
    pub fn pre_generate(&self, n: usize) -> Vec<String> {
        let mut rng = rand::thread_rng();
        (0..n)
            .map(|_| {
                let rendered = renderer::render(&self.body, &self.context, &mut rng);
                serde_json::to_string(&rendered).expect("rendered Value is always valid JSON")
            })
            .collect()
    }

    /// Generates a single request body on demand.
    /// Thread-safe: each call creates its own RNG state, so concurrent VU tasks
    /// can call this simultaneously without contention.
    pub fn generate_one(&self) -> String {
        let mut rng = rand::thread_rng();
        let rendered = renderer::render(&self.body, &self.context, &mut rng);
        serde_json::to_string(&rendered).expect("rendered Value is always valid JSON")
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
            "_lumen_metadata_templates": {
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

    // 9. generate_one returns valid JSON string
    #[test]
    fn generate_one_returns_valid_json() {
        let f = write_temp(r#"{"field": "static", "value": 42}"#);
        let template = Template::parse(f.path()).unwrap();
        let result = template.generate_one();
        // Must parse as valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&result)
            .expect("generate_one must return valid JSON");
        assert_eq!(parsed["field"], serde_json::Value::String("static".to_string()));
        assert_eq!(parsed["value"], serde_json::json!(42));
    }

    #[test]
    fn generate_one_is_independent_per_call() {
        // Two calls should both produce valid JSON (no shared mutable state issues)
        let f = write_temp(r#"{"field": "static"}"#);
        let template = Template::parse(f.path()).unwrap();
        let a = template.generate_one();
        let b = template.generate_one();
        assert!(serde_json::from_str::<serde_json::Value>(&a).is_ok());
        assert!(serde_json::from_str::<serde_json::Value>(&b).is_ok());
    }
}
