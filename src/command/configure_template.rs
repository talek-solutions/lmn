use crate::cli::command::{ConfigureRequestArgs, ConfigureResponseArgs};
use crate::command::{Body, Command};
use crate::command::run::BodyFormat;
use crate::config::error::ConfigError;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

// ── Constant definition ───────────────────────────────────────────────────────

const TEMPLATE_ROOT_DIR: &str = ".templates";

// ── Kind definition ───────────────────────────────────────────────────────────

pub enum TemplateKind {
    Request,
    Response,
}

impl TemplateKind {
    fn dir(&self) -> &'static str {
        match self {
            TemplateKind::Request => "requests",
            TemplateKind::Response => "responses",
        }
    }
}

// ── Command definition ────────────────────────────────────────────────────────

pub struct ConfigureTemplateCommand {
    pub alias: String,
    pub body: Option<Body>,
    pub template_path: Option<PathBuf>,
    pub kind: TemplateKind,
}

impl From<ConfigureRequestArgs> for ConfigureTemplateCommand {
    fn from(args: ConfigureRequestArgs) -> Self {
        ConfigureTemplateCommand {
            body: args.body.map(|s| Body::Formatted { content: s, format: BodyFormat::Json }),
            template_path: args.template_path,
            alias: args.alias,
            kind: TemplateKind::Request,
        }
    }
}

impl From<ConfigureResponseArgs> for ConfigureTemplateCommand {
    fn from(args: ConfigureResponseArgs) -> Self {
        ConfigureTemplateCommand {
            body: args.body.map(|s| Body::Formatted { content: s, format: BodyFormat::Json }),
            template_path: args.template_path,
            alias: args.alias,
            kind: TemplateKind::Response,
        }
    }
}

impl Command for ConfigureTemplateCommand {
    fn execute(self) -> Result<(), Box<dyn Error>> {
        let content: String = match (self.body, self.template_path) {
            (Some(body), _) => body.into(),
            (_, Some(path)) => {
                let raw = std::fs::read_to_string(&path).map_err(|_| {
                    ConfigError::TemplateNotFound(path.to_string_lossy().into_owned())
                })?;
                serde_json::from_str::<serde_json::Value>(&raw)
                    .map_err(|_| ConfigError::InvalidFormat(path.to_string_lossy().into_owned()))?;
                raw
            }
            (None, None) => return Err(Box::new(ConfigError::GeneralError)),
        };

        let mut file_name = PathBuf::from(&self.alias);
        file_name.set_extension("json");
        let full_file_alias = file_name
            .to_str()
            .ok_or_else(|| Box::<dyn Error>::from(ConfigError::InvalidFormat(self.alias)))?;

        create_file(self.kind.dir(), full_file_alias.to_string(), content)
            .map_err(|e| Box::new(e) as Box<dyn Error>)
    }
}

fn create_file(sub_dir: &str, file_name: String, content: String) -> Result<(), ConfigError> {
    let dir = PathBuf::from(TEMPLATE_ROOT_DIR).join(sub_dir);
    std::fs::create_dir_all(&dir).map_err(|_| ConfigError::Fs(file_name.clone()))?;

    let file_path = dir.join(&file_name);
    if file_path.exists() {
        return Err(ConfigError::TemplateAlreadyExists(file_name));
    }

    let mut file = File::create(&file_path).map_err(|_| ConfigError::Fs(file_name.clone()))?;
    file.write_all(content.as_bytes()).map_err(|_| ConfigError::Fs(file_name))
}
