/// Central registry of all tracing span names used in lumen.
///
/// Use the associated constants directly or call [`SpanName::for_request`] /
/// [`SpanName::for_template`] to get the right name in a given context.
pub struct SpanName;

impl SpanName {
    const PREFIX: &'static str = "lumen";

    /// Root span covering the entire lumen run.
    pub const RUN: &'static str = "lumen.run";

    /// Span covering the full parse + validation of a request template file.
    pub const TEMPLATE_PARSE: &'static str = "lumen.template.parse";

    /// Span covering pre-generation of all request bodies from a template.
    pub const TEMPLATE_RENDER: &'static str = "lumen.template.render";

    /// Span covering placeholder validation during template parsing.
    pub const TEMPLATE_VALIDATE_PLACEHOLDERS: &'static str = "lumen.template.validate_placeholders";

    /// Span covering circular reference detection during template parsing.
    pub const TEMPLATE_CHECK_CIRCULAR_REFS: &'static str = "lumen.template.check_circular_refs";

    /// Span covering the full parse + validation of a response template file.
    pub const RESPONSE_TEMPLATE_PARSE: &'static str = "lumen.response_template.parse";

    /// Span covering the entire request execution phase across all workers.
    pub const REQUESTS: &'static str = "lumen.requests";

    /// Span covering a single outbound HTTP request.
    pub const REQUEST: &'static str = "lumen.request";

    /// Returns the shared top-level prefix used for all span names.
    pub fn prefix() -> &'static str {
        Self::PREFIX
    }
}
