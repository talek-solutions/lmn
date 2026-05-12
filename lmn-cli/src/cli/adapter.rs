use std::path::PathBuf;

use crate::cli::command::{
    ConfigureRequestArgs, ConfigureResponseArgs, HttpMethod, OutputFormat, RunArgs,
};
use lmn_core::command::Body;
use lmn_core::command::configure_template::{ConfigureTemplateCommand, TemplateKind};
use lmn_core::command::run::RunCommand;
use lmn_core::config::{ExecutionConfig, LumenConfig, parse_config, resolve_scenarios};
use lmn_core::execution::{ExecutionMode, RequestSpec};
use lmn_core::http::BodyFormat;
use lmn_core::load_curve::LoadCurve;
use lmn_core::publish::PublishConfigYaml;
use lmn_core::threshold::Threshold;

impl From<HttpMethod> for lmn_core::command::HttpMethod {
    fn from(m: HttpMethod) -> Self {
        match m {
            HttpMethod::Get => lmn_core::command::HttpMethod::Get,
            HttpMethod::Post => lmn_core::command::HttpMethod::Post,
            HttpMethod::Put => lmn_core::command::HttpMethod::Put,
            HttpMethod::Patch => lmn_core::command::HttpMethod::Patch,
            HttpMethod::Delete => lmn_core::command::HttpMethod::Delete,
        }
    }
}

// ── method string parsing ─────────────────────────────────────────────────────

fn parse_method_str(s: &str) -> Result<lmn_core::command::HttpMethod, String> {
    match s.to_lowercase().as_str() {
        "get" => Ok(lmn_core::command::HttpMethod::Get),
        "post" => Ok(lmn_core::command::HttpMethod::Post),
        "put" => Ok(lmn_core::command::HttpMethod::Put),
        "patch" => Ok(lmn_core::command::HttpMethod::Patch),
        "delete" => Ok(lmn_core::command::HttpMethod::Delete),
        other => Err(format!(
            "unknown method '{other}' in config — expected one of: get, post, put, patch, delete"
        )),
    }
}

// ── output format string parsing ──────────────────────────────────────────────

fn parse_output_str(s: &str) -> Result<OutputFormat, String> {
    match s.to_lowercase().as_str() {
        "table" => Ok(OutputFormat::Table),
        "json" => Ok(OutputFormat::Json),
        other => Err(format!(
            "unknown output format '{other}' in config — expected one of: table, json"
        )),
    }
}

// ── RunArgsResolved ───────────────────────────────────────────────────────────

/// CLI `RunArgs` after config-file merging.
///
/// CLI flags always take precedence over values loaded from `--config`/`-f`.
/// Fields that were not supplied by the user fall back to config-file values;
/// everything else retains the CLI value (including clap defaults).
///
/// `thresholds` is `None` when neither the config file nor the CLI carries
/// threshold rules — in that case the exit code is always 0 after a successful
/// run.
pub struct RunArgsResolved {
    pub request: RequestSpec,
    pub execution: ExecutionMode,
    /// Threshold rules sourced from the config file.
    /// `None` when no config was supplied or the config has no `thresholds` section.
    pub thresholds: Option<Vec<Threshold>>,
    pub output: OutputFormat,
    pub output_file: Option<PathBuf>,
    pub publish_config: Option<PublishConfigYaml>,
}

impl RunArgsResolved {
    /// Converts a `RunArgsResolved` into a `RunCommand` (the core engine type).
    pub fn into_run_command(self) -> RunCommand {
        RunCommand {
            request: self.request,
            execution: self.execution,
        }
    }
}

// ── TryFrom<RunArgs> for RunArgsResolved ─────────────────────────────────────

impl TryFrom<RunArgs> for RunArgsResolved {
    type Error = Box<dyn std::error::Error>;

    fn try_from(args: RunArgs) -> Result<Self, Self::Error> {
        // Load .env file if present — silently ignore if missing
        dotenvy::dotenv().ok();

        const MAX_FILE_BYTES: usize = 1_048_576; // 1 MB

        // Load and parse config file when --config/-f was supplied.
        // Read first, then check size — avoids TOCTOU race between metadata check and read.
        let cfg: Option<LumenConfig> = args
            .config
            .as_ref()
            .map(|path| {
                let bytes = std::fs::read(path)
                    .map_err(|e| format!("failed to read config '{}': {e}", path.display()))?;
                if bytes.len() > MAX_FILE_BYTES {
                    return Err(format!(
                        "config file '{}' exceeds 1 MB limit ({} bytes)",
                        path.display(),
                        bytes.len()
                    ));
                }
                let contents = String::from_utf8(bytes).map_err(|e| {
                    format!("config file '{}' is not valid UTF-8: {e}", path.display())
                })?;
                parse_config(&contents)
                    .map_err(|e| format!("failed to parse config '{}': {e}", path.display()))
            })
            .transpose()
            .map_err(|e: String| Box::<dyn std::error::Error>::from(e))?;

        // ── Merge strategy for fields with CLI defaults ───────────────────────
        // CLI Some always wins → config fills None → hardcoded default as fallback.
        // This correctly handles the case where the user explicitly passes a CLI
        // flag (Some) vs. leaving it absent (None, filled by config or default).

        // ── method ────────────────────────────────────────────────────────────
        let method: lmn_core::command::HttpMethod = if let Some(m) = args.method {
            m.into()
        } else if let Some(s) = cfg.as_ref().and_then(|c| c.run.as_ref()?.method.as_deref()) {
            parse_method_str(s).map_err(Box::<dyn std::error::Error>::from)?
        } else {
            lmn_core::command::HttpMethod::Get
        };

        // ── output format ─────────────────────────────────────────────────────
        let output: OutputFormat = if let Some(o) = args.output {
            o
        } else if let Some(s) = cfg.as_ref().and_then(|c| c.run.as_ref()?.output.as_deref()) {
            parse_output_str(s).map_err(Box::<dyn std::error::Error>::from)?
        } else {
            OutputFormat::Table
        };

        // ── output_file: CLI Some wins; else config value; else None ──────────
        let output_file: Option<PathBuf> = args.output_file.or_else(|| {
            cfg.as_ref()
                .and_then(|c| c.run.as_ref()?.output_file.as_deref().map(PathBuf::from))
        });

        // ── request_count / concurrency ───────────────────────────────────────
        let request_count: usize = args
            .request_count
            .map(|v| v as usize)
            .or_else(|| {
                cfg.as_ref()
                    .and_then(|c| c.execution.as_ref()?.request_count)
            })
            .unwrap_or(100);

        let concurrency: usize = args
            .concurrency
            .map(|v| v as usize)
            .or_else(|| cfg.as_ref().and_then(|c| c.execution.as_ref()?.concurrency))
            .unwrap_or(100);

        // ── execution mode ────────────────────────────────────────────────────
        // Priority:
        //   1. CLI --load-curve flag → ExecutionMode::Curve (from file)
        //   2. config execution.stages → ExecutionMode::Curve (via TryFrom)
        //   3. fixed mode using request_count / concurrency
        let execution: ExecutionMode = if let Some(path) = args.load_curve {
            // Read first, then check size — avoids TOCTOU race.
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("failed to read load curve file '{}': {e}", path.display()))?;
            if bytes.len() > MAX_FILE_BYTES {
                return Err(format!(
                    "load curve file '{}' exceeds 1 MB limit ({} bytes)",
                    path.display(),
                    bytes.len()
                )
                .into());
            }
            let content = String::from_utf8(bytes).map_err(|e| {
                format!(
                    "load curve file '{}' is not valid UTF-8: {e}",
                    path.display()
                )
            })?;
            let curve = content.parse::<LoadCurve>().map_err(|e| {
                format!("failed to parse load curve file '{}': {e}", path.display())
            })?;
            curve
                .validate()
                .map_err(|e| format!("invalid load curve '{}': {e}", path.display()))?;
            ExecutionMode::Curve(curve)
        } else if let Some(ref c) = cfg {
            let exec_cfg: Option<&ExecutionConfig> = c.execution.as_ref();
            if exec_cfg.and_then(|e| e.stages.as_ref()).is_some() {
                let exec = exec_cfg
                    .ok_or("execution config unexpectedly None")?
                    .clone();
                let curve =
                    LoadCurve::try_from(exec).map_err(Box::<dyn std::error::Error>::from)?;
                ExecutionMode::Curve(curve)
            } else {
                ExecutionMode::Fixed {
                    request_count,
                    concurrency,
                }
            }
        } else {
            ExecutionMode::Fixed {
                request_count,
                concurrency,
            }
        };

        let thresholds = cfg.as_ref().and_then(|c| c.thresholds.clone());

        // ── headers ───────────────────────────────────────────────────────────────
        // Merge strategy: config headers as base, CLI --header flags override on
        // same key (case-insensitive). Then resolve ${ENV_VAR} in all values.
        let mut header_map: Vec<(String, String)> = cfg
            .as_ref()
            .and_then(|c| c.run.as_ref()?.headers.clone())
            .map(|map| map.into_iter().collect())
            .unwrap_or_default();

        // CLI headers override config headers (case-insensitive key match)
        for raw in &args.headers {
            // parse_header already validated the ': ' separator
            let colon_pos = raw
                .find(": ")
                .ok_or_else(|| format!("invalid header '{raw}': missing ': ' separator"))?;
            let name = raw[..colon_pos].to_string();
            let value = raw[colon_pos + 2..].to_string();
            // Remove existing entry with same name (case-insensitive)
            header_map.retain(|(k, _)| !k.eq_ignore_ascii_case(&name));
            header_map.push((name, value));
        }

        // Validate merged header count and lengths (mirrors parse_config validation)
        const MAX_HEADERS: usize = 64;
        const MAX_HEADER_NAME_LEN: usize = 256;
        const MAX_HEADER_VALUE_LEN: usize = 8192;

        if header_map.len() > MAX_HEADERS {
            return Err(format!(
                "too many headers: {}, maximum is {MAX_HEADERS}",
                header_map.len()
            )
            .into());
        }
        for (name, value) in &header_map {
            if name.len() > MAX_HEADER_NAME_LEN {
                return Err(format!(
                    "header name '{}...' exceeds maximum length of {MAX_HEADER_NAME_LEN} bytes",
                    &name[..MAX_HEADER_NAME_LEN.min(name.len())]
                )
                .into());
            }
            if value.len() > MAX_HEADER_VALUE_LEN {
                return Err(format!(
                    "header '{name}' value exceeds maximum length of {MAX_HEADER_VALUE_LEN} bytes"
                )
                .into());
            }
        }

        // Warn on raw secrets BEFORE resolution (check original values, not resolved ones)
        for (name, raw_value) in &header_map {
            let lower_name = name.to_lowercase();
            if [
                "authorization",
                "x-api-key",
                "token",
                "secret",
                "password",
                "x-auth",
            ]
            .iter()
            .any(|k| lower_name.contains(k))
                && !raw_value.contains("${")
                && raw_value.len() > 4
            {
                eprintln!(
                    "warning: header '{name}' contains a raw value — consider using ${{ENV_VAR}} \
                     to avoid hardcoding secrets"
                );
            }
        }

        // Resolve ${ENV_VAR} in header values, then wrap in SensitiveString so
        // secrets are redacted if the value ever appears in debug output.
        use lmn_core::config::resolve_env_placeholders;
        use lmn_core::config::secret::SensitiveString;
        let headers: Vec<(String, SensitiveString)> = header_map
            .into_iter()
            .map(|(name, value)| {
                let resolved = resolve_env_placeholders(&value)
                    .map_err(|e| format!("header '{name}': {e}"))?;
                Ok::<_, String>((name, SensitiveString::new(resolved)))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e: String| Box::<dyn std::error::Error>::from(e))?;

        // ── RequestSpec resolution ────────────────────────────────────────────
        // If scenarios are defined in config, resolve them into RequestSpec::Scenarios.
        // Otherwise fall through to the single-request path (RequestSpec::Single).
        let request: RequestSpec =
            if let Some(scenario_configs) = cfg.as_ref().and_then(|c| c.scenarios.as_ref()) {
                let resolved = resolve_scenarios(scenario_configs, &headers)?;
                RequestSpec::Scenarios(resolved)
            } else {
                // Single-request path: host is required.
                let host = args
                    .host
                    .or_else(|| cfg.as_ref().and_then(|c| c.run.as_ref()?.host.clone()))
                    .ok_or("host is required: set -H or run.host in config file")?;

                // Resolve optional template paths from config
                let template_path = args
                    .request_template
                    .or_else(|| args.request_alias.map(resolve_alias("requests")))
                    .or_else(|| {
                        cfg.as_ref()
                            .and_then(|c| c.request_template.as_deref().map(PathBuf::from))
                    });

                let response_template_path = args
                    .response_template
                    .or_else(|| args.response_alias.map(resolve_alias("responses")));

                RequestSpec::Single {
                    host,
                    method,
                    body: args.body.map(|s| Body::Formatted {
                        content: s,
                        format: BodyFormat::Json,
                    }),
                    template_path,
                    response_template_path,
                    headers,
                }
            };

        let publish_yaml = cfg.as_ref().and_then(|c| c.publish.clone());
        let flag_url = args.publish_url;
        // URL precedence: --publish-url flag > LUMEN_API_URL env > yaml url.
        // Resolved here so the builder receives a single, already-resolved URL.
        let resolved_url = flag_url
            .clone()
            .or_else(|| {
                std::env::var("LUMEN_API_URL")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| publish_yaml.as_ref().and_then(|y| y.url.clone()));
        let publish_config = match (&flag_url, &publish_yaml) {
            // CLI flag: implicitly enabled.
            (Some(_), _) => Some(PublishConfigYaml {
                enabled: Some(true),
                url: resolved_url,
            }),
            // YAML section present, no CLI flag.
            (None, Some(yaml)) => Some(PublishConfigYaml {
                enabled: yaml.enabled,
                url: resolved_url,
            }),
            // Nothing configured.
            (None, None) => None,
        };

        Ok(RunArgsResolved {
            request,
            execution,
            thresholds,
            output,
            output_file,
            publish_config,
        })
    }
}

/// Merges a `HashMap<String, String>` of headers into an existing ordered
/// `Vec<(String, String)>`, using case-insensitive last-wins semantics.
#[cfg(test)]
fn merge_headers_into(
    base: &mut Vec<(String, String)>,
    incoming: &std::collections::HashMap<String, String>,
) {
    for (name, value) in incoming {
        base.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        base.push((name.clone(), value.clone()));
    }
}

impl From<ConfigureRequestArgs> for ConfigureTemplateCommand {
    fn from(args: ConfigureRequestArgs) -> Self {
        ConfigureTemplateCommand {
            body: args.body.map(|s| Body::Formatted {
                content: s,
                format: BodyFormat::Json,
            }),
            template_path: args.template_path,
            alias: args.alias,
            kind: TemplateKind::Request,
        }
    }
}

impl From<ConfigureResponseArgs> for ConfigureTemplateCommand {
    fn from(args: ConfigureResponseArgs) -> Self {
        ConfigureTemplateCommand {
            body: args.body.map(|s| Body::Formatted {
                content: s,
                format: BodyFormat::Json,
            }),
            template_path: args.template_path,
            alias: args.alias,
            kind: TemplateKind::Response,
        }
    }
}

pub fn resolve_alias(sub_dir: &'static str) -> impl Fn(String) -> PathBuf {
    move |alias| {
        let mut path = PathBuf::from(alias);
        if path.extension().is_none() {
            path.set_extension("json");
        }
        PathBuf::from(".templates").join(sub_dir).join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmn_core::command::Command;
    use lmn_core::execution::OnStepFailure;

    #[tokio::test]
    async fn execute_creates_file_from_body() {
        let alias = "__test_exec_body";
        let path = std::path::PathBuf::from(".templates/requests").join(format!("{}.json", alias));
        let _ = std::fs::remove_file(&path);
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());

        let cmd = ConfigureTemplateCommand::from(ConfigureRequestArgs {
            alias: alias.to_string(),
            body: Some(r#"{"ok":true}"#.to_string()),
            template_path: None,
        });
        assert!(cmd.execute().await.is_ok());
        assert!(path.exists());
    }

    #[tokio::test]
    async fn execute_returns_error_when_no_body_or_path() {
        let cmd = ConfigureTemplateCommand::from(ConfigureRequestArgs {
            alias: "irrelevant".to_string(),
            body: None,
            template_path: None,
        });
        assert!(cmd.execute().await.is_err());
    }

    #[tokio::test]
    async fn execute_returns_error_on_duplicate_alias() {
        let alias = "__test_exec_duplicate";
        let path = std::path::PathBuf::from(".templates/requests").join(format!("{}.json", alias));
        let _ = std::fs::remove_file(&path);
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());

        let make_cmd = || {
            ConfigureTemplateCommand::from(ConfigureRequestArgs {
                alias: alias.to_string(),
                body: Some("{}".to_string()),
                template_path: None,
            })
        };
        assert!(make_cmd().execute().await.is_ok());
        assert!(make_cmd().execute().await.is_err());
    }

    #[test]
    fn resolve_alias_appends_json_extension() {
        let path = resolve_alias("requests")("my-alias".to_string());
        assert_eq!(path, PathBuf::from(".templates/requests/my-alias.json"));
    }

    #[test]
    fn resolve_alias_preserves_existing_extension() {
        let path = resolve_alias("requests")("my-alias.json".to_string());
        assert_eq!(path, PathBuf::from(".templates/requests/my-alias.json"));
    }

    #[test]
    fn resolve_alias_uses_correct_subdir() {
        let path = resolve_alias("responses")("template".to_string());
        assert_eq!(path, PathBuf::from(".templates/responses/template.json"));
    }

    fn make_run_args(load_curve: Option<std::path::PathBuf>) -> RunArgs {
        RunArgs {
            host: Some("http://localhost:3000".to_string()),
            request_count: Some(100),
            concurrency: Some(10),
            method: Some(HttpMethod::Get),
            body: None,
            request_template: None,
            request_alias: None,
            response_template: None,
            response_alias: None,
            load_curve,
            output: Some(crate::cli::command::OutputFormat::Table),
            output_file: None,
            config: None,
            headers: vec![],
            publish_url: None,
        }
    }

    #[test]
    fn output_flag_absent_is_none() {
        use clap::Parser as _;
        let cli = crate::cli::command::LoadTestRunCli::try_parse_from([
            "lmn",
            "run",
            "--host",
            "http://localhost:3000",
        ])
        .expect("parse failed");
        let crate::cli::command::LoadTestRunCli::Run(args) = cli else {
            panic!("expected Run variant");
        };
        assert!(args.output.is_none());
    }

    #[test]
    fn output_flag_accepts_json() {
        use clap::Parser as _;
        let cli = crate::cli::command::LoadTestRunCli::try_parse_from([
            "lmn",
            "run",
            "--host",
            "http://localhost:3000",
            "--output",
            "json",
        ])
        .expect("parse failed");
        let crate::cli::command::LoadTestRunCli::Run(args) = cli else {
            panic!("expected Run variant");
        };
        assert!(matches!(
            args.output,
            Some(crate::cli::command::OutputFormat::Json)
        ));
    }

    #[test]
    fn output_file_is_none_by_default() {
        use clap::Parser as _;
        let cli = crate::cli::command::LoadTestRunCli::try_parse_from([
            "lmn",
            "run",
            "--host",
            "http://localhost:3000",
        ])
        .expect("parse failed");
        let crate::cli::command::LoadTestRunCli::Run(args) = cli else {
            panic!("expected Run variant");
        };
        assert!(args.output_file.is_none());
    }

    #[test]
    fn config_flag_is_none_by_default() {
        use clap::Parser as _;
        let cli = crate::cli::command::LoadTestRunCli::try_parse_from([
            "lmn",
            "run",
            "--host",
            "http://localhost:3000",
        ])
        .expect("parse failed");
        let crate::cli::command::LoadTestRunCli::Run(args) = cli else {
            panic!("expected Run variant");
        };
        assert!(args.config.is_none());
    }

    #[test]
    fn config_flag_accepts_path() {
        use clap::Parser as _;
        let cli = crate::cli::command::LoadTestRunCli::try_parse_from([
            "lmn",
            "run",
            "--host",
            "http://localhost:3000",
            "--config",
            "lumen.yaml",
        ])
        .expect("parse failed");
        let crate::cli::command::LoadTestRunCli::Run(args) = cli else {
            panic!("expected Run variant");
        };
        assert_eq!(args.config, Some(PathBuf::from("lumen.yaml")));
    }

    #[test]
    fn config_short_flag_accepts_path() {
        use clap::Parser as _;
        let cli = crate::cli::command::LoadTestRunCli::try_parse_from([
            "lmn",
            "run",
            "--host",
            "http://localhost:3000",
            "-f",
            "ci.yaml",
        ])
        .expect("parse failed");
        let crate::cli::command::LoadTestRunCli::Run(args) = cli else {
            panic!("expected Run variant");
        };
        assert_eq!(args.config, Some(PathBuf::from("ci.yaml")));
    }

    #[test]
    fn try_from_run_args_without_curve_succeeds() {
        let resolved = RunArgsResolved::try_from(make_run_args(None));
        assert!(resolved.is_ok());
        let resolved = resolved.unwrap();
        assert!(matches!(resolved.execution, ExecutionMode::Fixed { .. }));
        assert!(resolved.thresholds.is_none());
    }

    #[test]
    fn try_from_run_args_with_nonexistent_curve_file_fails() {
        let result =
            RunArgsResolved::try_from(make_run_args(Some(PathBuf::from("nonexistent-curve.json"))));
        assert!(result.is_err());
    }

    #[test]
    fn try_from_run_args_with_valid_curve_file_succeeds() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let json = r#"{"stages":[{"duration":"10s","target_vus":5}]}"#;
        f.write_all(json.as_bytes()).unwrap();

        let result = RunArgsResolved::try_from(make_run_args(Some(f.path().to_path_buf())));
        assert!(result.is_ok());
        assert!(matches!(result.unwrap().execution, ExecutionMode::Curve(_)));
    }

    #[test]
    fn try_from_run_args_with_invalid_json_curve_fails() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"not json").unwrap();

        let result = RunArgsResolved::try_from(make_run_args(Some(f.path().to_path_buf())));
        assert!(result.is_err());
    }

    #[test]
    fn try_from_run_args_with_invalid_curve_vus_fails() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        // target_vus exceeds MAX_VUS (10_000)
        let json = r#"{"stages":[{"duration":"10s","target_vus":99999}]}"#;
        f.write_all(json.as_bytes()).unwrap();
        let result = RunArgsResolved::try_from(make_run_args(Some(f.path().to_path_buf())));
        assert!(result.is_err());
    }

    #[test]
    fn try_from_run_args_with_empty_stages_fails() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let json = r#"{"stages":[]}"#;
        f.write_all(json.as_bytes()).unwrap();
        let result = RunArgsResolved::try_from(make_run_args(Some(f.path().to_path_buf())));
        assert!(result.is_err());
    }

    #[test]
    fn host_missing_with_no_config_returns_error() {
        let mut args = make_run_args(None);
        args.host = None;
        let result = RunArgsResolved::try_from(args);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("host is required"),
            "expected host error, got: {msg}"
        );
    }

    #[test]
    fn host_from_config_used_when_cli_host_absent() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://from-config:8080\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args);
        assert!(result.is_ok(), "expected ok, got error");
        // RequestSpec::Single — extract host for assertion
        let spec = result.unwrap().request;
        let host = match spec {
            RequestSpec::Single { host, .. } => host,
            _ => panic!("expected Single variant"),
        };
        assert_eq!(host, "http://from-config:8080");
    }

    #[test]
    fn cli_host_takes_precedence_over_config_host() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://from-config:8080\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = Some("http://from-cli:9090".to_string());
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args);
        assert!(result.is_ok());
        let spec = result.unwrap().request;
        let host = match spec {
            RequestSpec::Single { host, .. } => host,
            _ => panic!("expected Single variant"),
        };
        assert_eq!(host, "http://from-cli:9090");
    }

    // ── new tests for config merging ──────────────────────────────────────────

    #[test]
    fn method_from_config_used_when_cli_is_default() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  method: post\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.method = None; // simulate user not passing --method
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        let spec = result.request;
        let method = match spec {
            RequestSpec::Single { method, .. } => method,
            _ => panic!("expected Single variant"),
        };
        assert!(
            matches!(method, lmn_core::command::HttpMethod::Post),
            "expected Post from config"
        );
    }

    #[test]
    fn output_format_from_config_used_when_cli_is_default() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  output: json\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.output = None; // simulate user not passing --output
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        assert!(
            matches!(result.output, OutputFormat::Json),
            "expected Json from config"
        );
    }

    #[test]
    fn output_file_from_config_used_when_cli_not_set() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  output_file: /tmp/out.json\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        assert_eq!(
            result.output_file,
            Some(PathBuf::from("/tmp/out.json")),
            "expected output_file from config"
        );
    }

    #[test]
    fn execution_stages_from_config_builds_curve_mode() {
        use std::io::Write;
        let yaml = b"run:\n  host: http://localhost:3000\nexecution:\n  stages:\n    - duration: 10s\n      target_vus: 5\n";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(yaml).unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        assert!(
            matches!(result.execution, ExecutionMode::Curve(_)),
            "expected Curve execution mode from config stages"
        );
    }

    #[test]
    fn invalid_method_string_in_config_returns_clear_error() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  method: FOO\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.method = None; // ensure config is the method source
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("unknown method 'foo'") || msg.contains("unknown method"),
            "expected method error, got: {msg}"
        );
        assert!(
            msg.contains("get, post, put, patch, delete"),
            "error should list valid methods, got: {msg}"
        );
    }

    #[test]
    fn invalid_output_string_in_config_returns_clear_error() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  output: FOO\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.output = None; // ensure config is the output source
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("unknown output format"),
            "expected output format error, got: {msg}"
        );
        assert!(
            msg.contains("table, json"),
            "error should list valid formats, got: {msg}"
        );
    }

    #[test]
    fn cli_header_flag_overrides_config_header() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(
            b"run:\n  host: http://localhost:3000\n  headers:\n    Authorization: config-token\n",
        )
        .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());
        args.headers = vec!["Authorization: Bearer cli-token".to_string()];

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        let headers = match result.request {
            RequestSpec::Single { headers, .. } => headers,
            _ => panic!("expected Single variant"),
        };
        let auth = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("Authorization"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            auth,
            Some("Bearer cli-token"),
            "CLI header should override config header"
        );
        // must be exactly one Authorization entry (no duplicates)
        let count = headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("Authorization"))
            .count();
        assert_eq!(
            count, 1,
            "expected exactly one Authorization header after override"
        );
    }

    #[test]
    fn config_headers_used_when_no_cli_headers() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(
            b"run:\n  host: http://localhost:3000\n  headers:\n    X-Custom: from-config\n",
        )
        .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());
        // args.headers is already vec![] from make_run_args

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        let headers = match result.request {
            RequestSpec::Single { headers, .. } => headers,
            _ => panic!("expected Single variant"),
        };
        let custom = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("X-Custom"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            custom,
            Some("from-config"),
            "config header should be present when no CLI headers"
        );
    }

    #[test]
    fn env_var_in_header_value_resolved() {
        use std::io::Write;
        unsafe { std::env::set_var("LUMEN_TEST_HEADER_TOKEN", "resolved-secret") };

        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  headers:\n    X-Token: \"${LUMEN_TEST_HEADER_TOKEN}\"\n").unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        let headers = match result.request {
            RequestSpec::Single { headers, .. } => headers,
            _ => panic!("expected Single variant"),
        };
        let token = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("X-Token"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            token,
            Some("resolved-secret"),
            "env var in header value should be resolved"
        );
    }

    #[test]
    fn too_many_cli_headers_returns_error() {
        // Build RunArgs with 65 --header strings (one over the 64 limit)
        let headers: Vec<String> = (0..65)
            .map(|i| format!("X-Custom-{i:02}: value{i}"))
            .collect();

        let mut args = make_run_args(None);
        args.headers = headers;

        let result = RunArgsResolved::try_from(args);
        assert!(result.is_err(), "expected error for too many headers");
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("too many headers"),
            "expected 'too many headers' error, got: {msg}"
        );
        assert!(
            msg.contains("64"),
            "error should mention the limit, got: {msg}"
        );
    }

    // ── scenario resolution tests ─────────────────────────────────────────────

    #[test]
    fn scenarios_config_resolves_to_scenarios_spec() {
        use std::io::Write;
        let yaml = b"
scenarios:
  - name: browse
    steps:
      - name: list_products
        host: https://api.example.com/products
execution:
  request_count: 50
  concurrency: 5
";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(yaml).unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        assert!(
            matches!(result.request, RequestSpec::Scenarios(_)),
            "expected Scenarios variant from config with scenarios"
        );
    }

    #[test]
    fn scenarios_global_headers_propagate_to_steps() {
        use std::io::Write;
        let yaml = b"
run:
  headers:
    Authorization: Bearer global-token
scenarios:
  - name: browse
    steps:
      - name: list_products
        host: https://api.example.com/products
";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(yaml).unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        let scenarios = match result.request {
            RequestSpec::Scenarios(s) => s,
            _ => panic!("expected Scenarios variant"),
        };
        assert_eq!(scenarios.len(), 1);
        let step = &scenarios[0].steps[0];
        let auth = step
            .plain_headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("Authorization"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            auth,
            Some("Bearer global-token"),
            "global header should propagate to step"
        );
    }

    #[test]
    fn scenarios_step_header_overrides_scenario_header() {
        use std::io::Write;
        let yaml = b"
scenarios:
  - name: checkout
    headers:
      X-Session: scenario-session
    steps:
      - name: add_to_cart
        host: https://api.example.com/cart
        method: post
        headers:
          X-Session: step-session
";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(yaml).unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        let scenarios = match result.request {
            RequestSpec::Scenarios(s) => s,
            _ => panic!("expected Scenarios variant"),
        };
        let step = &scenarios[0].steps[0];
        let session = step
            .plain_headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("X-Session"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            session,
            Some("step-session"),
            "step header should override scenario header"
        );
        let count = step
            .plain_headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("X-Session"))
            .count();
        assert_eq!(count, 1, "should not have duplicate X-Session headers");
    }

    #[test]
    fn scenarios_weight_and_on_step_failure_resolved() {
        use std::io::Write;
        let yaml = b"
scenarios:
  - name: checkout
    weight: 3
    on_step_failure: abort_iteration
    steps:
      - name: login
        host: https://api.example.com/auth
        method: post
  - name: browse
    weight: 1
    steps:
      - name: list
        host: https://api.example.com/products
";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(yaml).unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        let scenarios = match result.request {
            RequestSpec::Scenarios(s) => s,
            _ => panic!("expected Scenarios variant"),
        };
        assert_eq!(scenarios[0].weight, 3);
        assert!(matches!(
            scenarios[0].on_step_failure,
            OnStepFailure::AbortIteration
        ));
        assert_eq!(scenarios[1].weight, 1);
        assert!(matches!(
            scenarios[1].on_step_failure,
            OnStepFailure::Continue
        ));
    }

    #[test]
    fn merge_headers_into_overrides_existing() {
        use std::collections::HashMap;

        let mut base = vec![
            ("Authorization".to_string(), "old-token".to_string()),
            ("X-Custom".to_string(), "keep".to_string()),
        ];
        let mut incoming = HashMap::new();
        incoming.insert("authorization".to_string(), "new-token".to_string());

        merge_headers_into(&mut base, &incoming);

        assert_eq!(base.len(), 2);
        let auth = base
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("Authorization"))
            .map(|(_, v)| v.as_str());
        assert_eq!(auth, Some("new-token"));
        let custom = base
            .iter()
            .find(|(k, _)| k == "X-Custom")
            .map(|(_, v)| v.as_str());
        assert_eq!(custom, Some("keep"));
    }

    // ── publish_config merge tests ───────────────────────────────────────────

    #[test]
    fn publish_config_none_when_no_flag_and_no_yaml() {
        let resolved = RunArgsResolved::try_from(make_run_args(None)).unwrap();
        assert!(
            resolved.publish_config.is_none(),
            "expected None when neither CLI flag nor YAML publish section is set"
        );
    }

    #[test]
    fn publish_config_from_cli_flag_enables_and_sets_url() {
        let mut args = make_run_args(None);
        args.publish_url = Some("https://custom.example.com".into());

        let resolved = RunArgsResolved::try_from(args).unwrap();
        let pc = resolved
            .publish_config
            .expect("expected Some when --publish-url is set");
        assert_eq!(pc.enabled, Some(true));
        assert_eq!(pc.url.as_deref(), Some("https://custom.example.com"));
    }

    #[test]
    fn publish_config_from_yaml_preserves_yaml_fields() {
        use std::io::Write;
        let _env_lock = lock_lumen_api_url();
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\npublish:\n  enabled: true\n  url: https://yaml.example.com\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let resolved = RunArgsResolved::try_from(args).unwrap();
        let pc = resolved.publish_config.expect("expected Some from YAML");
        assert_eq!(pc.enabled, Some(true));
        assert_eq!(pc.url.as_deref(), Some("https://yaml.example.com"));
    }

    #[test]
    fn publish_config_cli_flag_overrides_yaml() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\npublish:\n  enabled: false\n  url: https://yaml.example.com\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());
        args.publish_url = Some("https://cli.example.com".into());

        let resolved = RunArgsResolved::try_from(args).unwrap();
        let pc = resolved
            .publish_config
            .expect("expected Some when CLI flag is set");
        assert_eq!(
            pc.enabled,
            Some(true),
            "CLI flag should force enabled=true even when YAML says false"
        );
        assert_eq!(
            pc.url.as_deref(),
            Some("https://cli.example.com"),
            "CLI flag URL should override YAML URL"
        );
    }

    /// RAII guard that sets an env var on creation and removes it on drop,
    /// ensuring cleanup even if the test panics.
    struct EnvGuard(&'static str);
    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            unsafe { std::env::set_var(key, value) };
            Self(key)
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var(self.0) };
        }
    }

    /// Serializes tests that read or write `LUMEN_API_URL`. Rust runs unit
    /// tests in parallel within one process, and the env var is process-global,
    /// so without this lock a test that sets `LUMEN_API_URL` can race with a
    /// test that asserts the YAML url is used.
    static LUMEN_API_URL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_lumen_api_url() -> std::sync::MutexGuard<'static, ()> {
        LUMEN_API_URL_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn publish_config_env_url_wins_over_yaml_url() {
        use std::io::Write;
        let _env_lock = lock_lumen_api_url();
        let _guard = EnvGuard::set("LUMEN_API_URL", "https://env.example.com");
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\npublish:\n  enabled: true\n  url: https://yaml.example.com\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let resolved = RunArgsResolved::try_from(args).unwrap();
        let pc = resolved.publish_config.expect("expected Some");
        assert_eq!(
            pc.url.as_deref(),
            Some("https://env.example.com"),
            "LUMEN_API_URL env should override YAML url"
        );
    }

    #[test]
    fn publish_config_cli_flag_wins_over_env_url() {
        let _env_lock = lock_lumen_api_url();
        let _guard = EnvGuard::set("LUMEN_API_URL", "https://env.example.com");

        let mut args = make_run_args(None);
        args.publish_url = Some("https://cli.example.com".into());

        let resolved = RunArgsResolved::try_from(args).unwrap();
        let pc = resolved.publish_config.expect("expected Some");
        assert_eq!(
            pc.url.as_deref(),
            Some("https://cli.example.com"),
            "--publish-url flag should override LUMEN_API_URL env"
        );
    }

    #[test]
    fn publish_config_yaml_disabled_without_flag_stays_disabled() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\npublish:\n  enabled: false\n")
            .unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let resolved = RunArgsResolved::try_from(args).unwrap();
        let pc = resolved
            .publish_config
            .expect("expected Some from YAML section");
        assert_eq!(
            pc.enabled,
            Some(false),
            "YAML enabled=false should be preserved when no CLI flag"
        );
    }
}
