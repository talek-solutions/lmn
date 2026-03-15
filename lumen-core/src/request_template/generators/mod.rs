pub mod float;
pub mod string;

use rand::Rng;
use serde_json::Value;

pub trait Generate {
    fn generate(&self, rng: &mut impl Rng) -> Value;
}
