//! Capture state for step chaining: extract values from HTTP responses and
//! inject them into subsequent step requests.

use std::collections::HashMap;

use serde_json::Value;

// ── CaptureDefinition ────────────────────────────────────────────────────────

/// A single capture rule: extract a JSON path from the response body and store
/// the result under `alias` in the per-iteration `CaptureState`.
#[derive(Debug, Clone)]
pub struct CaptureDefinition {
    pub alias: String,
    pub path: Vec<String>,
}

// ── CaptureState ─────────────────────────────────────────────────────────────

/// Per-iteration mutable capture state. Created (or cleared) at the start of
/// each iteration. No `Arc`, no `Mutex`, no cross-iteration leakage.
#[derive(Debug, Default)]
pub struct CaptureState {
    values: HashMap<String, String>,
}

impl CaptureState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.values.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }
}

// ── parse_json_path ──────────────────────────────────────────────────────────

/// Parses a `$.`-prefixed JSON path into a `Vec<String>` of key segments.
///
/// `"$.data.access_token"` → `["data", "access_token"]`.
/// Only object key traversal is supported (no array indexing).
pub fn parse_json_path(path: &str) -> Result<Vec<String>, String> {
    let rest = path
        .strip_prefix("$.")
        .ok_or_else(|| format!("capture path must start with '$.' — got '{path}'"))?;

    if rest.is_empty() {
        return Err(format!("capture path is empty after '$.' — got '{path}'"));
    }

    let segments: Vec<String> = rest.split('.').map(|s| s.to_string()).collect();

    if segments.iter().any(|s| s.is_empty()) {
        return Err(format!(
            "capture path contains empty segment — got '{path}'"
        ));
    }

    Ok(segments)
}

// ── value_to_string ──────────────────────────────────────────────────────────

/// Converts a `serde_json::Value` to a `String` for capture storage.
///
/// - `String(s)` → `s` (no wrapping quotes)
/// - `Number(n)` → `n.to_string()`
/// - `Bool(b)` → `b.to_string()`
/// - `Null` → `None` (capture not inserted)
/// - `Object` / `Array` → compact JSON via `serde_json::to_string()`
pub fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => serde_json::to_string(other).ok(),
    }
}

// ── inject_captures ──────────────────────────────────────────────────────────

/// Replaces all `{{capture.KEY}}` patterns in `text` with values from `state`.
///
/// Returns `Err` if a referenced key is missing from the state (the request
/// would contain unresolved references and should not be sent).
pub fn inject_captures(text: &str, state: &CaptureState) -> Result<String, String> {
    let marker = "{{capture.";
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find(marker) {
        result.push_str(&rest[..start]);
        let after_marker = &rest[start + marker.len()..];

        let end = after_marker
            .find("}}")
            .ok_or_else(|| format!("unterminated capture placeholder in: {text}"))?;

        let key = &after_marker[..end];

        let value = state.get(key).ok_or_else(|| {
            format!("capture key '{key}' not found in state — preceding step may have failed")
        })?;

        result.push_str(value);
        rest = &after_marker[end + 2..];
    }

    result.push_str(rest);
    Ok(result)
}

// ── inject_captures_into_headers ─────────────────────────────────────────────

/// Applies capture injection to header **values** only (keys are untouched).
pub fn inject_captures_into_headers(
    headers: &[(String, String)],
    state: &CaptureState,
) -> Result<Vec<(String, String)>, String> {
    headers
        .iter()
        .map(|(name, value)| {
            let injected = inject_captures(value, state)?;
            Ok((name.clone(), injected))
        })
        .collect()
}

// ── scan_capture_refs ────────────────────────────────────────────────────────

/// Scans `text` for `{{capture.KEY}}` references and returns the keys.
///
/// Returns `Err` if an unterminated `{{capture.` is found (missing `}}`).
/// This catches config typos at startup rather than at runtime.
pub fn scan_capture_refs(text: &str) -> Result<Vec<String>, String> {
    let marker = "{{capture.";
    let mut refs = Vec::new();
    let mut rest = text;

    while let Some(start) = rest.find(marker) {
        let after_marker = &rest[start + marker.len()..];
        if let Some(end) = after_marker.find("}}") {
            refs.push(after_marker[..end].to_string());
            rest = &after_marker[end + 2..];
        } else {
            return Err(format!(
                "unterminated capture placeholder: '{{{{capture.{}…'",
                &after_marker[..after_marker.len().min(20)]
            ));
        }
    }

    Ok(refs)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── parse_json_path ──────────────────────────────────────────────────────

    #[test]
    fn parse_json_path_simple() {
        let path = parse_json_path("$.data.access_token").unwrap();
        assert_eq!(path, vec!["data", "access_token"]);
    }

    #[test]
    fn parse_json_path_single_segment() {
        let path = parse_json_path("$.token").unwrap();
        assert_eq!(path, vec!["token"]);
    }

    #[test]
    fn parse_json_path_deep() {
        let path = parse_json_path("$.a.b.c.d").unwrap();
        assert_eq!(path, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn parse_json_path_no_prefix() {
        assert!(parse_json_path("data.token").is_err());
    }

    #[test]
    fn parse_json_path_empty_after_prefix() {
        assert!(parse_json_path("$.").is_err());
    }

    #[test]
    fn parse_json_path_empty_segment() {
        assert!(parse_json_path("$.a..b").is_err());
    }

    // ── value_to_string ──────────────────────────────────────────────────────

    #[test]
    fn value_to_string_string() {
        assert_eq!(value_to_string(&json!("hello")), Some("hello".to_string()));
    }

    #[test]
    fn value_to_string_number() {
        assert_eq!(value_to_string(&json!(42)), Some("42".to_string()));
    }

    #[test]
    fn value_to_string_float() {
        assert_eq!(value_to_string(&json!(3.14)), Some("3.14".to_string()));
    }

    #[test]
    fn value_to_string_bool() {
        assert_eq!(value_to_string(&json!(true)), Some("true".to_string()));
    }

    #[test]
    fn value_to_string_null() {
        assert_eq!(value_to_string(&json!(null)), None);
    }

    #[test]
    fn value_to_string_object() {
        let val = json!({"a": 1});
        let result = value_to_string(&val).unwrap();
        assert!(result.contains("\"a\""));
        assert!(result.contains("1"));
    }

    #[test]
    fn value_to_string_array() {
        let val = json!([1, 2, 3]);
        let result = value_to_string(&val).unwrap();
        assert_eq!(result, "[1,2,3]");
    }

    // ── inject_captures ──────────────────────────────────────────────────────

    #[test]
    fn inject_captures_single_replacement() {
        let mut state = CaptureState::new();
        state.insert("token".to_string(), "abc123".to_string());
        let result = inject_captures("Bearer {{capture.token}}", &state).unwrap();
        assert_eq!(result, "Bearer abc123");
    }

    #[test]
    fn inject_captures_multiple_replacements() {
        let mut state = CaptureState::new();
        state.insert("token".to_string(), "tok".to_string());
        state.insert("user_id".to_string(), "42".to_string());
        let result = inject_captures("{{capture.token}} for {{capture.user_id}}", &state).unwrap();
        assert_eq!(result, "tok for 42");
    }

    #[test]
    fn inject_captures_no_placeholders() {
        let state = CaptureState::new();
        let result = inject_captures("no captures here", &state).unwrap();
        assert_eq!(result, "no captures here");
    }

    #[test]
    fn inject_captures_missing_key_returns_err() {
        let state = CaptureState::new();
        let result = inject_captures("{{capture.missing}}", &state);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing"));
    }

    #[test]
    fn inject_captures_unterminated_returns_err() {
        let state = CaptureState::new();
        let result = inject_captures("{{capture.broken", &state);
        assert!(result.is_err());
    }

    // ── inject_captures_into_headers ─────────────────────────────────────────

    #[test]
    fn inject_captures_into_headers_replaces_values() {
        let mut state = CaptureState::new();
        state.insert("token".to_string(), "secret".to_string());
        let headers = vec![
            (
                "Authorization".to_string(),
                "Bearer {{capture.token}}".to_string(),
            ),
            ("X-Static".to_string(), "no-capture".to_string()),
        ];
        let result = inject_captures_into_headers(&headers, &state).unwrap();
        assert_eq!(result[0].1, "Bearer secret");
        assert_eq!(result[1].1, "no-capture");
    }

    #[test]
    fn inject_captures_into_headers_missing_key_returns_err() {
        let state = CaptureState::new();
        let headers = vec![("Auth".to_string(), "{{capture.nope}}".to_string())];
        assert!(inject_captures_into_headers(&headers, &state).is_err());
    }

    // ── scan_capture_refs ────────────────────────────────────────────────────

    #[test]
    fn scan_capture_refs_extracts_keys() {
        let refs = scan_capture_refs("{{capture.token}} and {{capture.user_id}}").unwrap();
        assert_eq!(refs, vec!["token", "user_id"]);
    }

    #[test]
    fn scan_capture_refs_no_captures() {
        let refs = scan_capture_refs("no captures here").unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn scan_capture_refs_unterminated_is_error() {
        let err = scan_capture_refs("{{capture.ok}} then {{capture.broken").unwrap_err();
        assert!(err.contains("unterminated"), "{err}");
    }

    // ── CaptureState ─────────────────────────────────────────────────────────

    #[test]
    fn capture_state_clear() {
        let mut state = CaptureState::new();
        state.insert("a".to_string(), "1".to_string());
        assert!(state.get("a").is_some());
        state.clear();
        assert!(state.get("a").is_none());
    }
}
