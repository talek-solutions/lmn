use loadtest_core::response_template::field::collect_tracked_fields;
use serde_json::json;

#[test]
fn collects_string_field() {
    let val = json!({ "status": "{{STRING}}" });
    let fields = collect_tracked_fields(&val, &[]).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].path, vec!["status"]);
}

#[test]
fn collects_float_field() {
    let val = json!({ "score": "{{FLOAT}}" });
    let fields = collect_tracked_fields(&val, &[]).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].path, vec!["score"]);
}

#[test]
fn ignores_non_placeholder_strings() {
    let val = json!({ "status": "ok" });
    assert!(collect_tracked_fields(&val, &[]).unwrap().is_empty());
}

#[test]
fn collects_nested_fields() {
    let val = json!({ "error": { "code": "{{STRING}}" } });
    let fields = collect_tracked_fields(&val, &[]).unwrap();
    assert_eq!(fields[0].path, vec!["error", "code"]);
}

#[test]
fn errors_on_unknown_placeholder_type() {
    let val = json!({ "x": "{{UNKNOWN}}" });
    assert!(collect_tracked_fields(&val, &[]).is_err());
}

#[test]
fn collects_multiple_fields() {
    let val = json!({ "a": "{{STRING}}", "b": "{{FLOAT}}" });
    let fields = collect_tracked_fields(&val, &[]).unwrap();
    assert_eq!(fields.len(), 2);
}
