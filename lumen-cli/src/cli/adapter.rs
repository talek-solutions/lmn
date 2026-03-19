use std::path::PathBuf;

use crate::cli::command::{ConfigureRequestArgs, ConfigureResponseArgs, HttpMethod, OutputFormat, RunArgs};
use lumen_core::command::configure_template::{ConfigureTemplateCommand, TemplateKind};
use lumen_core::command::run::{ExecutionMode, RequestSpec, RunCommand, SamplingConfig};
use lumen_core::http::BodyFormat;
use lumen_core::command::Body;
use lumen_core::load_curve::LoadCurve;
use lumen_core::config::{ExecutionConfig, LumenConfig, parse_config};
use lumen_core::threshold::Threshold;

impl From<HttpMethod> for lumen_core::command::HttpMethod {
    fn from(m: HttpMethod) -> Self {
        match m {
            HttpMethod::Get    => lumen_core::command::HttpMethod::Get,
            HttpMethod::Post   => lumen_core::command::HttpMethod::Post,
            HttpMethod::Put    => lumen_core::command::HttpMethod::Put,
            HttpMethod::Patch  => lumen_core::command::HttpMethod::Patch,
            HttpMethod::Delete => lumen_core::command::HttpMethod::Delete,
        }
    }
}

// ── method string parsing ─────────────────────────────────────────────────────

fn parse_method_str(s: &str) -> Result<lumen_core::command::HttpMethod, String> {
    match s.to_lowercase().as_str() {
        "get"    => Ok(lumen_core::command::HttpMethod::Get),
        "post"   => Ok(lumen_core::command::HttpMethod::Post),
        "put"    => Ok(lumen_core::command::HttpMethod::Put),
        "patch"  => Ok(lumen_core::command::HttpMethod::Patch),
        "delete" => Ok(lumen_core::command::HttpMethod::Delete),
        other => Err(format!(
            "unknown method '{other}' in config — expected one of: get, post, put, patch, delete"
        )),
    }
}

// ── output format string parsing ──────────────────────────────────────────────

fn parse_output_str(s: &str) -> Result<OutputFormat, String> {
    match s.to_lowercase().as_str() {
        "table" => Ok(OutputFormat::Table),
        "json"  => Ok(OutputFormat::Json),
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
    pub request: lumen_core::command::run::RequestSpec,
    pub execution: lumen_core::command::run::ExecutionMode,
    pub sampling: lumen_core::command::run::SamplingConfig,
    /// Threshold rules sourced from the config file.
    /// `None` when no config was supplied or the config has no `thresholds` section.
    pub thresholds: Option<Vec<Threshold>>,
    pub output: OutputFormat,
    pub output_file: Option<PathBuf>,
}

impl RunArgsResolved {
    /// Converts a `RunArgsResolved` into a `RunCommand` (the core engine type).
    pub fn into_run_command(self) -> RunCommand {
        RunCommand {
            request: self.request,
            execution: self.execution,
            sampling: self.sampling,
        }
    }
}

// ── TryFrom<RunArgs> for RunArgsResolved ─────────────────────────────────────

impl TryFrom<RunArgs> for RunArgsResolved {
    type Error = Box<dyn std::error::Error>;

    fn try_from(args: RunArgs) -> Result<Self, Self::Error> {
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
                let contents = String::from_utf8(bytes)
                    .map_err(|e| format!("config file '{}' is not valid UTF-8: {e}", path.display()))?;
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
        let method: lumen_core::command::HttpMethod = if let Some(m) = args.method {
            m.into()
        } else if let Some(s) = cfg.as_ref().and_then(|c| c.run.as_ref()?.method.as_deref()) {
            parse_method_str(s).map_err(|e| Box::<dyn std::error::Error>::from(e))?
        } else {
            lumen_core::command::HttpMethod::Get
        };

        // ── output format ─────────────────────────────────────────────────────
        let output: OutputFormat = if let Some(o) = args.output {
            o
        } else if let Some(s) = cfg.as_ref().and_then(|c| c.run.as_ref()?.output.as_deref()) {
            parse_output_str(s).map_err(|e| Box::<dyn std::error::Error>::from(e))?
        } else {
            OutputFormat::Table
        };

        // ── output_file: CLI Some wins; else config value; else None ──────────
        let output_file: Option<PathBuf> = args.output_file
            .or_else(|| {
                cfg.as_ref()
                    .and_then(|c| c.run.as_ref()?.output_file.as_deref().map(PathBuf::from))
            });

        // ── sample_threshold ──────────────────────────────────────────────────
        let sample_threshold: usize = args.sample_threshold
            .or_else(|| cfg.as_ref().and_then(|c| c.run.as_ref()?.sample_threshold))
            .unwrap_or(50);

        // ── result_buffer ─────────────────────────────────────────────────────
        let result_buffer: usize = args.result_buffer
            .or_else(|| cfg.as_ref().and_then(|c| c.run.as_ref()?.result_buffer))
            .unwrap_or(100_000);

        // ── request_count / concurrency ───────────────────────────────────────
        let request_count: usize = args.request_count
            .map(|v| v as usize)
            .or_else(|| cfg.as_ref().and_then(|c| c.execution.as_ref()?.request_count))
            .unwrap_or(100);

        let concurrency: usize = args.concurrency
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
                ).into());
            }
            let content = String::from_utf8(bytes)
                .map_err(|e| format!("load curve file '{}' is not valid UTF-8: {e}", path.display()))?;
            let curve = content.parse::<LoadCurve>()
                .map_err(|e| format!("failed to parse load curve file '{}': {e}", path.display()))?;
            curve.validate()
                .map_err(|e| format!("invalid load curve '{}': {e}", path.display()))?;
            ExecutionMode::Curve(curve)
        } else if let Some(ref c) = cfg {
            let exec_cfg: Option<&ExecutionConfig> = c.execution.as_ref();
            if exec_cfg.and_then(|e| e.stages.as_ref()).is_some() {
                let exec = exec_cfg.unwrap().clone();
                let curve = LoadCurve::try_from(exec)
                    .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
                ExecutionMode::Curve(curve)
            } else {
                ExecutionMode::Fixed { request_count, concurrency }
            }
        } else {
            ExecutionMode::Fixed { request_count, concurrency }
        };

        let thresholds = cfg.as_ref().and_then(|c| c.thresholds.clone());

        // Resolve host: CLI flag > config run.host > error.
        let host = args.host
            .or_else(|| cfg.as_ref().and_then(|c| c.run.as_ref()?.host.clone()))
            .ok_or_else(|| "host is required: set -H or run.host in config file")?;

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

        Ok(RunArgsResolved {
            request: RequestSpec {
                host,
                method,
                body: args.body.map(|s| Body::Formatted {
                    content: s,
                    format: BodyFormat::Json,
                }),
                template_path,
                response_template_path,
            },
            execution,
            sampling: SamplingConfig {
                sample_threshold,
                result_buffer,
            },
            thresholds,
            output,
            output_file,
        })
    }
}

impl From<ConfigureRequestArgs> for ConfigureTemplateCommand {
    fn from(args: ConfigureRequestArgs) -> Self {
        ConfigureTemplateCommand {
            body: args
                .body
                .map(|s| Body::Formatted { content: s, format: BodyFormat::Json }),
            template_path: args.template_path,
            alias: args.alias,
            kind: TemplateKind::Request,
        }
    }
}

impl From<ConfigureResponseArgs> for ConfigureTemplateCommand {
    fn from(args: ConfigureResponseArgs) -> Self {
        ConfigureTemplateCommand {
            body: args
                .body
                .map(|s| Body::Formatted { content: s, format: BodyFormat::Json }),
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
    use lumen_core::command::Command;

    #[tokio::test]
    async fn execute_creates_file_from_body() {
        let alias = "__test_exec_body";
        let path = std::path::PathBuf::from(".templates/requests").join(format!("{}.json", alias));
        let _ = std::fs::remove_file(&path);
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
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
            fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
        }
        let _cleanup = Cleanup(path.clone());

        let make_cmd = || ConfigureTemplateCommand::from(ConfigureRequestArgs {
            alias: alias.to_string(),
            body: Some("{}".to_string()),
            template_path: None,
        });
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
            sample_threshold: Some(50),
            result_buffer: Some(100_000),
            output: Some(crate::cli::command::OutputFormat::Table),
            output_file: None,
            config: None,
        }
    }

    #[test]
    fn output_flag_absent_is_none() {
        use clap::Parser as _;
        let cli = crate::cli::command::LoadTestRunCli::try_parse_from([
            "lumen", "run", "--host", "http://localhost:3000",
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
            "lumen", "run", "--host", "http://localhost:3000", "--output", "json",
        ])
        .expect("parse failed");
        let crate::cli::command::LoadTestRunCli::Run(args) = cli else {
            panic!("expected Run variant");
        };
        assert!(matches!(args.output, Some(crate::cli::command::OutputFormat::Json)));
    }

    #[test]
    fn output_file_is_none_by_default() {
        use clap::Parser as _;
        let cli = crate::cli::command::LoadTestRunCli::try_parse_from([
            "lumen", "run", "--host", "http://localhost:3000",
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
            "lumen", "run", "--host", "http://localhost:3000",
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
            "lumen", "run", "--host", "http://localhost:3000", "--config", "lumen.yaml",
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
            "lumen", "run", "--host", "http://localhost:3000", "-f", "ci.yaml",
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
        let result = RunArgsResolved::try_from(make_run_args(Some(PathBuf::from("nonexistent-curve.json"))));
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
        assert!(msg.contains("host is required"), "expected host error, got: {msg}");
    }

    #[test]
    fn host_from_config_used_when_cli_host_absent() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://from-config:8080\n").unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args);
        assert!(result.is_ok(), "expected ok, got error");
        assert_eq!(result.unwrap().request.host, "http://from-config:8080");
    }

    #[test]
    fn cli_host_takes_precedence_over_config_host() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://from-config:8080\n").unwrap();

        let mut args = make_run_args(None);
        args.host = Some("http://from-cli:9090".to_string());
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().request.host, "http://from-cli:9090");
    }

    // ── new tests for config merging ──────────────────────────────────────────

    #[test]
    fn method_from_config_used_when_cli_is_default() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  method: post\n").unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.method = None; // simulate user not passing --method
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        assert!(
            matches!(result.request.method, lumen_core::command::HttpMethod::Post),
            "expected Post from config"
        );
    }

    #[test]
    fn output_format_from_config_used_when_cli_is_default() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  output: json\n").unwrap();

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
        f.write_all(b"run:\n  host: http://localhost:3000\n  output_file: /tmp/out.json\n").unwrap();

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
    fn sample_threshold_from_config() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  sample_threshold: 200\n").unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.sample_threshold = None; // simulate user not passing --sample-threshold
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        assert_eq!(result.sampling.sample_threshold, 200);
    }

    #[test]
    fn result_buffer_from_config() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  result_buffer: 50000\n").unwrap();

        let mut args = make_run_args(None);
        args.host = None;
        args.result_buffer = None; // simulate user not passing --result-buffer
        args.config = Some(f.path().to_path_buf());

        let result = RunArgsResolved::try_from(args).expect("should succeed");
        assert_eq!(result.sampling.result_buffer, 50000);
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
        f.write_all(b"run:\n  host: http://localhost:3000\n  method: FOO\n").unwrap();

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
        assert!(msg.contains("get, post, put, patch, delete"), "error should list valid methods, got: {msg}");
    }

    #[test]
    fn invalid_output_string_in_config_returns_clear_error() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"run:\n  host: http://localhost:3000\n  output: FOO\n").unwrap();

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
        assert!(msg.contains("table, json"), "error should list valid formats, got: {msg}");
    }
}
