/// Central registry of all tracing span names used in loadtest.
///
/// Use the associated constants directly or call [`SpanName::for_request`] /
/// [`SpanName::for_template`] to get the right name in a given context.
pub struct SpanName;

impl SpanName {
    const PREFIX: &'static str = "loadtest";

    /// Root span covering the entire loadtest run.
    pub const RUN: &'static str = "loadtest.run";

    /// Span covering the full parse + validation of a request template file.
    pub const TEMPLATE_PARSE: &'static str = "loadtest.template.parse";

    /// Span covering pre-generation of all request bodies from a template.
    pub const TEMPLATE_RENDER: &'static str = "loadtest.template.render";

    /// Span covering placeholder validation during template parsing.
    pub const TEMPLATE_VALIDATE_PLACEHOLDERS: &'static str = "loadtest.template.validate_placeholders";

    /// Span covering circular reference detection during template parsing.
    pub const TEMPLATE_CHECK_CIRCULAR_REFS: &'static str = "loadtest.template.check_circular_refs";

    /// Span covering the full parse + validation of a response template file.
    pub const RESPONSE_TEMPLATE_PARSE: &'static str = "loadtest.response_template.parse";

    /// Span covering the entire request execution phase across all workers.
    pub const REQUESTS: &'static str = "loadtest.requests";

    /// Span covering a single outbound HTTP request.
    pub const REQUEST: &'static str = "loadtest.request";

    /// Returns the shared top-level prefix used for all span names.
    pub fn prefix() -> &'static str {
        Self::PREFIX
    }
}
