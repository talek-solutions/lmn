#[derive(Debug)]
pub enum ConfigError {
    Fs(String),
    GeneralError,
    InvalidFormat(String),
    TemplateAlreadyExists(String),
    TemplateNotFound(String),
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
        }
    }
}

impl std::error::Error for ConfigError {}