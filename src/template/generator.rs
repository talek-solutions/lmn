use std::collections::HashMap;

use rand::Rng;
use serde_json::Value;

use crate::template::definition::{ObjectDef, TemplateDef};
use crate::template::generators::Generate;

// ── Context ───────────────────────────────────────────────────────────────────

pub struct GeneratorContext {
    pub defs: HashMap<String, TemplateDef>,
    pub once_values: HashMap<String, Value>,
}

impl GeneratorContext {
    pub fn new(defs: HashMap<String, TemplateDef>) -> Self {
        Self { defs, once_values: HashMap::new() }
    }

    pub fn with_once_values(self, once_values: HashMap<String, Value>) -> Self {
        Self { once_values, ..self }
    }

    /// Resolves a placeholder by name, returning a pre-computed `:once` value
    /// if available, otherwise generating a fresh one.
    pub fn resolve(&self, name: &str, rng: &mut impl Rng) -> Value {
        if let Some(v) = self.once_values.get(name) {
            return v.clone();
        }
        self.generate_by_name(name, rng)
    }

    pub(crate) fn generate_by_name(&self, name: &str, rng: &mut impl Rng) -> Value {
        match self.defs.get(name) {
            Some(def) => self.generate_def(def, rng),
            None => Value::Null,
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
