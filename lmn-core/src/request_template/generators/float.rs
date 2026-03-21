use rand::Rng;
use serde_json::Value;

use crate::request_template::definition::{FloatDef, FloatStrategy};
use crate::request_template::generators::Generate;

impl Generate for FloatDef {
    fn generate(&self, rng: &mut impl Rng) -> Value {
        let raw = match self.strategy {
            FloatStrategy::Exact(v) => v,
            FloatStrategy::Range { min, max } => rng.gen_range(min..=max),
        };

        let scale = 10f64.powi(self.decimals as i32);
        let rounded = (raw * scale).round() / scale;

        Value::Number(serde_json::Number::from_f64(rounded).unwrap_or(serde_json::Number::from(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_template::generators::Generate;

    #[test]
    fn exact_strategy_returns_exact_value() {
        let def = FloatDef {
            strategy: FloatStrategy::Exact(3.14),
            decimals: 2,
        };
        let val = def.generate(&mut rand::thread_rng()).as_f64().unwrap();
        assert!((val - 3.14).abs() < f64::EPSILON);
    }

    #[test]
    fn range_strategy_stays_within_bounds() {
        let def = FloatDef {
            strategy: FloatStrategy::Range { min: 1.0, max: 2.0 },
            decimals: 2,
        };
        let mut rng = rand::thread_rng();
        for _ in 0..20 {
            let val = def.generate(&mut rng).as_f64().unwrap();
            assert!(val >= 1.0 && val <= 2.0);
        }
    }

    #[test]
    fn applies_decimal_rounding() {
        let def = FloatDef {
            strategy: FloatStrategy::Exact(std::f64::consts::PI),
            decimals: 2,
        };
        let val = def.generate(&mut rand::thread_rng()).as_f64().unwrap();
        assert!((val - 3.14).abs() < 0.001);
    }
}
