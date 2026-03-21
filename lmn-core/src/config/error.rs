#[derive(Debug)]
pub enum ConfigError {
    Fs(String),
    GeneralError,
    InvalidFormat(String),
    TemplateAlreadyExists(String),
    TemplateNotFound(String),
    /// YAML parsing failed on a `LumenConfig` file.
    YamlParseError(serde_yml::Error),
    /// The config was parseable but contained invalid values.
    ValidationError(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Fs(e) => write!(f, "FS Error: {}", e),
            Self::GeneralError => write!(f, "General FS Error"),
            Self::InvalidFormat(format) => write!(f, "Invalid format: {}", format),
            Self::TemplateAlreadyExists(name) => {
                write!(f, "Template \"{}\" already exists", name)
            },
            Self::TemplateNotFound(name) => write!(f, "Template \"{}\" not found", name),
            Self::YamlParseError(e) => write!(f, "YAML parse error: {}", e),
            Self::ValidationError(msg) => write!(f, "Config validation error: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_fs() {
        assert_eq!(ConfigError::Fs("file.json".into()).to_string(), "FS Error: file.json");
    }

    #[test]
    fn display_general_error() {
        assert_eq!(ConfigError::GeneralError.to_string(), "General FS Error");
    }

    #[test]
    fn display_invalid_format() {
        assert_eq!(ConfigError::InvalidFormat("bad".into()).to_string(), "Invalid format: bad");
    }

    #[test]
    fn display_template_already_exists() {
        assert_eq!(
            ConfigError::TemplateAlreadyExists("foo".into()).to_string(),
            "Template \"foo\" already exists"
        );
    }

    #[test]
    fn display_template_not_found() {
        assert_eq!(
            ConfigError::TemplateNotFound("bar".into()).to_string(),
            "Template \"bar\" not found"
        );
    }
}