use serde::Deserialize;

use crate::request_template::definition::TemplateDef;
use crate::request_template::error::TemplateError;
use crate::request_template::validators::Validator;

const MAX_STRING_LENGTH: usize = 10_000;

// ── Raw ───────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct RawStringDetails {
    pub uppercase_count: Option<usize>,
    pub lowercase_count: Option<usize>,
    pub special_chars: Option<Vec<String>>,
    pub choice: Option<Vec<String>>,
}

// ── Validated ─────────────────────────────────────────────────────────────────

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

// ── Validator ─────────────────────────────────────────────────────────────────

pub struct StringValidator {
    pub exact: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub details: Option<RawStringDetails>,
}

impl Validator for StringValidator {
    fn validate(self, name: &str) -> Result<TemplateDef, TemplateError> {
        let details = self.details.unwrap_or_default();

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

        let length = validate_length_spec(self.exact, self.min, self.max, name)?;

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
}

fn validate_length_spec(
    exact: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
    name: &str,
) -> Result<LengthSpec, TemplateError> {
    if let Some(v) = exact {
        let n = v as usize;
        if n > MAX_STRING_LENGTH {
            return Err(TemplateError::InvalidConstraint(format!(
                "'{name}': exact length {n} exceeds maximum allowed ({MAX_STRING_LENGTH})"
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
    if max_v > MAX_STRING_LENGTH {
        return Err(TemplateError::InvalidConstraint(format!(
            "'{name}': max length ({max_v}) exceeds maximum allowed ({MAX_STRING_LENGTH})"
        )));
    }

    Ok(LengthSpec::Range { min: min_v, max: max_v })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_template::validators::Validator;

    fn v(exact: Option<f64>, min: Option<f64>, max: Option<f64>, details: Option<RawStringDetails>) -> StringValidator {
        StringValidator { exact, min, max, details }
    }

    #[test]
    fn validates_choice_list() {
        let d = RawStringDetails { choice: Some(vec!["a".into(), "b".into()]), ..Default::default() };
        assert!(v(None, None, None, Some(d)).validate("x").is_ok());
    }

    #[test]
    fn rejects_empty_choice_list() {
        let d = RawStringDetails { choice: Some(vec![]), ..Default::default() };
        assert!(v(None, None, None, Some(d)).validate("x").is_err());
    }

    #[test]
    fn validates_exact_length() {
        assert!(v(Some(5.0), None, None, None).validate("x").is_ok());
    }

    #[test]
    fn rejects_exact_exceeds_max_allowed() {
        assert!(v(Some(10_001.0), None, None, None).validate("x").is_err());
    }

    #[test]
    fn validates_range() {
        assert!(v(None, Some(3.0), Some(8.0), None).validate("x").is_ok());
    }

    #[test]
    fn rejects_min_greater_than_max() {
        assert!(v(None, Some(10.0), Some(5.0), None).validate("x").is_err());
    }

    #[test]
    fn rejects_char_counts_exceeding_min_length() {
        let d = RawStringDetails { uppercase_count: Some(5), lowercase_count: Some(5), ..Default::default() };
        assert!(v(Some(3.0), None, None, Some(d)).validate("x").is_err());
    }

    #[test]
    fn validate_length_spec_exact() {
        assert!(matches!(validate_length_spec(Some(5.0), None, None, "x").unwrap(), LengthSpec::Exact(5)));
    }

    #[test]
    fn validate_length_spec_range() {
        assert!(matches!(validate_length_spec(None, Some(2.0), Some(8.0), "x").unwrap(), LengthSpec::Range { min: 2, max: 8 }));
    }
}
