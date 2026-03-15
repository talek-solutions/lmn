use rand::Rng;
use rand::seq::SliceRandom;
use serde_json::Value;

use crate::request_template::definition::{LengthSpec, StringDef, StringGenConfig, StringStrategy};
use crate::request_template::generators::Generate;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_template::generators::Generate;

    #[test]
    fn choice_strategy_returns_one_of_choices() {
        let choices = vec!["foo".to_string(), "bar".to_string()];
        let def = StringDef { strategy: StringStrategy::Choice(choices.clone()) };
        let val = def.generate(&mut rand::thread_rng());
        assert!(choices.contains(&val.as_str().unwrap().to_string()));
    }

    #[test]
    fn generated_strategy_exact_length() {
        let def = StringDef {
            strategy: StringStrategy::Generated(StringGenConfig {
                length: LengthSpec::Exact(10),
                uppercase_count: 2,
                lowercase_count: 3,
                special_chars: vec![],
            }),
        };
        let val = def.generate(&mut rand::thread_rng());
        assert_eq!(val.as_str().unwrap().len(), 10);
    }

    #[test]
    fn generated_strategy_range_length() {
        let def = StringDef {
            strategy: StringStrategy::Generated(StringGenConfig {
                length: LengthSpec::Range { min: 5, max: 10 },
                uppercase_count: 0,
                lowercase_count: 0,
                special_chars: vec![],
            }),
        };
        let mut rng = rand::thread_rng();
        for _ in 0..20 {
            let len = def.generate(&mut rng).as_str().unwrap().len();
            assert!(len >= 5 && len <= 10);
        }
    }
}
