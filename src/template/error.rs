use std::io;

#[derive(Debug)]
pub enum TemplateError {
    Io(io::Error),
    InvalidJson(serde_json::Error),
    UnknownPlaceholder(String),
    CircularReference(Vec<String>),
    InvalidConstraint(String),
    MissingDefinition(String),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "failed to read template file: {e}"),
            Self::InvalidJson(e) => write!(f, "template is not valid JSON: {e}"),
            Self::UnknownPlaceholder(name) => {
                write!(f, "placeholder '{{{{{name}}}}}' is not defined in the template")
            }
            Self::CircularReference(cycle) => {
                write!(f, "circular reference: {}", cycle.join(" -> "))
            }
            Self::InvalidConstraint(msg) => write!(f, "invalid constraint: {msg}"),
            Self::MissingDefinition(name) => {
                write!(f, "placeholder '{name}' referenced in composition but not defined")
            }
        }
    }
}

impl std::error::Error for TemplateError {}

impl From<io::Error> for TemplateError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for TemplateError {
    fn from(e: serde_json::Error) -> Self {
        Self::InvalidJson(e)
    }
}
