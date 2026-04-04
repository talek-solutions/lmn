use std::collections::HashMap;

use rand::Rng;
use serde_json::Value;
use tracing::instrument;

use crate::request_template::definition::TemplateDef;
use crate::request_template::error::TemplateError;
use crate::request_template::generator::GeneratorContext;
use crate::request_template::{ENV_PLACEHOLDER_PREFIX, parse_placeholder, PlaceholderRef};

// ── PlaceholderHandler trait ──────────────────────────────────────────────────

/// A strategy for pre-resolving a class of placeholders before any requests fire.
///
/// Implementors declare which placeholders they handle via [`matches`] and
/// compute a map of `name → Value` via [`resolve`]. The default
/// [`collect_names`] and [`walk`] methods traverse the body tree using `matches`
/// to find relevant placeholder names.
pub trait PlaceholderHandler {
    /// Returns `true` if this handler is responsible for the given placeholder.
    fn matches(&self, ph: &PlaceholderRef) -> bool;

    /// Walks `body`, collects all matching placeholder names, and resolves them
    /// to their pre-computed values. Called once at template parse time.
    fn resolve(
        &self,
        body: &Value,
        ctx: &GeneratorContext,
    ) -> Result<HashMap<String, Value>, TemplateError>;

    /// Collects all placeholder names in `body` that this handler matches.
    /// Deduplicated and sorted for determinism.
    fn collect_names(&self, body: &Value) -> Vec<String> {
        let mut names = Vec::new();
        self.walk(body, &mut names);
        names.sort();
        names.dedup();
        names
    }

    /// Recursively walks `value`, pushing matching placeholder names into `names`.
    fn walk(&self, value: &Value, names: &mut Vec<String>) {
        match value {
            Value::String(s) => {
                if let Some(ph) = parse_placeholder(s) {
                    if self.matches(&ph) {
                        names.push(ph.name);
                    }
                }
            }
            Value::Object(map) => map.values().for_each(|v| self.walk(v, names)),
            Value::Array(arr) => arr.iter().for_each(|v| self.walk(v, names)),
            _ => {}
        }
    }
}

// ── GlobalPlaceholderHandler ──────────────────────────────────────────────────

/// Handles `:global` placeholders — non-ENV placeholders that carry the
/// `:global` suffix. Their value is generated once at startup and reused
/// across all requests in the run.
#[derive(Debug)]
pub struct GlobalPlaceholderHandler;

impl PlaceholderHandler for GlobalPlaceholderHandler {
    fn matches(&self, ph: &PlaceholderRef) -> bool {
        ph.global && !ph.name.starts_with(ENV_PLACEHOLDER_PREFIX)
    }

    fn resolve(
        &self,
        body: &Value,
        ctx: &GeneratorContext,
    ) -> Result<HashMap<String, Value>, TemplateError> {
        let names = self.collect_names(body);
        let mut rng = rand::rng();
        Ok(names
            .into_iter()
            .map(|n| {
                let val = ctx.generate_by_name(&n, &mut rng);
                (n, val)
            })
            .collect())
    }
}

// ── EnvPlaceholderHandler ─────────────────────────────────────────────────────

/// Handles `ENV:` placeholders — reads named environment variables at template
/// parse time (fail-closed: missing vars are an error).
#[derive(Debug)]
pub struct EnvPlaceholderHandler;

impl PlaceholderHandler for EnvPlaceholderHandler {
    fn matches(&self, ph: &PlaceholderRef) -> bool {
        ph.name.starts_with(ENV_PLACEHOLDER_PREFIX)
    }

    fn resolve(
        &self,
        body: &Value,
        _ctx: &GeneratorContext,
    ) -> Result<HashMap<String, Value>, TemplateError> {
        let names = self.collect_names(body);
        resolve_env_vars(&names)
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

// ── resolve_string_placeholders ───────────────────────────────────────────────

/// Resolves `{{placeholder_name}}` patterns in a raw string by calling the
/// corresponding generator from `ctx`.
///
/// Unlike [`render`], which operates on a `serde_json::Value` tree, this
/// function works directly on a string — useful for resolving placeholders
/// in header values or other non-body string fields.
///
/// Each placeholder is resolved to its `Display` form (i.e. the JSON
/// serialisation of the generated value, without surrounding quotes for
/// strings). Unknown placeholders are replaced with an empty string.
///
/// Single-pass only — the generated values are NOT scanned again for
/// `{{...}}` patterns.
pub fn resolve_string_placeholders(
    input: &str,
    ctx: &GeneratorContext,
    rng: &mut impl Rng,
) -> String {
    // Fast path: no `{{` in input — return a clone without any scanning.
    if !input.contains("{{") {
        return input.to_string();
    }

    let mut output = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(open) = remaining.find("{{") {
        // Push everything before the opening `{{`
        output.push_str(&remaining[..open]);
        let after_open = &remaining[open + 2..];

        match after_open.find("}}") {
            Some(close_offset) => {
                let placeholder_body = &after_open[..close_offset];
                // Reuse the existing parse_placeholder logic by wrapping in `{{...}}`
                let wrapped = format!("{{{{{placeholder_body}}}}}");
                let resolved_value = if let Some(ph) = parse_placeholder(&wrapped) {
                    let val = ctx.resolve(&ph.name, rng);
                    // Convert Value to a display string:
                    // - JSON strings are unwrapped (no surrounding quotes)
                    // - other types use their JSON representation
                    match val {
                        Value::String(s) => s,
                        other => other.to_string(),
                    }
                } else {
                    // Empty or malformed placeholder — emit empty string
                    String::new()
                };
                output.push_str(&resolved_value);
                remaining = &after_open[close_offset + 2..];
            }
            None => {
                // No closing `}}` — copy `{{` literally and continue
                output.push_str("{{");
                remaining = after_open;
            }
        }
    }

    // Append any trailing content after the last placeholder
    output.push_str(remaining);
    output
}

/// Recursively walks the template `Value` tree, substituting every placeholder
/// string with a freshly generated value from the context.
pub fn render(template: &Value, ctx: &GeneratorContext, rng: &mut impl Rng) -> Value {
    match template {
        Value::String(s) => {
            if let Some(ph) = parse_placeholder(s) {
                ctx.resolve(&ph.name, rng)
            } else {
                template.clone()
            }
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), render(v, ctx, rng)))
                .collect(),
        ),
        Value::Array(arr) => Value::Array(arr.iter().map(|v| render(v, ctx, rng)).collect()),
        _ => template.clone(),
    }
}

/// Validates that every `{{name}}` placeholder in the body has a corresponding
/// definition. Returns an error naming the first unknown placeholder found.
#[instrument(name = "lmn.template.validate_placeholders", skip(body, defs), fields(def_count = defs.len()))]
pub fn validate_placeholders(
    body: &Value,
    defs: &HashMap<String, TemplateDef>,
) -> Result<(), TemplateError> {
    walk_strings(body, &mut |s| {
        if let Some(ph) = parse_placeholder(s) {
            if ph.name.starts_with(ENV_PLACEHOLDER_PREFIX) {
                return Ok(()); // built-in, no def required
            }
            if !defs.contains_key(&ph.name) {
                return Err(TemplateError::UnknownPlaceholder(ph.name));
            }
        }
        Ok(())
    })
}

fn walk_strings<F>(value: &Value, f: &mut F) -> Result<(), TemplateError>
where
    F: FnMut(&str) -> Result<(), TemplateError>,
{
    match value {
        Value::String(s) => f(s),
        Value::Object(map) => {
            for v in map.values() {
                walk_strings(v, f)?;
            }
            Ok(())
        }
        Value::Array(arr) => {
            for v in arr {
                walk_strings(v, f)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_template::definition::{FloatDef, FloatStrategy, TemplateDef};
    use crate::request_template::generator::GeneratorContext;

    fn make_ctx_with_float(name: &str, value: f64) -> GeneratorContext {
        let mut defs = HashMap::new();
        defs.insert(
            name.to_string(),
            TemplateDef::Float(FloatDef {
                strategy: FloatStrategy::Exact(value),
                decimals: 2,
            }),
        );
        GeneratorContext::new(defs)
    }

    fn make_ctx_with_choice(name: &str, choices: Vec<String>) -> GeneratorContext {
        use crate::request_template::definition::{StringDef, StringStrategy};
        let mut defs = HashMap::new();
        defs.insert(
            name.to_string(),
            TemplateDef::String(StringDef {
                strategy: StringStrategy::Choice(choices),
            }),
        );
        GeneratorContext::new(defs)
    }

    #[test]
    fn no_placeholder_returns_input_unchanged() {
        let ctx = GeneratorContext::new(HashMap::new());
        let result = resolve_string_placeholders("plain-header-value", &ctx, &mut rand::rng());
        assert_eq!(result, "plain-header-value");
    }

    #[test]
    fn resolves_choice_placeholder_without_quotes() {
        let ctx = make_ctx_with_choice("user_id", vec!["alice".to_string()]);
        let result = resolve_string_placeholders("user-{{user_id}}", &ctx, &mut rand::rng());
        assert_eq!(result, "user-alice");
    }

    #[test]
    fn resolves_float_placeholder() {
        let ctx = make_ctx_with_float("amount", 9.99);
        let result = resolve_string_placeholders("val={{amount}}", &ctx, &mut rand::rng());
        assert_eq!(result, "val=9.99");
    }

    #[test]
    fn resolves_multiple_placeholders_in_string() {
        use crate::request_template::definition::{StringDef, StringStrategy};
        let mut defs = HashMap::new();
        defs.insert(
            "a".to_string(),
            TemplateDef::String(StringDef {
                strategy: StringStrategy::Choice(vec!["foo".to_string()]),
            }),
        );
        defs.insert(
            "b".to_string(),
            TemplateDef::String(StringDef {
                strategy: StringStrategy::Choice(vec!["bar".to_string()]),
            }),
        );
        let ctx = GeneratorContext::new(defs);
        let result = resolve_string_placeholders("{{a}}-{{b}}", &ctx, &mut rand::rng());
        assert_eq!(result, "foo-bar");
    }

    #[test]
    fn unknown_placeholder_resolves_to_null_string() {
        // GeneratorContext::resolve for unknown names returns Value::Null
        // Value::Null.to_string() via serde_json is "null"
        let ctx = GeneratorContext::new(HashMap::new());
        let result =
            resolve_string_placeholders("prefix-{{unknown}}-suffix", &ctx, &mut rand::rng());
        assert_eq!(result, "prefix-null-suffix");
    }

    #[test]
    fn unclosed_braces_preserved_literally() {
        let ctx = GeneratorContext::new(HashMap::new());
        let result = resolve_string_placeholders("{{unclosed", &ctx, &mut rand::rng());
        assert_eq!(result, "{{unclosed");
    }

    #[test]
    fn validate_placeholders_skips_env_prefixed_names() {
        // A body containing {{ENV:MY_VAR}} should pass validation even with no defs,
        // because ENV: placeholders are built-in and need no TemplateDef entry.
        let body = Value::Object({
            let mut m = serde_json::Map::new();
            m.insert(
                "token".to_string(),
                Value::String("{{ENV:MY_VAR}}".to_string()),
            );
            m
        });
        let defs = HashMap::new();
        assert!(validate_placeholders(&body, &defs).is_ok());
    }

    #[test]
    fn global_handler_finds_global_placeholders() {
        use serde_json::json;
        let body = json!({ "a": "{{x:global}}", "b": "{{y}}", "c": "{{x:global}}" });
        let handler = GlobalPlaceholderHandler;
        let names = handler.collect_names(&body);
        assert_eq!(names, vec!["x"]);
    }

    #[test]
    fn global_handler_returns_empty_when_none() {
        use serde_json::json;
        let body = json!({ "a": "{{x}}", "b": "plain" });
        let handler = GlobalPlaceholderHandler;
        assert!(handler.collect_names(&body).is_empty());
    }
}
