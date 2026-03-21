use lmn_core::response_template::{
    extractor::{ExtractedValue, ExtractionResult},
    stats::ResponseStats,
};

fn string_result(path: &str, value: &str) -> ExtractionResult {
    ExtractionResult {
        values: vec![(path.to_string(), ExtractedValue::String(value.to_string()))],
        mismatches: vec![],
    }
}

fn float_result(path: &str, value: f64) -> ExtractionResult {
    ExtractionResult {
        values: vec![(path.to_string(), ExtractedValue::Float(value))],
        mismatches: vec![],
    }
}

fn mismatch_result(path: &str) -> ExtractionResult {
    ExtractionResult {
        values: vec![],
        mismatches: vec![path.to_string()],
    }
}

#[test]
fn increments_total_responses() {
    let mut stats = ResponseStats::new();
    stats.record(string_result("status", "ok"));
    stats.record(string_result("status", "ok"));
    assert_eq!(stats.total_responses, 2);
}

#[test]
fn builds_string_distribution() {
    let mut stats = ResponseStats::new();
    stats.record(string_result("status", "ok"));
    stats.record(string_result("status", "ok"));
    stats.record(string_result("status", "err"));
    let dist = &stats.string_distributions["status"];
    assert_eq!(dist["ok"], 2);
    assert_eq!(dist["err"], 1);
}

#[test]
fn accumulates_float_values() {
    let mut stats = ResponseStats::new();
    stats.record(float_result("score", 1.0));
    stats.record(float_result("score", 2.0));
    assert_eq!(stats.float_fields["score"].values.len(), 2);
}

#[test]
fn counts_mismatches() {
    let mut stats = ResponseStats::new();
    stats.record(mismatch_result("field"));
    stats.record(mismatch_result("field"));
    assert_eq!(stats.mismatch_counts["field"], 2);
}
