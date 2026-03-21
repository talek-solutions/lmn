use crate::config::error::ConfigError;

// ── SensitiveString ───────────────────────────────────────────────────────────

/// A string wrapper that redacts its value in `Debug` output to prevent
/// accidental logging of secrets.
///
/// Use `Display` (or `.as_str()`) to access the actual value when needed.
#[derive(Clone, PartialEq)]
pub struct SensitiveString(String);

impl SensitiveString {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SensitiveString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl std::fmt::Display for SensitiveString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl serde::Serialize for SensitiveString {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("[REDACTED]")
    }
}

// ── resolve_env_placeholders ──────────────────────────────────────────────────

/// Resolves `${VAR_NAME}` placeholders in `input` by substituting the
/// corresponding environment variable values.
///
/// # Rules
/// - VAR_NAME must contain only uppercase ASCII letters, digits, or underscores
///   (`[A-Z0-9_]`). Any other character inside `${...}` is a validation error.
/// - If a referenced variable is not set in the environment, returns a
///   [`ConfigError::ValidationError`] (fail-closed).
/// - Single-pass only — the substituted values are NOT scanned again for
///   `${...}` patterns (no recursive expansion).
/// - Literal `${` with no closing `}` is left as-is (not an error).
pub fn resolve_env_placeholders(input: &str) -> Result<String, ConfigError> {
    if !input.contains("${") {
        return Ok(input.to_string());
    }

    let mut output = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(open) = remaining.find("${") {
        output.push_str(&remaining[..open]);
        let after_open = &remaining[open + 2..];

        match after_open.find('}') {
            Some(close) => {
                let var_name = &after_open[..close];
                // Validate charset: only A-Z, 0-9, _ allowed
                if var_name.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "env var placeholder '${...}' must not be empty".to_string(),
                    ));
                }
                if !var_name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
                    // Find the first invalid character for a precise error message
                    let bad = var_name.chars().find(|c| !c.is_ascii_uppercase() && !c.is_ascii_digit() && *c != '_').unwrap();
                    return Err(ConfigError::ValidationError(format!(
                        "env var name '{var_name}' contains invalid character '{bad}' — \
                         only uppercase letters, digits, and underscores are allowed"
                    )));
                }
                let value = std::env::var(var_name).map_err(|_| {
                    ConfigError::ValidationError(format!(
                        "environment variable '{var_name}' is not set"
                    ))
                })?;
                output.push_str(&value);
                remaining = &after_open[close + 1..];
            }
            None => {
                // No closing `}` — push `${` literally and continue
                output.push_str("${");
                remaining = after_open;
            }
        }
    }

    output.push_str(remaining);
    Ok(output)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── SensitiveString ───────────────────────────────────────────────────────

    #[test]
    fn sensitive_string_debug_is_redacted() {
        let s = SensitiveString::new("super-secret-token".to_string());
        assert_eq!(format!("{s:?}"), "[REDACTED]");
    }

    #[test]
    fn sensitive_string_display_shows_value() {
        let s = SensitiveString::new("my-actual-value".to_string());
        assert_eq!(format!("{s}"), "my-actual-value");
    }

    #[test]
    fn sensitive_string_as_str_returns_value() {
        let s = SensitiveString::new("hello".to_string());
        assert_eq!(s.as_str(), "hello");
    }

    #[test]
    fn sensitive_string_clone_equals_original() {
        let s = SensitiveString::new("abc".to_string());
        let cloned = s.clone();
        assert_eq!(s, cloned);
    }

    #[test]
    fn sensitive_string_partial_eq() {
        let a = SensitiveString::new("x".to_string());
        let b = SensitiveString::new("x".to_string());
        let c = SensitiveString::new("y".to_string());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // ── resolve_env_placeholders ──────────────────────────────────────────────

    #[test]
    fn resolves_single_placeholder() {
        unsafe { std::env::set_var("LUMEN_TEST_TOKEN", "abc123") };
        let result = resolve_env_placeholders("Bearer ${LUMEN_TEST_TOKEN}").unwrap();
        assert_eq!(result, "Bearer abc123");
    }

    #[test]
    fn resolves_multiple_placeholders() {
        unsafe {
            std::env::set_var("LUMEN_TEST_USER", "alice");
            std::env::set_var("LUMEN_TEST_PASS", "s3cr3t");
        }
        let result = resolve_env_placeholders("${LUMEN_TEST_USER}:${LUMEN_TEST_PASS}").unwrap();
        assert_eq!(result, "alice:s3cr3t");
    }

    #[test]
    fn no_placeholders_returns_input_unchanged() {
        let result = resolve_env_placeholders("plain-value").unwrap();
        assert_eq!(result, "plain-value");
    }

    #[test]
    fn missing_env_var_returns_validation_error() {
        // Ensure the variable is definitely not set
        unsafe { std::env::remove_var("LUMEN_TEST_DEFINITELY_NOT_SET_XYZ") };
        let result = resolve_env_placeholders("Bearer ${LUMEN_TEST_DEFINITELY_NOT_SET_XYZ}");
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("LUMEN_TEST_DEFINITELY_NOT_SET_XYZ"), "error should name the var: {msg}");
    }

    #[test]
    fn invalid_charset_in_var_name_returns_error() {
        let result = resolve_env_placeholders("${lower_case}");
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("invalid character"), "expected charset error, got: {msg}");
    }

    #[test]
    fn empty_placeholder_returns_error() {
        let result = resolve_env_placeholders("${  }");
        // space is invalid char — will trigger charset error
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn empty_braces_returns_error() {
        let result = resolve_env_placeholders("${}");
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("must not be empty"), "expected empty error, got: {msg}");
    }

    #[test]
    fn unclosed_brace_is_kept_literally() {
        let result = resolve_env_placeholders("${NO_CLOSE").unwrap();
        assert_eq!(result, "${NO_CLOSE");
    }

    #[test]
    fn dollar_without_brace_is_kept_literally() {
        let result = resolve_env_placeholders("$VAR").unwrap();
        assert_eq!(result, "$VAR");
    }

    #[test]
    fn no_recursive_expansion() {
        // Set a var whose value itself looks like another placeholder
        unsafe { std::env::set_var("LUMEN_TEST_RECURSIVE", "${LUMEN_TEST_INNER}") };
        // LUMEN_TEST_INNER is NOT set — if recursive expansion happened this would error
        let result = resolve_env_placeholders("${LUMEN_TEST_RECURSIVE}").unwrap();
        assert_eq!(result, "${LUMEN_TEST_INNER}");
    }

    #[test]
    fn mixed_literals_and_placeholders() {
        unsafe { std::env::set_var("LUMEN_TEST_API_KEY", "key-xyz") };
        let result = resolve_env_placeholders("prefix_${LUMEN_TEST_API_KEY}_suffix").unwrap();
        assert_eq!(result, "prefix_key-xyz_suffix");
    }

    #[test]
    fn digits_and_underscores_in_var_name_are_valid() {
        unsafe { std::env::set_var("LUMEN_TEST_A1_B2", "ok") };
        let result = resolve_env_placeholders("${LUMEN_TEST_A1_B2}").unwrap();
        assert_eq!(result, "ok");
    }

    #[test]
    fn resolve_env_placeholders_with_utf8_value() {
        // Env var value contains multi-byte UTF-8; must come back intact with no corruption.
        unsafe { std::env::set_var("LUMEN_TEST_UTF8_VAL", "café") };
        let result = resolve_env_placeholders("prefix_${LUMEN_TEST_UTF8_VAL}_suffix").unwrap();
        assert_eq!(result, "prefix_café_suffix");
    }

    // ── SensitiveString::Serialize ────────────────────────────────────────────

    #[test]
    fn sensitive_string_serializes_as_redacted() {
        let s = SensitiveString::new("super-secret".to_string());
        let serialized = serde_json::to_string(&s).expect("serialization must not fail");
        assert_eq!(serialized, r#""[REDACTED]""#);
    }
}
