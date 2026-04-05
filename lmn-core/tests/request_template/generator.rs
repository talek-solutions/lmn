use lmn_core::request_template::{
    definition::{FloatDef, FloatStrategy, ObjectDef, TemplateDef},
    generator::GeneratorContext,
};
use std::collections::HashMap;

fn float_exact(v: f64) -> TemplateDef {
    TemplateDef::Float(FloatDef {
        strategy: FloatStrategy::Exact(v),
        decimals: 0,
    })
}

#[test]
fn generate_def_generates_fresh_float() {
    let ctx = GeneratorContext::new(HashMap::new());
    let def = float_exact(99.0);
    let val = ctx.generate_def(&def, &mut rand::rng());
    assert!(val.is_number());
}

#[test]
fn generate_def_produces_float() {
    let ctx = GeneratorContext::new(HashMap::new());
    let def = float_exact(5.0);
    let val = ctx.generate_def(&def, &mut rand::rng());
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
    let val = ctx.generate_def(&def, &mut rand::rng());
    assert!(val["amount"].is_number());
}
