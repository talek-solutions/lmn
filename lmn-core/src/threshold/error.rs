/// Errors that can occur during threshold parsing and evaluation.
#[derive(Debug)]
pub enum ThresholdError {
    /// Input could not be parsed as valid JSON or YAML.
    ParseError(String),
    /// Input was parseable but failed validation (e.g. out-of-range values).
    ValidationError(String),
}

impl std::fmt::Display for ThresholdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "Threshold parse error: {}", msg),
            Self::ValidationError(msg) => write!(f, "Threshold validation error: {}", msg),
        }
    }
}

impl std::error::Error for ThresholdError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_parse_error() {
        let e = ThresholdError::ParseError("bad json".to_string());
        assert!(e.to_string().contains("bad json"));
    }

    #[test]
    fn display_validation_error() {
        let e = ThresholdError::ValidationError("out of range".to_string());
        assert!(e.to_string().contains("out of range"));
    }
}
