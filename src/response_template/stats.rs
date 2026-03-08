use std::collections::HashMap;

use crate::response_template::extractor::{ExtractedValue, ExtractionResult};

pub struct FloatFieldStats {
    pub values: Vec<f64>,
}

pub struct ResponseStats {
    pub string_distributions: HashMap<String, HashMap<String, usize>>,
    pub float_fields: HashMap<String, FloatFieldStats>,
    pub mismatch_counts: HashMap<String, usize>,
    pub total_responses: usize,
}

impl ResponseStats {
    pub fn new() -> Self {
        Self {
            string_distributions: HashMap::new(),
            float_fields: HashMap::new(),
            mismatch_counts: HashMap::new(),
            total_responses: 0,
        }
    }

    pub fn record(&mut self, result: ExtractionResult) {
        self.total_responses += 1;

        for (path, value) in result.values {
            match value {
                ExtractedValue::String(s) => {
                    *self
                        .string_distributions
                        .entry(path)
                        .or_default()
                        .entry(s)
                        .or_insert(0) += 1;
                }
                ExtractedValue::Float(f) => {
                    self.float_fields
                        .entry(path)
                        .or_insert_with(|| FloatFieldStats { values: Vec::new() })
                        .values
                        .push(f);
                }
            }
        }

        for path in result.mismatches {
            *self.mismatch_counts.entry(path).or_insert(0) += 1;
        }
    }
}
