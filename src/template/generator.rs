use std::collections::HashMap;

use rand::Rng;
use rand::seq::SliceRandom;
use serde_json::Value;

use crate::template::definition::{
    FloatDef, FloatStrategy, LengthSpec, ObjectDef, StringDef, StringGenConfig, StringStrategy,
    TemplateDef,
};

// ── Trait ─────────────────────────────────────────────────────────────────────

pub trait Generate {
    fn generate(&self, rng: &mut impl Rng) -> Value;
}

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

// ── String generation ─────────────────────────────────────────────────────────

impl Generate for StringDef {
    fn generate(&self, rng: &mut impl Rng) -> Value {
        match &self.strategy {
            StringStrategy::Choice(choices) => {
                let idx = rng.gen_range(0..choices.len());
                Value::String(choices[idx].clone())
            }
            StringStrategy::Generated(config) => Value::String(generate_string(config, rng)),
        }
    }
}

fn generate_string(config: &StringGenConfig, rng: &mut impl Rng) -> String {
    let length = match config.length {
        LengthSpec::Exact(n) => n,
        LengthSpec::Range { min, max } => rng.gen_range(min..=max),
    };

    let mut chars: Vec<char> = Vec::with_capacity(length);

    for _ in 0..config.uppercase_count {
        chars.push(rng.gen_range(b'A'..=b'Z') as char);
    }
    for _ in 0..config.lowercase_count {
        chars.push(rng.gen_range(b'a'..=b'z') as char);
    }

    let remaining = length.saturating_sub(config.uppercase_count + config.lowercase_count);
    for _ in 0..remaining {
        if !config.special_chars.is_empty() {
            let idx = rng.gen_range(0..config.special_chars.len());
            chars.push(config.special_chars[idx]);
        } else {
            const ALPHANUM: &[u8] =
                b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
            chars.push(ALPHANUM[rng.gen_range(0..ALPHANUM.len())] as char);
        }
    }

    chars.shuffle(rng);
    chars.into_iter().collect()
}

// ── Float generation ──────────────────────────────────────────────────────────

impl Generate for FloatDef {
    fn generate(&self, rng: &mut impl Rng) -> Value {
        let raw = match self.strategy {
            FloatStrategy::Exact(v) => v,
            FloatStrategy::Range { min, max } => rng.gen_range(min..=max),
        };

        let scale = 10f64.powi(self.decimals as i32);
        let rounded = (raw * scale).round() / scale;

        Value::Number(
            serde_json::Number::from_f64(rounded).unwrap_or(serde_json::Number::from(0)),
        )
    }
}
