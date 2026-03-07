use std::collections::HashMap;

use rand::Rng;
use serde_json::Value;

use crate::template::definition::TemplateDef;
use crate::template::error::TemplateError;
use crate::template::generator::GeneratorContext;
use crate::template::parse_placeholder;

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
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), render(v, ctx, rng))).collect())
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| render(v, ctx, rng)).collect()),
        _ => template.clone(),
    }
}

/// Validates that every `{{name}}` placeholder in the body has a corresponding
/// definition. Returns an error naming the first unknown placeholder found.
pub fn validate_placeholders(
    body: &Value,
    defs: &HashMap<String, TemplateDef>,
) -> Result<(), TemplateError> {
    walk_strings(body, &mut |s| {
        if let Some(ph) = parse_placeholder(s) {
            if !defs.contains_key(&ph.name) {
                return Err(TemplateError::UnknownPlaceholder(ph.name));
            }
        }
        Ok(())
    })
}

/// Collects the names of all `:once` placeholders in the body (deduplicated).
pub fn collect_once_placeholder_names(body: &Value) -> Vec<String> {
    let mut names = Vec::new();
    collect_once(body, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_once(value: &Value, names: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if let Some(ph) = parse_placeholder(s) {
                if ph.once {
                    names.push(ph.name);
                }
            }
        }
        Value::Object(map) => map.values().for_each(|v| collect_once(v, names)),
        Value::Array(arr) => arr.iter().for_each(|v| collect_once(v, names)),
        _ => {}
    }
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
