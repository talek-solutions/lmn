use std::collections::HashMap;

use crate::request_template::definition::TemplateDef;
use crate::request_template::error::TemplateError;
use crate::request_template::validators::Validator;

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
    Some(inner.trim_end_matches(":global"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_template::validators::Validator;

    #[test]
    fn validates_valid_composition() {
        let comp = [("amount".to_string(), "{{price}}".to_string())]
            .into_iter()
            .collect();
        assert!(ObjectValidator { composition: comp }.validate("x").is_ok());
    }

    #[test]
    fn rejects_non_placeholder_value() {
        let comp = [("amount".to_string(), "price".to_string())]
            .into_iter()
            .collect();
        assert!(ObjectValidator { composition: comp }.validate("x").is_err());
    }

    #[test]
    fn extract_plain_name_strips_braces() {
        assert_eq!(extract_plain_name("{{name}}"), Some("name"));
    }

    #[test]
    fn extract_plain_name_strips_global_suffix() {
        assert_eq!(extract_plain_name("{{name:global}}"), Some("name"));
    }

    #[test]
    fn extract_plain_name_returns_none_for_plain_string() {
        assert_eq!(extract_plain_name("plain"), None);
    }
}
