use std::collections::HashMap;
use loadtest_core::request_template::definition::{check_circular_refs, FloatDef, FloatStrategy, ObjectDef, TemplateDef, validate_all};
use serde_json::json;

fn float_def() -> TemplateDef {
    TemplateDef::Float(FloatDef { strategy: FloatStrategy::Exact(1.0), decimals: 0 })
}

fn object_def(refs: &[(&str, &str)]) -> TemplateDef {
    TemplateDef::Object(ObjectDef {
        composition: refs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    })
}

#[test]
fn validate_all_accepts_valid_float_def() {
    let raw: HashMap<String, _> = serde_json::from_value(json!({
        "price": { "type": "float", "exact": 10.0, "details": { "decimals": 2 } }
    })).unwrap();
    assert!(validate_all(raw).is_ok());
}

#[test]
fn validate_all_accepts_valid_string_def() {
    let raw: HashMap<String, _> = serde_json::from_value(json!({
        "name": { "type": "string", "exact": 5, "details": {} }
    })).unwrap();
    assert!(validate_all(raw).is_ok());
}

#[test]
fn validate_all_rejects_invalid_float() {
    let raw: HashMap<String, _> = serde_json::from_value(json!({
        "price": { "type": "float", "min": 10.0 }
    })).unwrap();
    assert!(validate_all(raw).is_err());
}

#[test]
fn check_circular_refs_ok_for_no_objects() {
    let mut defs = HashMap::new();
    defs.insert("x".to_string(), float_def());
    assert!(check_circular_refs(&defs).is_ok());
}

#[test]
fn check_circular_refs_detects_cycle() {
    let mut defs = HashMap::new();
    defs.insert("a".to_string(), object_def(&[("x", "b")]));
    defs.insert("b".to_string(), object_def(&[("y", "a")]));
    assert!(check_circular_refs(&defs).is_err());
}

#[test]
fn check_circular_refs_detects_missing_ref() {
    let mut defs = HashMap::new();
    defs.insert("a".to_string(), object_def(&[("x", "nonexistent")]));
    assert!(check_circular_refs(&defs).is_err());
}

#[test]
fn check_circular_refs_ok_for_valid_composition() {
    let mut defs = HashMap::new();
    defs.insert("a".to_string(), object_def(&[("x", "b")]));
    defs.insert("b".to_string(), float_def());
    assert!(check_circular_refs(&defs).is_ok());
}
