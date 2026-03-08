use rand::Rng;
use rand::seq::SliceRandom;
use serde_json::Value;

use crate::template::definition::{LengthSpec, StringDef, StringGenConfig, StringStrategy};
use crate::template::generators::Generate;

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
