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

impl Default for ResponseStats {
    fn default() -> Self {
        Self::new()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response_template::extractor::{ExtractedValue, ExtractionResult};

    fn empty_result() -> ExtractionResult {
        ExtractionResult {
            values: vec![],
            mismatches: vec![],
        }
    }

    fn mixed_result() -> ExtractionResult {
        ExtractionResult {
            values: vec![
                (
                    "status".to_string(),
                    ExtractedValue::String("ok".to_string()),
                ),
                ("score".to_string(), ExtractedValue::Float(9.5)),
            ],
            mismatches: vec!["missing".to_string()],
        }
    }

    #[test]
    fn empty_result_still_increments_total() {
        let mut stats = ResponseStats::new();
        stats.record(empty_result());
        assert_eq!(stats.total_responses, 1);
        assert!(stats.string_distributions.is_empty());
        assert!(stats.float_fields.is_empty());
        assert!(stats.mismatch_counts.is_empty());
    }

    #[test]
    fn mixed_result_records_all_field_types() {
        let mut stats = ResponseStats::new();
        stats.record(mixed_result());
        assert!(stats.string_distributions.contains_key("status"));
        assert!(stats.float_fields.contains_key("score"));
        assert_eq!(stats.mismatch_counts["missing"], 1);
    }

    #[test]
    fn float_values_accumulate_across_records() {
        let mut stats = ResponseStats::new();
        stats.record(mixed_result());
        stats.record(mixed_result());
        assert_eq!(stats.float_fields["score"].values, vec![9.5, 9.5]);
    }
}
