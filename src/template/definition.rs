use std::collections::HashMap;

use serde::Deserialize;

use crate::template::error::TemplateError;
use crate::template::validators;

// ── Re-exports ────────────────────────────────────────────────────────────────

pub use crate::template::validators::{
    float::{FloatDef, FloatStrategy, RawFloatDetails},
    object::ObjectDef,
    string::{LengthSpec, RawStringDetails, StringDef, StringGenConfig, StringStrategy},
};

// ── Raw (deserialized directly from JSON) ────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RawTemplateDef {
    String {
        exact: Option<f64>,
        min: Option<f64>,
        max: Option<f64>,
        details: Option<RawStringDetails>,
    },
    Float {
        exact: Option<f64>,
        min: Option<f64>,
        max: Option<f64>,
        details: Option<RawFloatDetails>,
    },
    Object {
        composition: HashMap<String, String>,
    },
}

// ── Validated ─────────────────────────────────────────────────────────────────

pub enum TemplateDef {
    String(StringDef),
    Float(FloatDef),
    Object(ObjectDef),
}

// ── Validation ────────────────────────────────────────────────────────────────

pub fn validate_all(
    raw: HashMap<String, RawTemplateDef>,
) -> Result<HashMap<String, TemplateDef>, TemplateError> {
    raw.into_iter()
        .map(|(name, raw_def)| validators::validate(raw_def, &name).map(|def| (name, def)))
        .collect()
}

// ── Circular reference detection ──────────────────────────────────────────────

pub fn check_circular_refs(defs: &HashMap<String, TemplateDef>) -> Result<(), TemplateError> {
    for def in defs.values() {
        if let TemplateDef::Object(obj) = def {
            for ref_name in obj.composition.values() {
                if !defs.contains_key(ref_name.as_str()) {
                    return Err(TemplateError::MissingDefinition(ref_name.clone()));
                }
            }
        }
    }

    for name in defs.keys() {
        let mut visiting: Vec<&str> = Vec::new();
        detect_cycle(name, defs, &mut visiting)?;
    }

    Ok(())
}

fn detect_cycle<'a>(
    name: &'a str,
    defs: &'a HashMap<String, TemplateDef>,
    visiting: &mut Vec<&'a str>,
) -> Result<(), TemplateError> {
    if visiting.contains(&name) {
        let mut cycle: Vec<String> = visiting.iter().map(|s| s.to_string()).collect();
        cycle.push(name.to_string());
        return Err(TemplateError::CircularReference(cycle));
    }

    if let Some(TemplateDef::Object(obj)) = defs.get(name) {
        visiting.push(name);
        for ref_name in obj.composition.values() {
            detect_cycle(ref_name, defs, visiting)?;
        }
        visiting.pop();
    }

    Ok(())
}
