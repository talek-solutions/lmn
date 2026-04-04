use std::collections::HashMap;

use rand::Rng;
use serde_json::Value;
use tracing::debug;

use crate::request_template::definition::{ObjectDef, TemplateDef};
use crate::request_template::generators::Generate;

// ── Context ───────────────────────────────────────────────────────────────────

pub struct GeneratorContext {
    pub defs: HashMap<String, TemplateDef>,
    /// Pre-resolved values — includes `:global` placeholder values and `ENV:` values.
    /// When a placeholder name is present here, its value is returned as-is without
    /// generating a fresh one.
    pub resolved: HashMap<String, Value>,
}

impl GeneratorContext {
    pub fn new(defs: HashMap<String, TemplateDef>) -> Self {
        Self {
            defs,
            resolved: HashMap::new(),
        }
    }

    /// Consumes `self` and returns a new context with the given pre-resolved values.
    /// Used to attach `:global` and `ENV:` values resolved at template parse time.
    pub fn with_resolved(mut self, resolved: HashMap<String, Value>) -> Self {
        self.resolved.extend(resolved);
        self
    }

    /// Resolves a placeholder by name, returning a pre-resolved value (`:global` or
    /// `ENV:`) if available, otherwise generating a fresh one.
    pub fn resolve(&self, name: &str, rng: &mut impl Rng) -> Value {
        if let Some(v) = self.resolved.get(name) {
            return v.clone();
        }
        self.generate_by_name(name, rng)
    }

    pub(crate) fn generate_by_name(&self, name: &str, rng: &mut impl Rng) -> Value {
        match self.defs.get(name) {
            Some(def) => self.generate_def(def, rng),
            None => {
                debug!(placeholder = name, "unknown placeholder resolved to null");
                Value::Null
            }
        }
    }

    pub fn generate_def(&self, def: &TemplateDef, rng: &mut impl Rng) -> Value {
        match def {
            TemplateDef::String(d) => d.generate(rng),
            TemplateDef::Float(d) => d.generate(rng),
            TemplateDef::Object(d) => self.generate_object(d, rng),
        }
    }

    fn generate_object(&self, def: &ObjectDef, rng: &mut impl Rng) -> Value {
        let map = def
            .composition
            .iter()
            .map(|(field, ref_name)| (field.clone(), self.resolve(ref_name, rng)))
            .collect();
        Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_template::definition::{FloatDef, FloatStrategy, ObjectDef, TemplateDef};

    fn float_exact(v: f64) -> TemplateDef {
        TemplateDef::Float(FloatDef {
            strategy: FloatStrategy::Exact(v),
            decimals: 0,
        })
    }

    #[test]
    fn generate_by_name_returns_null_for_unknown() {
        let ctx = GeneratorContext::new(HashMap::new());
        let val = ctx.generate_by_name("unknown", &mut rand::rng());
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn generate_by_name_returns_value_for_known() {
        let mut defs = HashMap::new();
        defs.insert("price".to_string(), float_exact(10.0));
        let ctx = GeneratorContext::new(defs);
        let val = ctx.generate_by_name("price", &mut rand::rng());
        assert!(val.is_number());
    }

    #[test]
    fn generate_object_composes_fields() {
        let mut defs = HashMap::new();
        defs.insert("price".to_string(), float_exact(42.0));
        let ctx = GeneratorContext::new(defs);
        let obj = ObjectDef {
            composition: [("amount".to_string(), "price".to_string())]
                .into_iter()
                .collect(),
        };
        let val = ctx.generate_object(&obj, &mut rand::rng());
        assert!(val["amount"].is_number());
    }
}
