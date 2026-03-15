use crate::command::run::RunStats;
use crate::command::{Body, Command};
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


impl Command for ConfigureTemplateCommand {
    fn execute(self) -> Result<Option<RunStats>, Box<dyn Error>> {
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
            .map_err(|e| Box::new(e) as Box<dyn Error>)?;
        Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_kind_dir() {
        assert_eq!(TemplateKind::Request.dir(), "requests");
    }

    #[test]
    fn response_kind_dir() {
        assert_eq!(TemplateKind::Response.dir(), "responses");
    }

    #[test]
    fn create_file_writes_content() {
        let file_name = "__test_create_file_writes.json".to_string();
        let path = PathBuf::from(TEMPLATE_ROOT_DIR).join("requests").join(&file_name);
        let _ = std::fs::remove_file(&path);

        let result = create_file("requests", file_name.clone(), r#"{"ok":true}"#.to_string());
        assert!(result.is_ok());
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn create_file_rejects_duplicate() {
        let file_name = "__test_create_file_duplicate.json".to_string();
        let path = PathBuf::from(TEMPLATE_ROOT_DIR).join("requests").join(&file_name);
        let _ = std::fs::remove_file(&path);

        create_file("requests", file_name.clone(), "{}".to_string()).unwrap();
        let result = create_file("requests", file_name.clone(), "{}".to_string());
        assert!(matches!(result, Err(ConfigError::TemplateAlreadyExists(_))));
        let _ = std::fs::remove_file(&path);
    }
}
