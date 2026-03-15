use std::path::PathBuf;

use crate::cli::command::{ConfigureRequestArgs, ConfigureResponseArgs, RunArgs};
use crate::command::configure_template::{ConfigureTemplateCommand, TemplateKind};
use crate::command::run::{BodyFormat, RunCommand};
use crate::command::Body;

impl From<RunArgs> for RunCommand {
    fn from(args: RunArgs) -> Self {
        RunCommand {
            host: args.host,
            threads: args.threads as usize,
            request_count: args.request_count as usize,
            concurrency: args.concurrency as usize,
            method: args.method,
            body: args.body.map(|s| Body::Formatted {
                content: s,
                format: BodyFormat::Json,
            }),
            template_path: args
                .template
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
