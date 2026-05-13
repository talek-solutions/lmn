use std::fmt;

// ── PublishError ──────────────────────────────────────────────────────────────

/// Errors that can occur while publishing a run report to the platform.
///
/// Variants are ordered roughly by lifecycle: configuration → transport →
/// server response. Error messages are phrased for display directly to the
/// end user (CLI operator) and MUST NOT contain the API key, URL userinfo,
/// or request/response headers.
#[derive(Debug)]
pub enum PublishError {
    /// The `--publish-url` or `LUMEN_API_URL` value could not be parsed as a
    /// URL, or did not meet the HTTPS-only policy (HTTPS is required except
    /// for loopback hosts).
    InvalidUrl(String),

    /// The serialized payload exceeds the hard 5 MB limit. Contains the
    /// actual serialized size in bytes for diagnostics.
    PayloadTooLarge {
        size_bytes: usize,
        limit_bytes: usize,
    },

    /// JSON serialization of the envelope failed. Should never happen in
    /// practice — reported for completeness.
    Serialization(serde_json::Error),

    /// Underlying HTTP transport error (DNS, TCP, TLS, timeout). The inner
    /// `reqwest::Error` has the detail; the top-level message presents the
    /// high-level class.
    Network(reqwest::Error),

    /// The server rejected the API key (HTTP 401 or 403). The operator must
    /// check their `LUMEN_API_KEY`.
    AuthFailed { status: u16 },

    /// The server rejected the payload (HTTP 4xx other than 401/403). Often
    /// indicates a schema mismatch between CLI and platform versions.
    BadRequest { status: u16, body_snippet: String },

    /// The server is unavailable or errored (HTTP 5xx). Usually transient;
    /// retries have been exhausted by the time this surfaces.
    ServerError { status: u16 },

    /// An unexpected HTTP status was returned. Included for forward
    /// compatibility — e.g. 3xx redirects (redirects are not followed).
    UnexpectedStatus { status: u16 },
}

impl PublishError {
    /// Returns whether this error class is worth retrying. Network / 5xx errors
    /// are retryable; 4xx / config errors are not.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Network(_) | Self::ServerError { .. })
    }
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(msg) => {
                write!(f, "publish URL is invalid: {msg}")
            }
            Self::PayloadTooLarge {
                size_bytes,
                limit_bytes,
            } => write!(
                f,
                "publish payload is {size_bytes} bytes, exceeding the {limit_bytes}-byte limit — \
                 contact support if you need a higher limit"
            ),
            Self::Serialization(e) => write!(f, "failed to serialize publish payload: {e}"),
            Self::Network(e) => {
                // Classify the error without leaking the full URL or headers.
                if e.is_timeout() {
                    write!(f, "publish timed out")
                } else if e.is_connect() {
                    write!(f, "publish connection failed")
                } else {
                    write!(f, "publish network error")
                }
            }
            Self::AuthFailed { status } => write!(
                f,
                "publish authentication failed (HTTP {status}) — check LUMEN_API_KEY"
            ),
            Self::BadRequest {
                status,
                body_snippet,
            } => write!(
                f,
                "publish was rejected by the server (HTTP {status}): {body_snippet}"
            ),
            Self::ServerError { status } => write!(
                f,
                "publish failed after retries — server returned HTTP {status}"
            ),
            Self::UnexpectedStatus { status } => write!(
                f,
                "publish returned unexpected HTTP {status} — refusing to retry"
            ),
        }
    }
}

impl std::error::Error for PublishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialization(e) => Some(e),
            // Intentionally not exposing the inner reqwest::Error — its
            // Display includes the full request URL which may reveal
            // internal infrastructure hostnames.
            Self::Network(_) => None,
            _ => None,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_url_display() {
        let e = PublishError::InvalidUrl("not https".into());
        assert_eq!(e.to_string(), "publish URL is invalid: not https");
    }

    #[test]
    fn payload_too_large_display() {
        let e = PublishError::PayloadTooLarge {
            size_bytes: 6_000_000,
            limit_bytes: 5_242_880,
        };
        let msg = e.to_string();
        assert!(msg.contains("6000000"));
        assert!(msg.contains("5242880"));
    }

    #[test]
    fn auth_failed_display() {
        let e = PublishError::AuthFailed { status: 401 };
        let msg = e.to_string();
        assert!(msg.contains("401"));
        assert!(msg.contains("LUMEN_API_KEY"));
    }

    #[test]
    fn bad_request_display() {
        let e = PublishError::BadRequest {
            status: 422,
            body_snippet: "invalid schema".into(),
        };
        let msg = e.to_string();
        assert!(msg.contains("422"));
        assert!(msg.contains("invalid schema"));
    }

    #[test]
    fn server_error_display_mentions_retries() {
        let e = PublishError::ServerError { status: 503 };
        let msg = e.to_string();
        assert!(msg.contains("retries"));
        assert!(msg.contains("503"));
    }

    #[test]
    fn is_retryable_network_yes() {
        assert!(PublishError::ServerError { status: 500 }.is_retryable());
    }

    #[test]
    fn is_retryable_auth_no() {
        assert!(!PublishError::AuthFailed { status: 401 }.is_retryable());
    }

    #[test]
    fn is_retryable_bad_request_no() {
        assert!(
            !PublishError::BadRequest {
                status: 400,
                body_snippet: "bad".into()
            }
            .is_retryable()
        );
    }

    #[test]
    fn is_retryable_payload_too_large_no() {
        assert!(
            !PublishError::PayloadTooLarge {
                size_bytes: 10,
                limit_bytes: 5
            }
            .is_retryable()
        );
    }

    #[test]
    fn debug_impl_does_not_panic() {
        let e = PublishError::InvalidUrl("test".into());
        let _ = format!("{e:?}");
    }
}
