use rand::Rng;
use serde_json::Value;

use crate::template::definition::{FloatDef, FloatStrategy};
use crate::template::generators::Generate;

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
