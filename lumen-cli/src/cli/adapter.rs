use std::path::PathBuf;

use crate::cli::command::{ConfigureRequestArgs, ConfigureResponseArgs, HttpMethod, RunArgs};
use lumen_core::command::configure_template::{ConfigureTemplateCommand, TemplateKind};
use lumen_core::command::run::RunCommand;
use lumen_core::http::BodyFormat;
use lumen_core::command::Body;
use lumen_core::load_curve::LoadCurve;

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

impl TryFrom<RunArgs> for RunCommand {
    type Error = Box<dyn std::error::Error>;

    fn try_from(args: RunArgs) -> Result<Self, Self::Error> {
        const MAX_CURVE_FILE_BYTES: u64 = 1_048_576; // 1 MB

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

        Ok(RunCommand {
            host: args.host,
            request_count: args.request_count as usize,
            concurrency: args.concurrency as usize,
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
            threads: 1,
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
        }
    }

    #[test]
    fn try_from_run_args_without_curve_succeeds() {
        let cmd = RunCommand::try_from(make_run_args(None));
        assert!(cmd.is_ok());
        let cmd = cmd.unwrap();
        assert!(cmd.load_curve.is_none());
    }

    #[test]
    fn try_from_run_args_with_nonexistent_curve_file_fails() {
        let cmd = RunCommand::try_from(make_run_args(Some(PathBuf::from("nonexistent-curve.json"))));
        assert!(cmd.is_err());
    }

    #[test]
    fn try_from_run_args_with_valid_curve_file_succeeds() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let json = r#"{"stages":[{"duration":"10s","target_vus":5}]}"#;
        f.write_all(json.as_bytes()).unwrap();

        let cmd = RunCommand::try_from(make_run_args(Some(f.path().to_path_buf())));
        assert!(cmd.is_ok());
        assert!(cmd.unwrap().load_curve.is_some());
    }

    #[test]
    fn try_from_run_args_with_invalid_json_curve_fails() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"not json").unwrap();

        let cmd = RunCommand::try_from(make_run_args(Some(f.path().to_path_buf())));
        assert!(cmd.is_err());
    }

    #[test]
    fn try_from_run_args_with_invalid_curve_vus_fails() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        // target_vus exceeds MAX_VUS (10_000)
        let json = r#"{"stages":[{"duration":"10s","target_vus":99999}]}"#;
        f.write_all(json.as_bytes()).unwrap();
        let cmd = RunCommand::try_from(make_run_args(Some(f.path().to_path_buf())));
        assert!(cmd.is_err());
    }

    #[test]
    fn try_from_run_args_with_empty_stages_fails() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let json = r#"{"stages":[]}"#;
        f.write_all(json.as_bytes()).unwrap();
        let cmd = RunCommand::try_from(make_run_args(Some(f.path().to_path_buf())));
        assert!(cmd.is_err());
    }
}
