//! Scenario resolution: converts `ScenarioConfig` entries (from a parsed config
//! file) into fully resolved `ResolvedScenario` structs ready for VU execution.
//!
//! This module lives in `lmn-core` so that it can access the `pub(crate)` helper
//! `build_request_config` and all internal types, while still exposing a public
//! API consumed by the CLI adapter layer.

use std::sync::Arc;

use crate::config::ScenarioConfig;
use crate::config::secret::{SensitiveString, resolve_env_placeholders};
use crate::execution::{OnStepFailure, ResolvedScenario, ResolvedStep, build_request_config};
use crate::request_template::Template;
use crate::response_template::ResponseTemplate;
use crate::response_template::field::TrackedField;

// ── resolve_scenarios ─────────────────────────────────────────────────────────

/// Resolves a list of `ScenarioConfig` entries (from a parsed YAML config) into
/// fully resolved `ResolvedScenario` values ready to be passed to a
/// `FixedExecutor` or `CurveExecutor`.
///
/// # Header merge order
///
/// Headers are merged with case-insensitive last-wins semantics in the following
/// priority order:
/// 1. `global_headers` — already-resolved `SensitiveString` values from the
///    `run.headers` config section (or CLI `--header` flags).
/// 2. Scenario-level `headers` — applied to every step in the scenario.
/// 3. Step-level `headers` — applied to this step only.
///
/// `${ENV_VAR}` placeholders are resolved in scenario and step header values.
/// Global headers are assumed to be pre-resolved by the caller.
pub fn resolve_scenarios(
    scenario_configs: &[ScenarioConfig],
    global_headers: &[(String, SensitiveString)],
) -> Result<Vec<ResolvedScenario>, Box<dyn std::error::Error>> {
    let mut resolved_scenarios: Vec<ResolvedScenario> = Vec::with_capacity(scenario_configs.len());

    for scenario_cfg in scenario_configs {
        let on_step_failure =
            parse_on_step_failure(scenario_cfg.on_step_failure.as_deref(), &scenario_cfg.name)?;
        let weight = scenario_cfg.weight.unwrap_or(1);

        // Build the scenario-level header base: global → scenario (last-wins).
        let mut scenario_header_map: Vec<(String, String)> = global_headers
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        if let Some(ref sh) = scenario_cfg.headers {
            merge_headers_into(&mut scenario_header_map, sh);
        }

        let mut resolved_steps: Vec<ResolvedStep> = Vec::with_capacity(scenario_cfg.steps.len());

        for step_cfg in &scenario_cfg.steps {
            // Build the step-level header map: scenario → step (last-wins).
            let mut step_header_map = scenario_header_map.clone();
            if let Some(ref step_headers) = step_cfg.headers {
                merge_headers_into(&mut step_header_map, step_headers);
            }

            // Resolve ${ENV_VAR} in all step header values.
            let resolved_headers: Vec<(String, SensitiveString)> = step_header_map
                .into_iter()
                .map(|(name, value)| {
                    let resolved = resolve_env_placeholders(&value).map_err(|e| {
                        format!(
                            "scenario '{}', step '{}', header '{name}': {e}",
                            scenario_cfg.name, step_cfg.name
                        )
                    })?;
                    Ok::<_, String>((name, SensitiveString::new(resolved)))
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e: String| Box::<dyn std::error::Error>::from(e))?;

            // Parse the step method.
            let step_method = parse_method_str(&step_cfg.method).map_err(|e| {
                format!(
                    "scenario '{}', step '{}': {e}",
                    scenario_cfg.name, step_cfg.name
                )
            })?;

            // Resolve ${ENV_VAR} in the step host.
            let step_host = resolve_env_placeholders(&step_cfg.host).map_err(|e| {
                format!(
                    "scenario '{}', step '{}', host: {e}",
                    scenario_cfg.name, step_cfg.name
                )
            })?;

            // Build per-step RequestConfig.
            // Concurrency hint is 1 here — the executor manages actual concurrency.
            // Tracked fields are set via response_template below.
            let request_config = build_request_config(
                step_host,
                step_method,
                None, // body is driven by request_template per step
                None, // tracked_fields come from response_template
                resolved_headers.clone(),
                1,
            )
            .map_err(|e| {
                format!(
                    "scenario '{}', step '{}': failed to build request config: {e}",
                    scenario_cfg.name, step_cfg.name
                )
            })?;

            // Load request template if present.
            let request_template: Option<Arc<Template>> = step_cfg
                .request_template
                .as_deref()
                .map(|path| {
                    Template::parse(path.as_ref()).map(Arc::new).map_err(|e| {
                        format!(
                            "scenario '{}', step '{}': failed to parse request_template '{path}': {e}",
                            scenario_cfg.name, step_cfg.name
                        )
                    })
                })
                .transpose()
                .map_err(|e: String| Box::<dyn std::error::Error>::from(e))?;

            // Load response template if present.
            let response_template: Option<Arc<Vec<TrackedField>>> = step_cfg
                .response_template
                .as_deref()
                .map(|path| {
                    ResponseTemplate::parse(path.as_ref())
                        .map(|rt| Arc::new(rt.fields))
                        .map_err(|e| {
                            format!(
                                "scenario '{}', step '{}': failed to parse response_template '{path}': {e}",
                                scenario_cfg.name, step_cfg.name
                            )
                        })
                })
                .transpose()
                .map_err(|e: String| Box::<dyn std::error::Error>::from(e))?;

            // Plain headers for per-request injection in the VU (no SensitiveString wrapping
            // at this layer — values are passed as raw strings to reqwest headers).
            let plain_headers: Arc<Vec<(String, String)>> = Arc::new(
                resolved_headers
                    .into_iter()
                    .map(|(k, v)| (k, v.to_string()))
                    .collect(),
            );

            resolved_steps.push(ResolvedStep {
                name: Arc::from(step_cfg.name.as_str()),
                request_config: Arc::clone(&request_config),
                plain_headers,
                request_template,
                response_template,
            });
        }

        resolved_scenarios.push(ResolvedScenario {
            name: Arc::from(scenario_cfg.name.as_str()),
            weight,
            on_step_failure,
            steps: resolved_steps,
        });
    }

    Ok(resolved_scenarios)
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn parse_on_step_failure(
    s: Option<&str>,
    scenario_name: &str,
) -> Result<OnStepFailure, Box<dyn std::error::Error>> {
    match s {
        Some("abort_iteration") => Ok(OnStepFailure::AbortIteration),
        Some("continue") | None => Ok(OnStepFailure::Continue),
        Some(other) => Err(format!(
            "scenario '{scenario_name}': invalid on_step_failure value '{other}'"
        )
        .into()),
    }
}

fn parse_method_str(s: &str) -> Result<crate::command::HttpMethod, String> {
    match s.to_lowercase().as_str() {
        "get" => Ok(crate::command::HttpMethod::Get),
        "post" => Ok(crate::command::HttpMethod::Post),
        "put" => Ok(crate::command::HttpMethod::Put),
        "patch" => Ok(crate::command::HttpMethod::Patch),
        "delete" => Ok(crate::command::HttpMethod::Delete),
        other => Err(format!(
            "unknown method '{other}' — expected one of: get, post, put, patch, delete"
        )),
    }
}

/// Merges incoming headers into a base list using case-insensitive last-wins semantics.
fn merge_headers_into(
    base: &mut Vec<(String, String)>,
    incoming: &std::collections::HashMap<String, String>,
) {
    for (name, value) in incoming {
        base.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        base.push((name.clone(), value.clone()));
    }
}
