use std::path::PathBuf;

use crate::cli::command::{ConfigureRequestArgs, ConfigureResponseArgs, HttpMethod, RunArgs};
use lumen_core::command::configure_template::{ConfigureTemplateCommand, TemplateKind};
use lumen_core::command::run::RunCommand;
use lumen_core::http::BodyFormat;
use lumen_core::command::Body;
use lumen_core::load_curve::LoadCurve;
use lumen_core::config::{LumenConfig, parse_config};
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
    pub host: String,
    pub request_count: usize,
    pub concurrency: usize,
    pub method: lumen_core::command::HttpMethod,
    pub body: Option<Body>,
    pub template_path: Option<PathBuf>,
    pub response_template_path: Option<PathBuf>,
    pub load_curve: Option<LoadCurve>,
    pub sample_threshold: usize,
    pub result_buffer: usize,
    /// Threshold rules sourced from the config file.
    /// `None` when no config was supplied or the config has no `thresholds` section.
    pub thresholds: Option<Vec<Threshold>>,
}

impl RunArgsResolved {
    /// Converts a `RunArgsResolved` into a `RunCommand` (the core engine type).
    pub fn into_run_command(self) -> RunCommand {
        RunCommand {
            host: self.host,
            request_count: self.request_count,
            concurrency: self.concurrency,
            method: self.method,
            body: self.body,
            template_path: self.template_path,
            response_template_path: self.response_template_path,
            load_curve: self.load_curve,
            sample_threshold: self.sample_threshold,
            result_buffer: self.result_buffer,
        }
    }
}

// ── TryFrom<RunArgs> for RunArgsResolved ─────────────────────────────────────

impl TryFrom<RunArgs> for RunArgsResolved {
    type Error = Box<dyn std::error::Error>;

    fn try_from(args: RunArgs) -> Result<Self, Self::Error> {
        const MAX_CURVE_FILE_BYTES: u64 = 1_048_576; // 1 MB

        // Load and parse config file when --config/-f was supplied.
        let cfg: Option<LumenConfig> = args
            .config
            .as_ref()
            .map(|path| {
                let file_size = std::fs::metadata(path)
                    .map_err(|e| format!("cannot access config file '{}': {e}", path.display()))?
                    .len();
                if file_size > MAX_CURVE_FILE_BYTES {
                    return Err(format!(
                        "config file '{}' exceeds 1 MB limit ({} bytes)",
                        path.display(),
                        file_size
                    ));
                }
                let contents = std::fs::read_to_string(path)
                    .map_err(|e| format!("failed to read config '{}': {e}", path.display()))?;
                parse_config(&contents)
                    .map_err(|e| format!("failed to parse config '{}': {e}", path.display()))
            })
            .transpose()
            .map_err(|e: String| Box::<dyn std::error::Error>::from(e))?;

        // Apply config-file defaults where CLI did not supply a value.
        // Clap always supplies a default for --request-count and --concurrency,
        // so we cannot distinguish "user typed -R 100" from "clap used the default".
        // The merge strategy used here is: config values act as the baseline;
        // clap defaults (which are always present) overwrite them. This means
        // --config values for request_count / concurrency are only meaningful when
        // the user also does NOT pass -R / -C on the CLI. This is documented in
        // CLI.md and is intentional — the CLI is the authoritative layer.
        let (request_count, concurrency) = if let Some(ref c) = cfg {
            let rc = c.run.as_ref().and_then(|r| r.requests).unwrap_or(args.request_count as usize);
            let con = c.run.as_ref().and_then(|r| r.concurrency).unwrap_or(args.concurrency as usize);
            (rc, con)
        } else {
            (args.request_count as usize, args.concurrency as usize)
        };

        let load_curve = args
            .load_curve
            .map(|path| {
                let file_size = std::fs::metadata(&path)
                    .map_err(|e| format!("cannot access load curve file '{}': {e}", path.display()))?
                    .len();
                if file_size > MAX_CURVE_FILE_BYTES {
                    return Err(format!(
                        "load curve file '{}' exceeds 1 MB limit ({} bytes)",
                        path.display(),
                        file_size
                    ));
                }
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("failed to read load curve file '{}': {e}", path.display()))?;
                let curve = content.parse::<LoadCurve>()
                    .map_err(|e| format!("failed to parse load curve file '{}': {e}", path.display()))?;
                curve.validate()
                    .map_err(|e| format!("invalid load curve '{}': {e}", path.display()))?;
                Ok(curve)
            })
            .transpose()
            .map_err(|e: String| Box::<dyn std::error::Error>::from(e))?;

        let thresholds = cfg.and_then(|c| c.thresholds);

        Ok(RunArgsResolved {
            host: args.host,
            request_count,
            concurrency,
            method: args.method.into(),
            body: args.body.map(|s| Body::Formatted {
                content: s,
                format: BodyFormat::Json,
            }),
            template_path: args
                .request_template
                .or_else(|| args.request_alias.map(resolve_alias("requests"))),
            response_template_path: args
                .response_template
                .or_else(|| args.response_alias.map(resolve_alias("responses"))),
            load_curve,
            sample_threshold: args.sample_threshold,
            result_buffer: args.result_buffer,
            thresholds,
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
            host: "http://localhost:3000".to_string(),
            request_count: 100,
            concurrency: 10,
            method: HttpMethod::Get,
            body: None,
            request_template: None,
            request_alias: None,
            response_template: None,
            response_alias: None,
            load_curve,
            sample_threshold: 50,
            result_buffer: 100_000,
            output: crate::cli::command::OutputFormat::Table,
            output_file: None,
            config: None,
        }
    }

    #[test]
    fn output_flag_defaults_to_table() {
        use clap::Parser as _;
        let cli = crate::cli::command::LoadTestRunCli::try_parse_from([
            "lumen", "run", "--host", "http://localhost:3000",
        ])
        .expect("parse failed");
        let crate::cli::command::LoadTestRunCli::Run(args) = cli else {
            panic!("expected Run variant");
        };
        assert!(matches!(args.output, crate::cli::command::OutputFormat::Table));
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
        assert!(matches!(args.output, crate::cli::command::OutputFormat::Json));
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
        assert!(resolved.load_curve.is_none());
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
        assert!(result.unwrap().load_curve.is_some());
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
}
