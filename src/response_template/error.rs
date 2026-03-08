use std::io;

#[derive(Debug)]
pub enum ResponseTemplateError {
    Io(io::Error),
    InvalidJson(serde_json::Error),
    InvalidFieldType(String),
}

impl std::fmt::Display for ResponseTemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "failed to read response template file: {e}"),
            Self::InvalidJson(e) => write!(f, "response template is not valid JSON: {e}"),
            Self::InvalidFieldType(t) => {
                write!(f, "unsupported response field type '{t}' — expected STRING or FLOAT")
            }
        }
    }
}

impl std::error::Error for ResponseTemplateError {}

impl From<io::Error> for ResponseTemplateError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for ResponseTemplateError {
    fn from(e: serde_json::Error) -> Self {
        Self::InvalidJson(e)
    }
}
