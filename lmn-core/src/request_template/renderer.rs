use std::collections::HashMap;
use std::sync::Arc;

use rand::Rng;
use serde_json::Value;
use tracing::instrument;

use crate::request_template::definition::TemplateDef;
use crate::request_template::error::TemplateError;
use crate::request_template::generator::GeneratorContext;
use crate::request_template::{ENV_PLACEHOLDER_PREFIX, PlaceholderRef, parse_placeholder};

// ── Segment ───────────────────────────────────────────────────────────────────

/// A pre-compiled unit of a template body.
enum Segment {
    /// Pre-serialized JSON fragment written verbatim into the output buffer at render time.
    Static(Arc<str>),
    /// Placeholder name to resolve at render time.
    Placeholder(String),
}

// ── CompiledTemplate ──────────────────────────────────────────────────────────

/// A template body compiled into a flat list of [`Segment`]s.
///
/// Created once at parse time via [`CompiledTemplate::compile`]; rendered
/// on every request via [`CompiledTemplate::render`] without revisiting the
/// original `Value` tree.
pub struct CompiledTemplate {
    segments: Vec<Segment>,
}

impl CompiledTemplate {
    /// Walks the `Value` tree depth-first and compiles it into a flat list of
    /// [`Segment`]s. Done once at parse time so render time only iterates
    /// the segment list.
    pub fn compile(body: &Value) -> Result<Self, TemplateError> {
        let mut segments = Vec::new();
        compile_value(body, &mut segments)?;
        Ok(Self { segments })
    }

    /// Iterates the compiled segments and writes static bytes and resolved
    /// placeholder values into a `String` buffer.
    ///
    /// For each placeholder:
    /// - If the name is present in `ctx.resolved` (pre-serialized `:global` or `ENV:`
    ///   value), it is written verbatim.
    /// - Otherwise, a fresh value is generated via `ctx.generate_by_name` and
    ///   serialized inline.
    pub fn render(
        &self,
        ctx: &GeneratorContext,
        rng: &mut impl Rng,
    ) -> Result<String, TemplateError> {
        let mut buf = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Static(s) => buf.push_str(s),
                Segment::Placeholder(name) => {
                    if let Some(precomputed) = ctx.resolved.get(name) {
                        buf.push_str(precomputed);
                    } else {
                        let val = ctx.generate_by_name(name, rng);
                        buf.push_str(
                            &serde_json::to_string(&val).map_err(TemplateError::Serialization)?,
                        );
                    }
                }
            }
        }
        Ok(buf)
    }
}

fn compile_value(value: &Value, out: &mut Vec<Segment>) -> Result<(), TemplateError> {
    match value {
        Value::String(s) => {
            if let Some(ph) = parse_placeholder(s) {
                out.push(Segment::Placeholder(ph.name));
            } else {
                let serialized = serde_json::to_string(s).map_err(TemplateError::Serialization)?;
                out.push(Segment::Static(Arc::from(serialized.as_str())));
            }
        }
        Value::Object(map) => {
            out.push(Segment::Static(Arc::from("{")));
            let mut first = true;
            for (key, val) in map {
                if !first {
                    out.push(Segment::Static(Arc::from(",")));
                }
                first = false;
                let key_json = serde_json::to_string(key).map_err(TemplateError::Serialization)?;
                out.push(Segment::Static(Arc::from(format!("{key_json}:").as_str())));
                compile_value(val, out)?;
            }
            out.push(Segment::Static(Arc::from("}")));
        }
        Value::Array(arr) => {
            out.push(Segment::Static(Arc::from("[")));
            let mut first = true;
            for val in arr {
                if !first {
                    out.push(Segment::Static(Arc::from(",")));
                }
                first = false;
                compile_value(val, out)?;
            }
            out.push(Segment::Static(Arc::from("]")));
        }
        // Number, Bool, Null — serialize as-is
        _ => {
            let serialized = serde_json::to_string(value).map_err(TemplateError::Serialization)?;
            out.push(Segment::Static(Arc::from(serialized.as_str())));
        }
    }
    Ok(())
}

// ── PlaceholderHandler trait ──────────────────────────────────────────────────

/// A strategy for pre-resolving a class of placeholders before any requests fire.
///
/// Implementors declare which placeholders they handle via [`matches`] and
/// compute a map of `name → Arc<str>` (pre-serialized JSON) via [`resolve`].
/// The default [`collect_names`] and [`walk`] methods traverse the body tree
/// using `matches` to find relevant placeholder names.
pub trait PlaceholderHandler {
    /// Returns `true` if this handler is responsible for the given placeholder.
    fn matches(&self, ph: &PlaceholderRef) -> bool;

    /// Walks `body`, collects all matching placeholder names, and resolves them
    /// to pre-serialized JSON values. Called once at template parse time.
    fn resolve(
        &self,
        body: &Value,
        ctx: &GeneratorContext,
    ) -> Result<HashMap<String, Arc<str>>, TemplateError>;

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
                if let Some(ph) = parse_placeholder(s)
                    && self.matches(&ph) {
                        names.push(ph.name);
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
    ) -> Result<HashMap<String, Arc<str>>, TemplateError> {
        let names = self.collect_names(body);
        let mut rng = rand::rng();
        names
            .into_iter()
            .map(|n| {
                let val = ctx.generate_by_name(&n, &mut rng);
                let serialized =
                    serde_json::to_string(&val).map_err(TemplateError::Serialization)?;
                Ok((n, Arc::from(serialized.as_str())))
            })
            .collect()
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
    ) -> Result<HashMap<String, Arc<str>>, TemplateError> {
        let names = self.collect_names(body);
        resolve_env_vars(&names)
    }
}

/// For each name like `"ENV:MY_TOKEN"`, reads the env var after the `ENV:` prefix
/// and pre-serializes its value as a JSON string literal.
/// Returns `Err(TemplateError::MissingEnvVar)` if any variable is not set.
/// Returns `Err(TemplateError::InvalidEnvVarName)` if the var name portion is empty.
fn resolve_env_vars(names: &[String]) -> Result<HashMap<String, Arc<str>>, TemplateError> {
    let mut map = HashMap::new();
    for name in names {
        let var_name = &name[ENV_PLACEHOLDER_PREFIX.len()..];
        if var_name.is_empty() {
            return Err(TemplateError::InvalidEnvVarName(name.to_string()));
        }
        match std::env::var(var_name) {
            Ok(val) => {
                let serialized =
                    serde_json::to_string(&val).map_err(TemplateError::Serialization)?;
                map.insert(name.clone(), Arc::from(serialized.as_str()));
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
/// Unlike [`CompiledTemplate::render`], which operates on pre-compiled segments,
/// this function works directly on a string — useful for resolving placeholders
/// in header values or other non-body string fields.
///
/// Each placeholder is resolved to its display form (i.e. the raw string value,
/// without surrounding JSON quotes). Unknown placeholders are replaced with an
/// empty string.
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
                    // Check pre-resolved values first (`:global` / `ENV:`).
                    // Pre-resolved values are JSON literals — unwrap string quotes
                    // for display in a header value.
                    if let Some(serialized) = ctx.resolved.get(&ph.name) {
                        match serde_json::from_str::<Value>(serialized) {
                            Ok(Value::String(s)) => s,
                            Ok(other) => other.to_string(),
                            Err(_) => serialized.to_string(),
                        }
                    } else {
                        let val = ctx.generate_by_name(&ph.name, rng);
                        match val {
                            Value::String(s) => s,
                            other => other.to_string(),
                        }
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
    fn compile_static_string_emits_json_quoted() {
        let compiled = CompiledTemplate::compile(&serde_json::json!("hello")).unwrap();
        assert_eq!(compiled.segments.len(), 1);
        if let Segment::Static(s) = &compiled.segments[0] {
            assert_eq!(s.as_ref(), "\"hello\"");
        } else {
            panic!("expected Static segment");
        }
    }

    #[test]
    fn compile_placeholder_string_emits_placeholder() {
        let compiled = CompiledTemplate::compile(&serde_json::json!("{{val}}")).unwrap();
        assert_eq!(compiled.segments.len(), 1);
        if let Segment::Placeholder(name) = &compiled.segments[0] {
            assert_eq!(name, "val");
        } else {
            panic!("expected Placeholder segment");
        }
    }

    #[test]
    fn compile_object_emits_braces_and_key() {
        let compiled = CompiledTemplate::compile(&serde_json::json!({ "k": "v" })).unwrap();
        // Expected: Static("{"), Static("\"k\":"), Static("\"v\""), Static("}")
        assert!(compiled.segments.len() >= 3);
    }

    #[test]
    fn compile_empty_object_roundtrips() {
        let compiled = CompiledTemplate::compile(&serde_json::json!({})).unwrap();
        let ctx = GeneratorContext::new(HashMap::new());
        let result = compiled.render(&ctx, &mut rand::rng()).unwrap();
        assert_eq!(result, "{}");
    }

    #[test]
    fn compile_empty_array_roundtrips() {
        let compiled = CompiledTemplate::compile(&serde_json::json!([])).unwrap();
        let ctx = GeneratorContext::new(HashMap::new());
        let result = compiled.render(&ctx, &mut rand::rng()).unwrap();
        assert_eq!(result, "[]");
    }

    #[test]
    fn compile_array_with_placeholder_renders_correctly() {
        let ctx = make_ctx_with_float("val", 1.0);
        let compiled =
            CompiledTemplate::compile(&serde_json::json!(["static", "{{val}}"])).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&compiled.render(&ctx, &mut rand::rng()).unwrap()).unwrap();
        assert_eq!(result[0], serde_json::json!("static"));
        assert!(result[1].is_number());
    }

    #[test]
    fn compile_special_chars_in_string_are_escaped() {
        let compiled =
            CompiledTemplate::compile(&serde_json::json!({"key": "hello \"world\"\nnewline"}))
                .unwrap();
        let ctx = GeneratorContext::new(HashMap::new());
        let output = compiled.render(&ctx, &mut rand::rng()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["key"], serde_json::json!("hello \"world\"\nnewline"));
    }

    #[test]
    fn compile_deeply_nested_renders_correctly() {
        let ctx = make_ctx_with_float("price", 5.0);
        let compiled =
            CompiledTemplate::compile(&serde_json::json!({ "a": { "b": { "c": "{{price}}" } } }))
                .unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&compiled.render(&ctx, &mut rand::rng()).unwrap()).unwrap();
        assert!(result["a"]["b"]["c"].is_number());
    }

    #[test]
    fn render_substitutes_placeholder() {
        let ctx = make_ctx_with_float("val", 42.0);
        let compiled =
            CompiledTemplate::compile(&serde_json::json!({ "field": "{{val}}" })).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&compiled.render(&ctx, &mut rand::rng()).unwrap()).unwrap();
        assert!(result["field"].is_number());
    }

    #[test]
    fn render_leaves_plain_string_unchanged() {
        let ctx = GeneratorContext::new(HashMap::new());
        let compiled = CompiledTemplate::compile(&serde_json::json!({ "field": "plain" })).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&compiled.render(&ctx, &mut rand::rng()).unwrap()).unwrap();
        assert_eq!(result["field"], serde_json::json!("plain"));
    }

    #[test]
    fn render_handles_nested_objects() {
        let ctx = make_ctx_with_float("price", 10.0);
        let compiled =
            CompiledTemplate::compile(&serde_json::json!({ "order": { "price": "{{price}}" } }))
                .unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&compiled.render(&ctx, &mut rand::rng()).unwrap()).unwrap();
        assert!(result["order"]["price"].is_number());
    }

    #[test]
    fn render_uses_preresolved_value() {
        let ctx = GeneratorContext::new(HashMap::new())
            .with_resolved([("x".to_string(), Arc::from("99"))].into_iter().collect());
        let compiled = CompiledTemplate::compile(&serde_json::json!({ "field": "{{x}}" })).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&compiled.render(&ctx, &mut rand::rng()).unwrap()).unwrap();
        assert_eq!(result["field"], serde_json::json!(99));
    }

    #[test]
    fn resolve_string_uses_preresolved_env_value() {
        let ctx = GeneratorContext::new(HashMap::new()).with_resolved(
            [("ENV:TOKEN".to_string(), Arc::from("\"mysecret\""))]
                .into_iter()
                .collect(),
        );
        let result = resolve_string_placeholders("Bearer {{ENV:TOKEN}}", &ctx, &mut rand::rng());
        assert_eq!(result, "Bearer mysecret");
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
