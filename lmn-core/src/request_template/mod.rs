pub mod definition;
pub mod error;
pub mod generator;
mod generators;
pub mod renderer;
mod validators;

use std::path::Path;

use serde_json::Value;
use tracing::instrument;

pub use error::TemplateError;
use generator::GeneratorContext;
use renderer::{
    CompiledTemplate, EnvPlaceholderHandler, GlobalPlaceholderHandler, PlaceholderHandler,
};

const METADATA_KEY: &str = "_lmn_metadata_templates";
pub(crate) const ENV_PLACEHOLDER_PREFIX: &str = "ENV:";

// ── Placeholder parsing ───────────────────────────────────────────────────────

pub struct PlaceholderRef {
    pub name: String,
    pub global: bool,
}

/// Parses `{{name}}` or `{{name:global}}` from a string.
/// Returns `None` if the string is not a placeholder.
pub fn parse_placeholder(s: &str) -> Option<PlaceholderRef> {
    let inner = s.trim().strip_prefix("{{")?.strip_suffix("}}")?;
    if inner.is_empty() {
        return None;
    }
    let (name, global) = match inner.strip_suffix(":global") {
        Some(n) => (n, true),
        None => (inner, false),
    };
    if name.is_empty() {
        return None;
    }
    Some(PlaceholderRef {
        name: name.to_string(),
        global,
    })
}

// ── Template ──────────────────────────────────────────────────────────────────

pub struct Template {
    compiled: CompiledTemplate,
    context: GeneratorContext,
}

impl Template {
    /// Reads, parses, and fully validates a template file.
    /// Fails fast on any invalid configuration before any requests are made.
    #[instrument(name = "lmn.template.parse", fields(path = %path.display()))]
    pub fn parse(path: &Path) -> Result<Self, TemplateError> {
        let content = std::fs::read_to_string(path)?;
        let mut root: serde_json::Map<String, Value> = serde_json::from_str(&content)?;

        // Extract metadata — it must not appear in request bodies
        let metadata = root
            .remove(METADATA_KEY)
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        let raw_defs: std::collections::HashMap<String, definition::RawTemplateDef> =
            serde_json::from_value(metadata)?;
        let defs = definition::validate_all(raw_defs)?;

        let body = Value::Object(root);

        // All body placeholders must have definitions
        renderer::validate_placeholders(&body, &defs)?;

        // Object compositions must not form cycles
        definition::check_circular_refs(&defs)?;

        let ctx = GeneratorContext::new(defs);

        // Pre-resolve :global placeholders — same value reused across all requests.
        let global_resolved = GlobalPlaceholderHandler.resolve(&body, &ctx)?;

        // Resolve ENV: placeholders — read from environment at startup (fail-closed).
        let env_resolved = EnvPlaceholderHandler.resolve(&body, &ctx)?;

        // Merge all pre-resolved values and compile the body into segments.
        let mut all_resolved = global_resolved;
        all_resolved.extend(env_resolved);

        let compiled = CompiledTemplate::compile(&body)?;
        // body is dropped here — no longer needed after compile.

        Ok(Template {
            compiled,
            context: ctx.with_resolved(all_resolved),
        })
    }

    /// Generates a single request body on demand.
    /// Thread-safe: each call creates its own RNG state, so concurrent VU tasks
    /// can call this simultaneously without contention.
    #[instrument(name = "lmn.template.generate_one", skip(self))]
    pub fn generate_one(&self) -> Result<String, TemplateError> {
        let mut rng = rand::rng();
        self.compiled.render(&self.context, &mut rng)
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
        let f = write_temp(
            r#"{
            "field": "{{a}}",
            "_lmn_metadata_templates": {
                "a": { "type": "object", "composition": { "x": "{{b}}" } },
                "b": { "type": "object", "composition": { "y": "{{a}}" } }
            }
        }"#,
        );
        assert!(Template::parse(f.path()).is_err());
    }

    #[test]
    fn parse_succeeds_with_no_placeholders() {
        let f = write_temp(r#"{"field": "static"}"#);
        assert!(Template::parse(f.path()).is_ok());
    }

    #[test]
    fn generate_one_returns_valid_json() {
        let f = write_temp(r#"{"field": "static", "value": 42}"#);
        let template = Template::parse(f.path()).unwrap();
        let result = template.generate_one().unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("generate_one must return valid JSON");
        assert_eq!(
            parsed["field"],
            serde_json::Value::String("static".to_string())
        );
        assert_eq!(parsed["value"], serde_json::json!(42));
    }

    #[test]
    fn generate_one_is_independent_per_call() {
        let f = write_temp(r#"{"field": "static"}"#);
        let template = Template::parse(f.path()).unwrap();
        let a = template.generate_one().unwrap();
        let b = template.generate_one().unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&a).is_ok());
        assert!(serde_json::from_str::<serde_json::Value>(&b).is_ok());
    }

    #[test]
    fn parse_env_placeholder_resolved_from_env() {
        unsafe { std::env::set_var("LUMEN_TEST_TOKEN", "secret123") };
        let f = write_temp(r#"{"token": "{{ENV:LUMEN_TEST_TOKEN}}"}"#);
        let template = Template::parse(f.path()).unwrap();
        let result = template.generate_one().unwrap();
        assert!(
            result.contains("secret123"),
            "expected 'secret123' in output, got: {result}"
        );
    }

    #[test]
    fn parse_env_placeholder_missing_var_is_error() {
        unsafe { std::env::remove_var("LUMEN_NONEXISTENT_12345") };
        let f = write_temp(r#"{"token": "{{ENV:LUMEN_NONEXISTENT_12345}}"}"#);
        let result = Template::parse(f.path());
        assert!(
            result.is_err(),
            "expected parse to fail for missing env var"
        );
        assert!(
            matches!(result.err(), Some(TemplateError::MissingEnvVar(_))),
            "expected MissingEnvVar error variant"
        );
    }

    #[test]
    fn parse_env_placeholder_no_def_required() {
        unsafe { std::env::set_var("LUMEN_TEST_TOKEN", "anyvalue") };
        let f =
            write_temp(r#"{"token": "{{ENV:LUMEN_TEST_TOKEN}}", "_lmn_metadata_templates": {}}"#);
        assert!(Template::parse(f.path()).is_ok());
    }

    #[test]
    fn parse_env_placeholder_with_global_suffix_resolves_correctly() {
        // {{ENV:LUMEN_TEST_GLOBAL_TOKEN:global}} — the :global suffix is parsed away by
        // parse_placeholder, leaving name = "ENV:LUMEN_TEST_GLOBAL_TOKEN". The global
        // handler skips it (ENV: prefix), and the ENV resolution path resolves it correctly.
        unsafe { std::env::set_var("LUMEN_TEST_GLOBAL_TOKEN", "global_secret_value") };
        let f = write_temp(r#"{"token": "{{ENV:LUMEN_TEST_GLOBAL_TOKEN:global}}"}"#);
        let template = Template::parse(f.path()).unwrap();
        let result = template.generate_one().unwrap();
        assert!(
            result.contains("global_secret_value"),
            "expected 'global_secret_value' in output, got: {result}"
        );
    }

    #[test]
    fn parse_env_placeholder_empty_var_name_is_error() {
        // {{ENV:}} has an empty variable name and must produce InvalidEnvVarName
        let f = write_temp(r#"{"token": "{{ENV:}}"}"#);
        let result = Template::parse(f.path());
        assert!(
            result.is_err(),
            "expected parse to fail for empty ENV var name"
        );
        assert!(
            matches!(result.err(), Some(TemplateError::InvalidEnvVarName(_))),
            "expected InvalidEnvVarName error variant"
        );
    }
}
