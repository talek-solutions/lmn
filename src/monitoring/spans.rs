/// Central registry of all tracing span names used in loadtest.
///
/// Use the associated constants directly or call [`SpanName::for_request`] /
/// [`SpanName::for_template`] to get the right name in a given context.
pub struct SpanName;

impl SpanName {
    const PREFIX: &'static str = "loadtest";

    /// Span covering the full parse + validation of a request template file.
    pub const TEMPLATE_PARSE: &'static str = "loadtest.template.parse";

    /// Span covering pre-generation of all request bodies from a template.
    pub const TEMPLATE_RENDER: &'static str = "loadtest.template.render";

    /// Span covering a single outbound HTTP request.
    pub const REQUEST: &'static str = "loadtest.request";

    /// Returns the shared top-level prefix used for all span names.
    pub fn prefix() -> &'static str {
        Self::PREFIX
    }
}
