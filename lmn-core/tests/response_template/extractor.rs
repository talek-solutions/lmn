use lmn_core::response_template::{
    extractor::{ExtractedValue, extract},
    field::{ResponseFieldType, TrackedField},
};
use serde_json::json;

fn field(path: &[&str], field_type: ResponseFieldType) -> TrackedField {
    TrackedField {
        path: path.iter().map(|s| s.to_string()).collect(),
        field_type,
    }
}

#[test]
fn extracts_string_field() {
    let body = json!({ "status": "ok" });
    let result = extract(&body, &[field(&["status"], ResponseFieldType::String)]);
    assert_eq!(result.values.len(), 1);
    assert!(result.mismatches.is_empty());
    assert!(
        matches!(&result.values[0], (k, ExtractedValue::String(v)) if k == "status" && v == "ok")
    );
}

#[test]
fn extracts_float_field() {
    let body = json!({ "amount": 3.15 });
    let result = extract(&body, &[field(&["amount"], ResponseFieldType::Float)]);
    assert_eq!(result.values.len(), 1);
    assert!(
        matches!(&result.values[0], (k, ExtractedValue::Float(v)) if k == "amount" && (*v - 3.15).abs() < f64::EPSILON)
    );
}

#[test]
fn extracts_nested_field() {
    let body = json!({ "error": { "code": "NOT_FOUND" } });
    let result = extract(
        &body,
        &[field(&["error", "code"], ResponseFieldType::String)],
    );
    assert!(
        matches!(&result.values[0], (k, ExtractedValue::String(v)) if k == "error.code" && v == "NOT_FOUND")
    );
}

#[test]
fn records_mismatch_for_missing_field() {
    let body = json!({ "other": "value" });
    let result = extract(&body, &[field(&["status"], ResponseFieldType::String)]);
    assert!(result.values.is_empty());
    assert_eq!(result.mismatches, vec!["status"]);
}

#[test]
fn records_mismatch_on_wrong_type() {
    let body = json!({ "status": 42 });
    let result = extract(&body, &[field(&["status"], ResponseFieldType::String)]);
    assert!(result.values.is_empty());
    assert_eq!(result.mismatches, vec!["status"]);
}

#[test]
fn handles_multiple_fields_mixed_results() {
    let body = json!({ "status": "ok", "score": 9.5 });
    let fields = vec![
        field(&["status"], ResponseFieldType::String),
        field(&["score"], ResponseFieldType::Float),
        field(&["missing"], ResponseFieldType::String),
    ];
    let result = extract(&body, &fields);
    assert_eq!(result.values.len(), 2);
    assert_eq!(result.mismatches, vec!["missing"]);
}
