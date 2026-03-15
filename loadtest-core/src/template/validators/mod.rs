pub mod float;
pub mod object;
pub mod string;

use crate::template::definition::{RawTemplateDef, TemplateDef};
use crate::template::error::TemplateError;

use float::FloatValidator;
use object::ObjectValidator;
use string::StringValidator;

pub trait Validator {
    fn validate(self, name: &str) -> Result<TemplateDef, TemplateError>;
}

pub fn validate(raw: RawTemplateDef, name: &str) -> Result<TemplateDef, TemplateError> {
    match raw {
        RawTemplateDef::String { exact, min, max, details } => {
            StringValidator { exact, min, max, details }.validate(name)
        }
        RawTemplateDef::Float { exact, min, max, details } => {
            FloatValidator { exact, min, max, details }.validate(name)
        }
        RawTemplateDef::Object { composition } => ObjectValidator { composition }.validate(name),
    }
}