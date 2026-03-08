use std::collections::HashMap;

use crate::template::definition::TemplateDef;
use crate::template::error::TemplateError;
use crate::template::validators::Validator;

// ── Validated ─────────────────────────────────────────────────────────────────

pub struct ObjectDef {
    /// Maps output field name → referenced placeholder name (without `{{}}`)
    pub composition: HashMap<String, String>,
}

// ── Validator ─────────────────────────────────────────────────────────────────

pub struct ObjectValidator {
    pub composition: HashMap<String, String>,
}

impl Validator for ObjectValidator {
    fn validate(self, _name: &str) -> Result<TemplateDef, TemplateError> {
        let composition = self
            .composition
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

fn extract_plain_name(s: &str) -> Option<&str> {
    let inner = s.trim().strip_prefix("{{")?.strip_suffix("}}")?;
    Some(inner.trim_end_matches(":once"))
}
