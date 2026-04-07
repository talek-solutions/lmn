use std::collections::HashMap;

use crate::histogram::{
    CategoricalHistogram, CategoricalHistogramParams, NumericHistogram, NumericHistogramParams,
};
use crate::response_template::extractor::{ExtractedValue, ExtractionResult};

const DEFAULT_CATEGORICAL_MAX_BUCKETS: usize = 256;
const DEFAULT_NUMERIC_MAX_SAMPLES: usize = 10_000;

// ── ResponseStats ─────────────────────────────────────────────────────────────

pub struct ResponseStats {
    pub string_fields: HashMap<String, CategoricalHistogram>,
    pub float_fields: HashMap<String, NumericHistogram>,
    pub mismatch_counts: HashMap<String, u64>,
    pub total_responses: u64,
}

impl Default for ResponseStats {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseStats {
    pub fn new() -> Self {
        Self {
            string_fields: HashMap::new(),
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
                    self.string_fields
                        .entry(path)
                        .or_insert_with(|| {
                            CategoricalHistogram::new(CategoricalHistogramParams {
                                max_buckets: DEFAULT_CATEGORICAL_MAX_BUCKETS,
                            })
                        })
                        .record(&s);
                }
                ExtractedValue::Float(f) => {
                    self.float_fields
                        .entry(path)
                        .or_insert_with(|| {
                            NumericHistogram::new(NumericHistogramParams {
                                max_samples: DEFAULT_NUMERIC_MAX_SAMPLES,
                            })
                        })
                        .record(f);
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
        assert!(stats.string_fields.is_empty());
        assert!(stats.float_fields.is_empty());
        assert!(stats.mismatch_counts.is_empty());
    }

    #[test]
    fn mixed_result_records_all_field_types() {
        let mut stats = ResponseStats::new();
        stats.record(mixed_result());
        assert!(stats.string_fields.contains_key("status"));
        assert!(stats.float_fields.contains_key("score"));
        assert_eq!(stats.mismatch_counts["missing"], 1);
    }

    #[test]
    fn float_values_accumulate_across_records() {
        let mut stats = ResponseStats::new();
        stats.record(mixed_result());
        stats.record(mixed_result());
        // Two records of score=9.5 should be tracked in NumericHistogram
        let hist = stats.float_fields.get("score").expect("score field");
        assert_eq!(hist.total_seen(), 2);
    }
}
