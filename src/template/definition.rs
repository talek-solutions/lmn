use std::collections::HashMap;

use serde::Deserialize;

use crate::template::error::TemplateError;

const MAX_STRING_LENGTH: usize = 10_000;

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

#[derive(Deserialize, Default)]
pub struct RawStringDetails {
    pub uppercase_count: Option<usize>,
    pub lowercase_count: Option<usize>,
    pub special_chars: Option<Vec<String>>,
    pub choice: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
pub struct RawFloatDetails {
    pub decimals: Option<u8>,
}

// ── Validated types ───────────────────────────────────────────────────────────

pub enum TemplateDef {
    String(StringDef),
    Float(FloatDef),
    Object(ObjectDef),
}

pub struct StringDef {
    pub strategy: StringStrategy,
}

pub enum StringStrategy {
    Choice(Vec<String>),
    Generated(StringGenConfig),
}

pub struct StringGenConfig {
    pub length: LengthSpec,
    pub uppercase_count: usize,
    pub lowercase_count: usize,
    pub special_chars: Vec<char>,
}

pub enum LengthSpec {
    Exact(usize),
    Range { min: usize, max: usize },
}

pub struct FloatDef {
    pub strategy: FloatStrategy,
    pub decimals: u8,
}

pub enum FloatStrategy {
    Exact(f64),
    Range { min: f64, max: f64 },
}

pub struct ObjectDef {
    /// Maps output field name → referenced placeholder name (without `{{}}`)
    pub composition: HashMap<String, String>,
}

// ── Validation ────────────────────────────────────────────────────────────────

pub fn validate_all(
    raw: HashMap<String, RawTemplateDef>,
) -> Result<HashMap<String, TemplateDef>, TemplateError> {
    raw.into_iter()
        .map(|(name, raw_def)| validate(raw_def, &name).map(|def| (name, def)))
        .collect()
}

fn validate(raw: RawTemplateDef, name: &str) -> Result<TemplateDef, TemplateError> {
    match raw {
        RawTemplateDef::String { exact, min, max, details } => {
            let details = details.unwrap_or_default();

            if let Some(choices) = details.choice {
                if choices.is_empty() {
                    return Err(TemplateError::InvalidConstraint(format!(
                        "'{name}': choice list must not be empty"
                    )));
                }
                return Ok(TemplateDef::String(StringDef {
                    strategy: StringStrategy::Choice(choices),
                }));
            }

            let length = validate_length_spec(exact, min, max, name, MAX_STRING_LENGTH)?;

            let min_len = match &length {
                LengthSpec::Exact(n) => *n,
                LengthSpec::Range { min, .. } => *min,
            };

            let uppercase_count = details.uppercase_count.unwrap_or(0);
            let lowercase_count = details.lowercase_count.unwrap_or(0);

            if uppercase_count + lowercase_count > min_len {
                return Err(TemplateError::InvalidConstraint(format!(
                    "'{name}': uppercase_count ({uppercase_count}) + lowercase_count \
                     ({lowercase_count}) exceeds minimum length ({min_len})"
                )));
            }

            let special_chars = details
                .special_chars
                .unwrap_or_default()
                .into_iter()
                .filter_map(|s| s.chars().next())
                .collect();

            Ok(TemplateDef::String(StringDef {
                strategy: StringStrategy::Generated(StringGenConfig {
                    length,
                    uppercase_count,
                    lowercase_count,
                    special_chars,
                }),
            }))
        }

        RawTemplateDef::Float { exact, min, max, details } => {
            let decimals = details.unwrap_or_default().decimals.unwrap_or(2);

            let strategy = if let Some(v) = exact {
                FloatStrategy::Exact(v)
            } else {
                let min_v = min.ok_or_else(|| {
                    TemplateError::InvalidConstraint(format!(
                        "'{name}': float requires either 'exact' or both 'min' and 'max'"
                    ))
                })?;
                let max_v = max.ok_or_else(|| {
                    TemplateError::InvalidConstraint(format!(
                        "'{name}': float requires either 'exact' or both 'min' and 'max'"
                    ))
                })?;
                if min_v > max_v {
                    return Err(TemplateError::InvalidConstraint(format!(
                        "'{name}': float min ({min_v}) > max ({max_v})"
                    )));
                }
                FloatStrategy::Range { min: min_v, max: max_v }
            };

            Ok(TemplateDef::Float(FloatDef { strategy, decimals }))
        }

        RawTemplateDef::Object { composition } => {
            let composition = composition
                .into_iter()
                .map(|(field, placeholder)| {
                    extract_plain_name(&placeholder)
                        .map(|n| (field, n.to_string()))
                        .ok_or_else(|| {
                            TemplateError::InvalidConstraint(format!(
                                "object composition value '{placeholder}' is not a valid \
                                 placeholder — expected '{{{{name}}}}'"
                            ))
                        })
                })
                .collect::<Result<HashMap<_, _>, _>>()?;

            Ok(TemplateDef::Object(ObjectDef { composition }))
        }
    }
}

fn validate_length_spec(
    exact: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
    name: &str,
    cap: usize,
) -> Result<LengthSpec, TemplateError> {
    if let Some(v) = exact {
        let n = v as usize;
        if n > cap {
            return Err(TemplateError::InvalidConstraint(format!(
                "'{name}': exact length {n} exceeds maximum allowed ({cap})"
            )));
        }
        return Ok(LengthSpec::Exact(n));
    }

    let min_v = min.map(|v| v as usize).unwrap_or(1);
    let max_v = max.map(|v| v as usize).unwrap_or(min_v);

    if min_v > max_v {
        return Err(TemplateError::InvalidConstraint(format!(
            "'{name}': min length ({min_v}) > max length ({max_v})"
        )));
    }
    if max_v > cap {
        return Err(TemplateError::InvalidConstraint(format!(
            "'{name}': max length ({max_v}) exceeds maximum allowed ({cap})"
        )));
    }

    Ok(LengthSpec::Range { min: min_v, max: max_v })
}

// ── Circular reference detection ──────────────────────────────────────────────

pub fn check_circular_refs(defs: &HashMap<String, TemplateDef>) -> Result<(), TemplateError> {
    // Validate all composition references point to known definitions
    for def in defs.values() {
        if let TemplateDef::Object(obj) = def {
            for ref_name in obj.composition.values() {
                if !defs.contains_key(ref_name.as_str()) {
                    return Err(TemplateError::MissingDefinition(ref_name.clone()));
                }
            }
        }
    }

    // DFS to detect cycles
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

/// Strips `{{` / `}}` and optional `:once` suffix to get the plain placeholder name.
pub fn extract_plain_name(s: &str) -> Option<&str> {
    let inner = s.trim().strip_prefix("{{")?.strip_suffix("}}")?;
    Some(inner.trim_end_matches(":once"))
}
