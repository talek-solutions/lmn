use lmn_core::request_template::{
    definition::{FloatDef, FloatStrategy, ObjectDef, TemplateDef},
    generator::GeneratorContext,
};
use serde_json::Value;
use std::collections::HashMap;

fn float_exact(v: f64) -> TemplateDef {
    TemplateDef::Float(FloatDef {
        strategy: FloatStrategy::Exact(v),
        decimals: 0,
    })
}

#[test]
fn resolve_returns_once_value_when_set() {
    let ctx = GeneratorContext::new(HashMap::new()).with_once_values(
        [("x".to_string(), Value::String("fixed".to_string()))]
            .into_iter()
            .collect(),
    );
    let val = ctx.resolve("x", &mut rand::thread_rng());
    assert_eq!(val, Value::String("fixed".to_string()));
}

#[test]
fn resolve_generates_fresh_when_no_once_value() {
    let mut defs = HashMap::new();
    defs.insert("price".to_string(), float_exact(99.0));
    let ctx = GeneratorContext::new(defs);
    let val = ctx.resolve("price", &mut rand::thread_rng());
    assert!(val.is_number());
}

#[test]
fn generate_def_produces_float() {
    let ctx = GeneratorContext::new(HashMap::new());
    let def = float_exact(5.0);
    let val = ctx.generate_def(&def, &mut rand::thread_rng());
    assert_eq!(val.as_f64().unwrap() as i64, 5);
}

#[test]
fn generate_def_produces_object_with_composed_fields() {
    let mut defs = HashMap::new();
    defs.insert("price".to_string(), float_exact(10.0));
    let ctx = GeneratorContext::new(defs);
    let def = TemplateDef::Object(ObjectDef {
        composition: [("amount".to_string(), "price".to_string())]
            .into_iter()
            .collect(),
    });
    let val = ctx.generate_def(&def, &mut rand::thread_rng());
    assert!(val["amount"].is_number());
}
