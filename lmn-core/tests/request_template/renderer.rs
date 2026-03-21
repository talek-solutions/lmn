use lmn_core::request_template::{
    definition::{FloatDef, FloatStrategy, TemplateDef},
    generator::GeneratorContext,
    renderer::{collect_once_placeholder_names, render, validate_placeholders},
};
use serde_json::json;
use std::collections::HashMap;

fn ctx_with_float(name: &str, value: f64) -> GeneratorContext {
    let mut defs = HashMap::new();
    defs.insert(
        name.to_string(),
        TemplateDef::Float(FloatDef {
            strategy: FloatStrategy::Exact(value),
            decimals: 0,
        }),
    );
    GeneratorContext::new(defs)
}

#[test]
fn render_substitutes_placeholder_string() {
    let ctx = ctx_with_float("val", 42.0);
    let template = json!({ "field": "{{val}}" });
    let result = render(&template, &ctx, &mut rand::rng());
    assert!(result["field"].is_number());
}

#[test]
fn render_leaves_plain_string_unchanged() {
    let ctx = GeneratorContext::new(HashMap::new());
    let template = json!({ "field": "plain" });
    let result = render(&template, &ctx, &mut rand::rng());
    assert_eq!(result["field"], json!("plain"));
}

#[test]
fn render_handles_nested_objects() {
    let ctx = ctx_with_float("price", 10.0);
    let template = json!({ "order": { "price": "{{price}}" } });
    let result = render(&template, &ctx, &mut rand::rng());
    assert!(result["order"]["price"].is_number());
}

#[test]
fn validate_placeholders_ok_when_all_defined() {
    let ctx = ctx_with_float("x", 1.0);
    let body = json!({ "a": "{{x}}" });
    assert!(validate_placeholders(&body, &ctx.defs).is_ok());
}

#[test]
fn validate_placeholders_err_on_unknown_placeholder() {
    let body = json!({ "a": "{{missing}}" });
    assert!(validate_placeholders(&body, &HashMap::new()).is_err());
}

#[test]
fn collect_once_finds_once_placeholders() {
    let body = json!({ "a": "{{x:once}}", "b": "{{y}}", "c": "{{x:once}}" });
    let names = collect_once_placeholder_names(&body);
    assert_eq!(names, vec!["x"]);
}

#[test]
fn collect_once_returns_empty_when_none() {
    let body = json!({ "a": "{{x}}", "b": "plain" });
    assert!(collect_once_placeholder_names(&body).is_empty());
}
