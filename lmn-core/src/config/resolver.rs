//! Scenario resolution: converts parsed `ScenarioConfig` entries into fully
//! resolved `ResolvedScenario` structs ready for VU execution.
//!
//! The [`ScenarioResolver`] handles the three-layer header merge
//! (global → scenario → step), `${ENV_VAR}` expansion, method parsing, and
//! template loading — producing a `Vec<ResolvedScenario>` that executors
//! consume directly.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::capture::{CaptureDefinition, parse_json_path, scan_capture_refs};
use crate::command::HttpMethod;
use crate::config::ScenarioConfig;
use crate::config::secret::{SensitiveString, resolve_env_placeholders};
use crate::execution::{OnStepFailure, ResolvedScenario, ResolvedStep, build_request_config};
use crate::request_template::Template;
use crate::response_template::ResponseTemplate;
use crate::response_template::field::TrackedField;

// ── ScenarioResolver ─────────────────────────────────────────────────────────

/// Resolves [`ScenarioConfig`] values into execution-ready [`ResolvedScenario`]
/// structs.
///
/// Header merge order (case-insensitive last-wins):
/// 1. **Global** — pre-resolved headers from `run.headers` / CLI `--header`.
/// 2. **Scenario** — applied to every step in the scenario.
/// 3. **Step** — applied to a single step only.
///
/// `${ENV_VAR}` placeholders in scenario and step headers are expanded during
/// resolution. Global headers are assumed pre-resolved by the caller.
pub struct ScenarioResolver<'a> {
    global_headers: &'a [(String, SensitiveString)],
}

impl<'a> ScenarioResolver<'a> {
    pub fn new(global_headers: &'a [(String, SensitiveString)]) -> Self {
        Self { global_headers }
    }

    /// Resolve all scenarios from parsed config into execution-ready structs.
    pub fn resolve(
        &self,
        configs: &[ScenarioConfig],
    ) -> Result<Vec<ResolvedScenario>, Box<dyn std::error::Error>> {
        configs
            .iter()
            .map(|cfg| self.resolve_scenario(cfg))
            .collect()
    }

    fn resolve_scenario(
        &self,
        cfg: &ScenarioConfig,
    ) -> Result<ResolvedScenario, Box<dyn std::error::Error>> {
        let scenario_headers = self.merge_scenario_headers(cfg.headers.as_ref());

        let steps = cfg
            .steps
            .iter()
            .map(|step| self.resolve_step(step, &cfg.name, &scenario_headers))
            .collect::<Result<Vec<_>, _>>()?;

        // Static capture dependency validation: ensure every {{capture.KEY}}
        // reference in a step is defined by a preceding step's captures.
        let mut defined_aliases = HashSet::new();
        for (i, step_cfg) in cfg.steps.iter().enumerate() {
            // Collect references from headers and body/inline_body
            let mut refs = Vec::new();
            if let Some(ref headers) = step_cfg.headers {
                for (name, value) in headers {
                    refs.extend(scan_capture_refs(value).map_err(|e| {
                        format!(
                            "scenario '{}', step '{}': header '{name}': {e}",
                            cfg.name, step_cfg.name
                        )
                    })?);
                }
            }
            if let Some(ref body) = step_cfg.body {
                refs.extend(scan_capture_refs(body).map_err(|e| {
                    format!(
                        "scenario '{}', step '{}': body: {e}",
                        cfg.name, step_cfg.name
                    )
                })?);
            }

            for key in &refs {
                if !defined_aliases.contains(key.as_str()) {
                    return Err(format!(
                        "scenario '{}', step '{}' (index {}): references \
                         {{{{capture.{key}}}}} but no preceding step defines it",
                        cfg.name, step_cfg.name, i
                    )
                    .into());
                }
            }

            // Add this step's capture aliases to the defined set
            if let Some(ref captures) = step_cfg.capture {
                for alias in captures.keys() {
                    defined_aliases.insert(alias.clone());
                }
            }
        }

        Ok(ResolvedScenario {
            name: Arc::from(cfg.name.as_str()),
            weight: cfg.weight.unwrap_or(1),
            on_step_failure: parse_on_step_failure(cfg.on_step_failure.as_deref(), &cfg.name)?,
            steps,
        })
    }

    fn resolve_step(
        &self,
        step: &crate::config::ScenarioStepConfig,
        scenario_name: &str,
        scenario_headers: &[(String, String)],
    ) -> Result<ResolvedStep, Box<dyn std::error::Error>> {
        let ctx = StepContext {
            scenario: scenario_name,
            step: &step.name,
        };

        let merged_headers = merge_step_headers(scenario_headers, step.headers.as_ref());
        let resolved_headers = resolve_header_env_vars(&merged_headers, &ctx)?;
        let plain_headers = to_plain_headers(&resolved_headers);

        let host =
            resolve_env_placeholders(&step.host).map_err(|e| ctx.error(format!("host: {e}")))?;

        let method = parse_method(&step.method).map_err(|e| ctx.error(e))?;

        let request_config = build_request_config(host, method, None, None, resolved_headers, 1)
            .map_err(|e| ctx.error(format!("request config: {e}")))?;

        let request_template = load_request_template(step.request_template.as_deref(), &ctx)?;
        let response_template = load_response_template(step.response_template.as_deref(), &ctx)?;

        // Parse capture definitions
        let captures = if let Some(ref capture_map) = step.capture {
            capture_map
                .iter()
                .map(|(alias, path)| {
                    let parsed = parse_json_path(path)
                        .map_err(|e| ctx.error(format!("capture '{alias}': {e}")))?;
                    Ok(CaptureDefinition {
                        alias: alias.clone(),
                        path: parsed,
                    })
                })
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?
        } else {
            vec![]
        };

        // Detect if any header value contains {{capture. references
        let has_capture_headers = plain_headers.iter().any(|(_, v)| v.contains("{{capture."));

        // Handle inline body
        let inline_body = step.body.as_deref().map(Arc::from);

        Ok(ResolvedStep {
            name: Arc::from(step.name.as_str()),
            request_config,
            plain_headers,
            request_template,
            response_template,
            captures,
            inline_body,
            has_capture_headers,
        })
    }

    /// Merge global headers with scenario-level overrides.
    fn merge_scenario_headers(
        &self,
        scenario_headers: Option<&HashMap<String, String>>,
    ) -> Vec<(String, String)> {
        let mut headers: Vec<(String, String)> = self
            .global_headers
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        if let Some(overrides) = scenario_headers {
            merge_headers_into(&mut headers, overrides);
        }
        headers
    }
}

// ── Public entry point (convenience wrapper) ─────────────────────────────────

/// Convenience function wrapping [`ScenarioResolver`].
pub fn resolve_scenarios(
    scenario_configs: &[ScenarioConfig],
    global_headers: &[(String, SensitiveString)],
) -> Result<Vec<ResolvedScenario>, Box<dyn std::error::Error>> {
    ScenarioResolver::new(global_headers).resolve(scenario_configs)
}

// ── StepContext ──────────────────────────────────────────────────────────────

/// Provides consistent error context for a specific scenario + step.
struct StepContext<'a> {
    scenario: &'a str,
    step: &'a str,
}

impl StepContext<'_> {
    fn error(&self, detail: impl std::fmt::Display) -> Box<dyn std::error::Error> {
        format!(
            "scenario '{}', step '{}': {detail}",
            self.scenario, self.step
        )
        .into()
    }
}

// ── Header helpers ──────────────────────────────────────────────────────────

fn merge_step_headers(
    base: &[(String, String)],
    step_headers: Option<&HashMap<String, String>>,
) -> Vec<(String, String)> {
    let mut merged = base.to_vec();
    if let Some(overrides) = step_headers {
        merge_headers_into(&mut merged, overrides);
    }
    merged
}

fn merge_headers_into(base: &mut Vec<(String, String)>, incoming: &HashMap<String, String>) {
    for (name, value) in incoming {
        base.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        base.push((name.clone(), value.clone()));
    }
}

fn resolve_header_env_vars(
    headers: &[(String, String)],
    ctx: &StepContext<'_>,
) -> Result<Vec<(String, SensitiveString)>, Box<dyn std::error::Error>> {
    headers
        .iter()
        .map(|(name, value)| {
            let resolved = resolve_env_placeholders(value)
                .map_err(|e| ctx.error(format!("header '{name}': {e}")))?;
            Ok((name.clone(), SensitiveString::new(resolved)))
        })
        .collect()
}

fn to_plain_headers(headers: &[(String, SensitiveString)]) -> Arc<Vec<(String, String)>> {
    Arc::new(
        headers
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect(),
    )
}

// ── Parsing helpers ─────────────────────────────────────────────────────────

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

fn parse_method(s: &str) -> Result<HttpMethod, String> {
    match s.to_lowercase().as_str() {
        "get" => Ok(HttpMethod::Get),
        "post" => Ok(HttpMethod::Post),
        "put" => Ok(HttpMethod::Put),
        "patch" => Ok(HttpMethod::Patch),
        "delete" => Ok(HttpMethod::Delete),
        other => Err(format!(
            "unknown method '{other}' — expected one of: get, post, put, patch, delete"
        )),
    }
}

// ── Template helpers ────────────────────────────────────────────────────────

fn load_request_template(
    path: Option<&str>,
    ctx: &StepContext<'_>,
) -> Result<Option<Arc<Template>>, Box<dyn std::error::Error>> {
    path.map(|p| {
        Template::parse(p.as_ref())
            .map(Arc::new)
            .map_err(|e| ctx.error(format!("request_template '{p}': {e}")))
    })
    .transpose()
}

fn load_response_template(
    path: Option<&str>,
    ctx: &StepContext<'_>,
) -> Result<Option<Arc<Vec<TrackedField>>>, Box<dyn std::error::Error>> {
    path.map(|p| {
        ResponseTemplate::parse(p.as_ref())
            .map(|rt| Arc::new(rt.fields))
            .map_err(|e| ctx.error(format!("response_template '{p}': {e}")))
    })
    .transpose()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ScenarioStepConfig;

    fn step(
        name: &str,
        body: Option<&str>,
        capture: Option<Vec<(&str, &str)>>,
    ) -> ScenarioStepConfig {
        ScenarioStepConfig {
            name: name.to_string(),
            host: "http://localhost".to_string(),
            method: "get".to_string(),
            headers: None,
            request_template: None,
            response_template: None,
            body: body.map(|s| s.to_string()),
            capture: capture.map(|pairs| {
                pairs
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            }),
        }
    }

    fn scenario(steps: Vec<ScenarioStepConfig>) -> ScenarioConfig {
        ScenarioConfig {
            name: "test".to_string(),
            weight: None,
            on_step_failure: None,
            headers: None,
            steps,
        }
    }

    fn resolve(cfg: &ScenarioConfig) -> Result<ResolvedScenario, Box<dyn std::error::Error>> {
        let resolver = ScenarioResolver::new(&[]);
        resolver.resolve_scenario(cfg)
    }

    #[test]
    fn capture_ref_satisfied_by_preceding_step() {
        let cfg = scenario(vec![
            step("login", None, Some(vec![("token", "$.data.token")])),
            step("use", Some(r#"{"t": "{{capture.token}}"}"#), None),
        ]);
        assert!(resolve(&cfg).is_ok());
    }

    #[test]
    fn capture_ref_undefined_key_is_error() {
        let cfg = scenario(vec![
            step("login", None, None),
            step("use", Some(r#"{"t": "{{capture.token}}"}"#), None),
        ]);
        let err = resolve(&cfg).err().expect("expected error").to_string();
        assert!(
            err.contains("capture.token"),
            "error should mention the key: {err}"
        );
        assert!(
            err.contains("no preceding step"),
            "error should explain cause: {err}"
        );
    }

    #[test]
    fn capture_ref_in_same_step_is_error() {
        // A step cannot reference its own captures — they haven't been extracted yet.
        let cfg = scenario(vec![step(
            "self_ref",
            Some(r#"{"t": "{{capture.token}}"}"#),
            Some(vec![("token", "$.data.token")]),
        )]);
        let err = resolve(&cfg).err().expect("expected error").to_string();
        assert!(err.contains("capture.token"), "{err}");
    }

    #[test]
    fn capture_ref_in_header_validated() {
        let mut s = step("use", None, None);
        s.headers = Some(
            [(
                "Authorization".to_string(),
                "Bearer {{capture.token}}".to_string(),
            )]
            .into_iter()
            .collect(),
        );
        let cfg = scenario(vec![s]);
        let err = resolve(&cfg).err().expect("expected error").to_string();
        assert!(err.contains("capture.token"), "{err}");
    }

    #[test]
    fn multiple_captures_chain_across_steps() {
        let cfg = scenario(vec![
            step("s1", None, Some(vec![("a", "$.a")])),
            step("s2", Some("{{capture.a}}"), Some(vec![("b", "$.b")])),
            step("s3", Some("{{capture.a}} {{capture.b}}"), None),
        ]);
        assert!(resolve(&cfg).is_ok());
    }

    #[test]
    fn no_captures_no_refs_is_ok() {
        let cfg = scenario(vec![
            step("s1", None, None),
            step("s2", Some("plain body"), None),
        ]);
        assert!(resolve(&cfg).is_ok());
    }
}
