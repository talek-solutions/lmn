use serde::Deserialize;

use crate::template::definition::TemplateDef;
use crate::template::error::TemplateError;
use crate::template::validators::Validator;

// ── Raw ───────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct RawFloatDetails {
    pub decimals: Option<u8>,
}

// ── Validated ─────────────────────────────────────────────────────────────────

pub struct FloatDef {
    pub strategy: FloatStrategy,
    pub decimals: u8,
}

pub enum FloatStrategy {
    Exact(f64),
    Range { min: f64, max: f64 },
}

// ── Validator ─────────────────────────────────────────────────────────────────

pub struct FloatValidator {
    pub exact: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub details: Option<RawFloatDetails>,
}

impl Validator for FloatValidator {
    fn validate(self, name: &str) -> Result<TemplateDef, TemplateError> {
        let decimals = self.details.unwrap_or_default().decimals.unwrap_or(2);

        let strategy = if let Some(v) = self.exact {
            FloatStrategy::Exact(v)
        } else {
            let min_v = self.min.ok_or_else(|| {
                TemplateError::InvalidConstraint(format!(
                    "'{name}': float requires either 'exact' or both 'min' and 'max'"
                ))
            })?;
            let max_v = self.max.ok_or_else(|| {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::validators::Validator;

    fn v(exact: Option<f64>, min: Option<f64>, max: Option<f64>) -> FloatValidator {
        FloatValidator { exact, min, max, details: None }
    }

    #[test]
    fn validates_exact() {
        assert!(v(Some(1.0), None, None).validate("x").is_ok());
    }

    #[test]
    fn validates_range() {
        assert!(v(None, Some(1.0), Some(5.0)).validate("x").is_ok());
    }

    #[test]
    fn rejects_min_greater_than_max() {
        assert!(v(None, Some(5.0), Some(1.0)).validate("x").is_err());
    }

    #[test]
    fn rejects_missing_min() {
        assert!(v(None, None, Some(5.0)).validate("x").is_err());
    }

    #[test]
    fn rejects_missing_max() {
        assert!(v(None, Some(1.0), None).validate("x").is_err());
    }
}
