pub mod definition;
pub mod error;
pub mod generator;
mod generators;
pub mod renderer;
mod validators;

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;
use tracing::instrument;

pub use error::TemplateError;
use generator::GeneratorContext;

const METADATA_KEY: &str = "_lmn_metadata_templates";
pub(crate) const ENV_PLACEHOLDER_PREFIX: &str = "ENV:";

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
    Some(PlaceholderRef {
        name: name.to_string(),
        once,
    })
}

// ── ENV placeholder helpers ───────────────────────────────────────────────────

/// Walks the body `Value` tree and collects the names of all placeholders
/// whose name starts with `"ENV:"` (e.g. `"ENV:MY_TOKEN"`). Deduplicated.
fn collect_env_placeholder_names(body: &Value) -> Vec<String> {
    let mut names = Vec::new();
    collect_env(body, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_env(value: &Value, names: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if let Some(ph) = parse_placeholder(s)
                && ph.name.starts_with(ENV_PLACEHOLDER_PREFIX)
            {
                names.push(ph.name);
            }
        }
        Value::Object(map) => map.values().for_each(|v| collect_env(v, names)),
        Value::Array(arr) => arr.iter().for_each(|v| collect_env(v, names)),
        _ => {}
    }
}

/// For each name like `"ENV:MY_TOKEN"`, reads the env var after the `ENV:` prefix.
/// Returns `Err(TemplateError::MissingEnvVar)` if any variable is not set.
/// Returns `Err(TemplateError::InvalidEnvVarName)` if the var name portion is empty.
fn resolve_env_vars(names: &[String]) -> Result<HashMap<String, Value>, TemplateError> {
    let mut map = HashMap::new();
    for name in names {
        let var_name = &name[ENV_PLACEHOLDER_PREFIX.len()..];
        if var_name.is_empty() {
            return Err(TemplateError::InvalidEnvVarName(name.to_string()));
        }
        match std::env::var(var_name) {
            Ok(val) => {
                map.insert(name.clone(), Value::String(val));
            }
            Err(_) => return Err(TemplateError::MissingEnvVar(var_name.to_string())),
        }
    }
    Ok(map)
}

// ── Template ──────────────────────────────────────────────────────────────────

pub struct Template {
    body: Value,
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
        let mut rng = rand::rng();

        let once_values: HashMap<String, Value> = renderer::collect_once_placeholder_names(&body)
            .into_iter()
            .map(|name| {
                let val = ctx.generate_by_name(&name, &mut rng);
                (name, val)
            })
            .collect();

        // Resolve ENV: placeholders — read from environment at startup (fail-closed)
        let env_names = collect_env_placeholder_names(&body);
        let env_values = resolve_env_vars(&env_names)?;

        // Merge env values into once_values
        let mut all_once_values = once_values;
        all_once_values.extend(env_values);

        Ok(Template {
            body,
            context: ctx.with_once_values(all_once_values),
        })
    }

    /// Pre-generates `n` request bodies, each with independently rendered placeholders.
    /// `:once` placeholders share the same value across all `n` bodies.
    #[instrument(name = "lmn.template.render", skip(self), fields(n))]
    pub fn pre_generate(&self, n: usize) -> Vec<String> {
        let mut rng = rand::rng();
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
        let mut rng = rand::rng();
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

    // 9. generate_one returns valid JSON string
    #[test]
    fn generate_one_returns_valid_json() {
        let f = write_temp(r#"{"field": "static", "value": 42}"#);
        let template = Template::parse(f.path()).unwrap();
        let result = template.generate_one();
        // Must parse as valid JSON
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
        // Two calls should both produce valid JSON (no shared mutable state issues)
        let f = write_temp(r#"{"field": "static"}"#);
        let template = Template::parse(f.path()).unwrap();
        let a = template.generate_one();
        let b = template.generate_one();
        assert!(serde_json::from_str::<serde_json::Value>(&a).is_ok());
        assert!(serde_json::from_str::<serde_json::Value>(&b).is_ok());
    }

    #[test]
    fn parse_env_placeholder_resolved_from_env() {
        unsafe { std::env::set_var("LUMEN_TEST_TOKEN", "secret123") };
        let f = write_temp(r#"{"token": "{{ENV:LUMEN_TEST_TOKEN}}"}"#);
        let template = Template::parse(f.path()).unwrap();
        let result = template.generate_one();
        assert!(
            result.contains("secret123"),
            "expected 'secret123' in output, got: {result}"
        );
    }

    #[test]
    fn parse_env_placeholder_missing_var_is_error() {
        // Ensure the var is definitely not set
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
        // Template with ENV: placeholder and empty _lmn_metadata_templates should succeed
        unsafe { std::env::set_var("LUMEN_TEST_TOKEN", "anyvalue") };
        let f =
            write_temp(r#"{"token": "{{ENV:LUMEN_TEST_TOKEN}}", "_lmn_metadata_templates": {}}"#);
        assert!(Template::parse(f.path()).is_ok());
    }

    #[test]
    fn parse_env_placeholder_with_once_suffix_resolves_correctly() {
        // {{ENV:LUMEN_TEST_ONCE_TOKEN:once}} — the :once suffix is parsed away by
        // parse_placeholder, leaving name = "ENV:LUMEN_TEST_ONCE_TOKEN". The collect_once
        // path must skip it (since there is no generator def), and the ENV resolution path
        // must still resolve it correctly.
        unsafe { std::env::set_var("LUMEN_TEST_ONCE_TOKEN", "once_secret_value") };
        let f = write_temp(r#"{"token": "{{ENV:LUMEN_TEST_ONCE_TOKEN:once}}"}"#);
        let template = Template::parse(f.path()).unwrap();
        let result = template.generate_one();
        assert!(
            result.contains("once_secret_value"),
            "expected 'once_secret_value' in output, got: {result}"
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
