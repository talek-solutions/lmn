use std::str::FromStr;
use std::time::Duration;

use serde::Deserialize;

// ── Duration string parsing ───────────────────────────────────────────────────

/// Parses human-readable duration strings: "30s", "2m", "1m30s".
pub fn parse_duration_str(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("duration string is empty".to_string());
    }

    let mut remaining = s;
    let mut total_secs: u64 = 0;
    let mut parsed_any = false;

    // Parse optional minutes component
    if let Some(m_pos) = remaining.find('m') {
        // Ensure only digits before 'm'
        let minutes_str = &remaining[..m_pos];
        if minutes_str.is_empty() {
            return Err(format!("invalid duration string: '{s}'"));
        }
        let minutes: u64 = minutes_str
            .parse()
            .map_err(|_| format!("invalid minutes in duration: '{s}'"))?;
        total_secs += minutes * 60;
        remaining = &remaining[m_pos + 1..];
        parsed_any = true;
    }

    // Parse optional seconds component
    if let Some(s_pos) = remaining.find('s') {
        let secs_str = &remaining[..s_pos];
        if secs_str.is_empty() {
            return Err(format!("invalid duration string: '{s}'"));
        }
        let secs: u64 = secs_str
            .parse()
            .map_err(|_| format!("invalid seconds in duration: '{s}'"))?;
        total_secs += secs;
        remaining = &remaining[s_pos + 1..];
        parsed_any = true;
    }

    if !parsed_any || !remaining.is_empty() {
        return Err(format!("invalid duration string: '{s}'"));
    }

    Ok(Duration::from_secs(total_secs))
}

fn deserialize_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    parse_duration_str(&s).map_err(serde::de::Error::custom)
}

// ── RampType ─────────────────────────────────────────────────────────────────

/// Determines how VU count changes within a stage.
/// `Linear` interpolates smoothly from the previous VU count to `target_vus`.
/// `Step` jumps immediately to `target_vus` at the start of the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RampType {
    #[default]
    Linear,
    Step,
}

// ── Stage ─────────────────────────────────────────────────────────────────────

/// A single stage of a load curve, describing a duration and a target VU count.
#[derive(Debug, Clone, Deserialize)]
pub struct Stage {
    #[serde(deserialize_with = "deserialize_duration")]
    pub duration: Duration,
    pub target_vus: u32,
    #[serde(default)]
    pub ramp: RampType,
}

// ── LoadCurve ─────────────────────────────────────────────────────────────────

/// A sequence of stages that defines how virtual users (VUs) are scaled over time.
#[derive(Debug, Clone, Deserialize)]
pub struct LoadCurve {
    pub stages: Vec<Stage>,
}

impl LoadCurve {
    /// Returns the total duration of all stages combined.
    pub fn total_duration(&self) -> Duration {
        self.stages.iter().map(|s| s.duration).sum()
    }

    /// Returns the target VU count at a given elapsed time into the curve.
    ///
    /// The curve implicitly starts from 0 VUs. Each stage ramps from the
    /// previous stage's `target_vus` (or 0 for the first stage) to its own
    /// `target_vus`, following the stage's `RampType`.
    ///
    /// Returns 0 if `elapsed >= total_duration()` (curve complete).
    pub fn target_vus_at(&self, elapsed: Duration) -> u32 {
        if self.stages.is_empty() {
            return 0;
        }

        let mut stage_start = Duration::ZERO;
        let mut prev_vus: u32 = 0;

        for stage in &self.stages {
            let stage_end = stage_start + stage.duration;

            if elapsed < stage_end {
                // elapsed falls within this stage
                let progress = if stage.duration.is_zero() {
                    1.0
                } else {
                    (elapsed - stage_start).as_secs_f64() / stage.duration.as_secs_f64()
                };

                return match stage.ramp {
                    RampType::Step => stage.target_vus,
                    RampType::Linear => {
                        let from = prev_vus as f64;
                        let to = stage.target_vus as f64;
                        (from + (to - from) * progress).round() as u32
                    }
                };
            }

            prev_vus = stage.target_vus;
            stage_start = stage_end;
        }

        // elapsed >= total_duration: curve is complete, ramp down to 0
        0
    }
}

pub const MAX_VUS: u32 = 10_000;
pub const MAX_STAGES: usize = 1_000;

impl LoadCurve {
    /// Validates the curve against hard limits.
    ///
    /// Returns `Err` if the curve has no stages, exceeds `MAX_STAGES`,
    /// or any stage's `target_vus` exceeds `MAX_VUS`.
    pub fn validate(&self) -> Result<(), String> {
        if self.stages.is_empty() {
            return Err("load curve must have at least one stage".to_string());
        }
        if self.stages.len() > MAX_STAGES {
            return Err(format!(
                "load curve has {} stages, maximum is {MAX_STAGES}",
                self.stages.len()
            ));
        }
        for (i, stage) in self.stages.iter().enumerate() {
            if stage.target_vus > MAX_VUS {
                return Err(format!(
                    "stage {i}: target_vus {} exceeds maximum {MAX_VUS}",
                    stage.target_vus
                ));
            }
        }
        Ok(())
    }
}

// ── TryFrom<ExecutionConfig> for LoadCurve ────────────────────────────────────

impl TryFrom<crate::config::ExecutionConfig> for LoadCurve {
    type Error = String;

    fn try_from(cfg: crate::config::ExecutionConfig) -> Result<Self, Self::Error> {
        let stages = cfg
            .stages
            .ok_or("execution.stages is required for curve mode")?;
        let curve = LoadCurve { stages };
        curve.validate()?;
        Ok(curve)
    }
}

impl FromStr for LoadCurve {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_curve(stages: Vec<(u64, u32, RampType)>) -> LoadCurve {
        LoadCurve {
            stages: stages
                .into_iter()
                .map(|(secs, vus, ramp)| Stage {
                    duration: Duration::from_secs(secs),
                    target_vus: vus,
                    ramp,
                })
                .collect(),
        }
    }

    // 1. Linear interpolation mid-stage
    #[test]
    fn target_vus_at_linear_interpolation_mid_stage() {
        // Stage: 0 -> 100 VUs over 100s. At 50s we expect ~50 VUs.
        let curve = make_curve(vec![(100, 100, RampType::Linear)]);
        let vus = curve.target_vus_at(Duration::from_secs(50));
        assert_eq!(vus, 50);
    }

    // 2. Step jump at stage boundary
    #[test]
    fn target_vus_at_step_jumps_immediately() {
        // Stage: Step to 80 VUs over 60s. Any elapsed within stage should give 80.
        let curve = make_curve(vec![(60, 80, RampType::Step)]);
        // At t=0 within a step stage, should be 80 immediately
        let vus = curve.target_vus_at(Duration::from_secs(1));
        assert_eq!(vus, 80);
        // Also at t=30
        let vus = curve.target_vus_at(Duration::from_secs(30));
        assert_eq!(vus, 80);
    }

    // 3. At elapsed = 0 returns 0 (before first stage ramp has progressed)
    #[test]
    fn target_vus_at_zero_elapsed_returns_zero() {
        let curve = make_curve(vec![(60, 100, RampType::Linear)]);
        let vus = curve.target_vus_at(Duration::ZERO);
        assert_eq!(vus, 0);
    }

    // 4. At elapsed >= total_duration returns 0 (curve complete)
    #[test]
    fn target_vus_at_after_total_duration_returns_zero() {
        let curve = make_curve(vec![(60, 100, RampType::Linear)]);
        let vus = curve.target_vus_at(Duration::from_secs(60));
        assert_eq!(vus, 0);
        let vus = curve.target_vus_at(Duration::from_secs(120));
        assert_eq!(vus, 0);
    }

    // 5. total_duration sums all stage durations
    #[test]
    fn total_duration_sums_all_stages() {
        let curve = make_curve(vec![
            (30, 10, RampType::Linear),
            (60, 50, RampType::Linear),
            (90, 0, RampType::Linear),
        ]);
        assert_eq!(curve.total_duration(), Duration::from_secs(180));
    }

    // 6. JSON parsing — valid curve parses correctly
    #[test]
    fn json_parsing_valid_curve() {
        let json = r#"{
            "stages": [
                { "duration": "30s", "target_vus": 10 },
                { "duration": "1m", "target_vus": 50, "ramp": "linear" },
                { "duration": "30s", "target_vus": 0, "ramp": "step" }
            ]
        }"#;
        let curve: LoadCurve = json.parse().expect("should parse");
        assert_eq!(curve.stages.len(), 3);
        assert_eq!(curve.stages[0].duration, Duration::from_secs(30));
        assert_eq!(curve.stages[0].target_vus, 10);
        assert_eq!(curve.stages[1].duration, Duration::from_secs(60));
        assert_eq!(curve.stages[1].target_vus, 50);
        assert_eq!(curve.stages[2].ramp, RampType::Step);
    }

    // 7. JSON parsing — missing `ramp` field defaults to Linear
    #[test]
    fn json_parsing_missing_ramp_defaults_to_linear() {
        let json = r#"{
            "stages": [
                { "duration": "10s", "target_vus": 5 }
            ]
        }"#;
        let curve: LoadCurve = json.parse().expect("should parse");
        assert_eq!(curve.stages[0].ramp, RampType::Linear);
    }

    // 8. Duration string parsing — "30s", "2m", "1m30s"
    #[test]
    fn duration_string_parsing() {
        assert_eq!(parse_duration_str("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration_str("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(
            parse_duration_str("1m30s").unwrap(),
            Duration::from_secs(90)
        );
    }

    #[test]
    fn duration_string_parsing_invalid() {
        assert!(parse_duration_str("").is_err());
        assert!(parse_duration_str("abc").is_err());
        assert!(parse_duration_str("1h").is_err());
    }

    #[test]
    fn validate_rejects_empty_stages() {
        let curve = LoadCurve { stages: vec![] };
        assert!(curve.validate().is_err());
    }

    #[test]
    fn validate_rejects_too_many_stages() {
        let stages = (0..MAX_STAGES + 1)
            .map(|_| Stage {
                duration: Duration::from_secs(1),
                target_vus: 1,
                ramp: RampType::Linear,
            })
            .collect();
        let curve = LoadCurve { stages };
        assert!(curve.validate().is_err());
    }

    #[test]
    fn validate_rejects_vus_exceeding_max() {
        let curve = make_curve(vec![(10, MAX_VUS + 1, RampType::Linear)]);
        assert!(curve.validate().is_err());
    }

    #[test]
    fn validate_accepts_valid_curve() {
        let curve = make_curve(vec![(10, 100, RampType::Linear)]);
        assert!(curve.validate().is_ok());
    }

    // ── TryFrom<ExecutionConfig> tests ────────────────────────────────────────

    #[test]
    fn try_from_execution_config_valid_stages() {
        let cfg = crate::config::ExecutionConfig {
            stages: Some(vec![
                Stage {
                    duration: Duration::from_secs(10),
                    target_vus: 5,
                    ramp: RampType::Linear,
                },
                Stage {
                    duration: Duration::from_secs(20),
                    target_vus: 10,
                    ramp: RampType::Step,
                },
            ]),
            request_count: None,
            concurrency: None,
            rps: None,
        };
        let curve = LoadCurve::try_from(cfg).expect("should succeed");
        assert_eq!(curve.stages.len(), 2);
        assert_eq!(curve.stages[0].target_vus, 5);
        assert_eq!(curve.stages[1].target_vus, 10);
    }

    #[test]
    fn try_from_execution_config_empty_stages_fails_validation() {
        let cfg = crate::config::ExecutionConfig {
            stages: Some(vec![]),
            request_count: None,
            concurrency: None,
            rps: None,
        };
        let result = LoadCurve::try_from(cfg);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("at least one stage"),
            "expected validation error, got: {msg}"
        );
    }

    #[test]
    fn try_from_execution_config_missing_stages_field_fails() {
        let cfg = crate::config::ExecutionConfig {
            stages: None,
            request_count: Some(100),
            concurrency: Some(10),
            rps: None,
        };
        let result = LoadCurve::try_from(cfg);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("execution.stages is required"),
            "expected missing stages error, got: {msg}"
        );
    }

    // Multi-stage linear interpolation: second stage ramps from first stage's target
    #[test]
    fn target_vus_at_multi_stage_linear() {
        // Stage 1: 0 -> 100 over 100s
        // Stage 2: 100 -> 200 over 100s
        let curve = make_curve(vec![
            (100, 100, RampType::Linear),
            (100, 200, RampType::Linear),
        ]);
        // At t=150 (50s into stage 2): should be 150
        let vus = curve.target_vus_at(Duration::from_secs(150));
        assert_eq!(vus, 150);
    }
}
