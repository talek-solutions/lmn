use std::path::PathBuf;

use crate::cli::command::{ConfigureRequestArgs, ConfigureResponseArgs, HttpMethod, RunArgs};
use loadtest_core::command::configure_template::{ConfigureTemplateCommand, TemplateKind};
use loadtest_core::command::run::RunCommand;
use loadtest_core::http::BodyFormat;
use loadtest_core::command::Body;

impl From<HttpMethod> for loadtest_core::command::HttpMethod {
    fn from(m: HttpMethod) -> Self {
        match m {
            HttpMethod::Get    => loadtest_core::command::HttpMethod::Get,
            HttpMethod::Post   => loadtest_core::command::HttpMethod::Post,
            HttpMethod::Put    => loadtest_core::command::HttpMethod::Put,
            HttpMethod::Patch  => loadtest_core::command::HttpMethod::Patch,
            HttpMethod::Delete => loadtest_core::command::HttpMethod::Delete,
        }
    }
}

impl From<RunArgs> for RunCommand {
    fn from(args: RunArgs) -> Self {
        RunCommand {
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
        }
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
    use loadtest_core::command::Command;

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
}
