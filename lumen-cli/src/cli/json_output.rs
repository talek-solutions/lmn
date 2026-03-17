use std::io::Write;
use std::path::PathBuf;

/// Where the JSON output should be written.
pub enum JsonDest {
    Stdout,
    /// The path is used directly without canonicalization. Callers must ensure
    /// this value is not derived from untrusted input.
    File(PathBuf),
}

/// Parameters for `write_json_output`.
pub struct WriteJsonOutputParams<'a> {
    /// The serialisable run report produced by `lumen_core::output::RunReport`.
    ///
    /// The concrete type is a stub (`serde_json::Value`) until Dev 1's
    /// `lumen-core/src/output/` module is merged.  Once merged, replace the
    /// type here with `&'a lumen_core::output::RunReport` and remove the
    /// `serde_json::Value` import.
    pub report: &'a serde_json::Value,
    pub dest: JsonDest,
}

/// Serialises `report` to pretty-printed JSON and writes it to `dest`.
///
/// On file write failure the error is returned; the caller is responsible for
/// printing to stderr and exiting with code 1.
pub fn write_json_output(params: WriteJsonOutputParams<'_>) -> Result<(), Box<dyn std::error::Error>> {
    // TODO: replace `serde_json::Value` with `lumen_core::output::RunReport`
    // once Dev 1's output module is merged.  The serialisation call below is
    // already correct — `serde_json::to_string_pretty` works for any `Serialize`.
    let json = serde_json::to_string_pretty(params.report)?;

    match params.dest {
        JsonDest::Stdout => {
            println!("{json}");
        }
        JsonDest::File(path) => {
            let mut file = std::fs::File::create(&path)
                .map_err(|e| format!("failed to create output file '{}': {e}", path.display()))?;
            file.write_all(json.as_bytes())
                .map_err(|e| format!("failed to write output file '{}': {e}", path.display()))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal_report() -> serde_json::Value {
        json!({
            "version": 1,
            "run": { "mode": "fixed", "elapsed_ms": 100.0, "curve_duration_ms": null, "template_generation_ms": null },
            "requests": { "total": 10, "ok": 10, "failed": 0, "error_rate": 0.0, "throughput_rps": 100.0 },
            "latency": {
                "min_ms": 1.0, "p10_ms": 1.0, "p25_ms": 2.0, "p50_ms": 3.0,
                "p75_ms": 4.0, "p90_ms": 5.0, "p95_ms": 6.0, "p99_ms": 7.0,
                "max_ms": 10.0, "avg_ms": 3.5
            },
            "status_codes": { "200": 10 },
            "sampling": {
                "sampled": false,
                "final_sample_rate": 1.0,
                "min_sample_rate": 1.0,
                "reservoir_size": 100000,
                "results_collected": 10
            },
            "response_stats": null,
            "curve_stages": null
        })
    }

    #[test]
    fn write_json_output_to_stdout_succeeds() {
        // Verifies that the function does not error for the Stdout variant.
        // Actual stdout bytes are not captured; the test asserts the Result is Ok.
        let value = minimal_report();
        let result = write_json_output(WriteJsonOutputParams {
            report: &value,
            dest: JsonDest::Stdout,
        });
        assert!(result.is_ok(), "write_json_output(Stdout) returned error: {:?}", result.err());
    }

    #[test]
    fn write_json_output_writes_to_file() {
        let report = minimal_report();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        // Close the tempfile handle so we can overwrite it via File::create.
        drop(tmp);

        let result = write_json_output(WriteJsonOutputParams {
            report: &report,
            dest: JsonDest::File(path.clone()),
        });
        assert!(result.is_ok(), "write_json_output returned error: {:?}", result.err());

        let contents = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["requests"]["total"], 10);
    }

    #[test]
    fn write_json_output_file_invalid_path_returns_error() {
        let report = minimal_report();
        let bad_path = PathBuf::from("/nonexistent_dir_lumen_test/output.json");
        let result = write_json_output(WriteJsonOutputParams {
            report: &report,
            dest: JsonDest::File(bad_path),
        });
        assert!(result.is_err());
    }
}
