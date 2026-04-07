use tokio::task::JoinError;

/// Errors that can occur during a load test run.
#[derive(Debug)]
pub enum RunError {
    /// Failed to build the HTTP client for this run.
    HttpClientBuild(reqwest::Error),
    /// The internal drain task terminated unexpectedly.
    DrainTaskFailed(JoinError),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HttpClientBuild(e) => write!(f, "failed to build HTTP client: {e}"),
            Self::DrainTaskFailed(e) => write!(f, "drain task failed unexpectedly: {e}"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::HttpClientBuild(e) => Some(e),
            Self::DrainTaskFailed(e) => Some(e),
        }
    }
}

impl From<reqwest::Error> for RunError {
    fn from(e: reqwest::Error) -> Self {
        Self::HttpClientBuild(e)
    }
}

impl From<JoinError> for RunError {
    fn from(e: JoinError) -> Self {
        Self::DrainTaskFailed(e)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_reqwest_error() -> reqwest::Error {
        // reqwest::get on an unparseable URL fails immediately without a network
        // call, giving us a real reqwest::Error without async overhead.
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(reqwest::get("not-a-valid-url"))
            .unwrap_err()
    }

    fn make_join_error() -> JoinError {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            tokio::spawn(async { panic!("test panic") })
                .await
                .unwrap_err()
        })
    }

    #[test]
    fn display_http_client_build() {
        let run_err = RunError::HttpClientBuild(make_reqwest_error());
        assert!(
            run_err
                .to_string()
                .starts_with("failed to build HTTP client:"),
            "unexpected: {run_err}"
        );
    }

    #[test]
    fn display_drain_task_failed() {
        let run_err = RunError::DrainTaskFailed(make_join_error());
        assert!(
            run_err
                .to_string()
                .starts_with("drain task failed unexpectedly:"),
            "unexpected: {run_err}"
        );
    }

    #[test]
    fn source_is_set() {
        let run_err = RunError::HttpClientBuild(make_reqwest_error());
        assert!(
            std::error::Error::source(&run_err).is_some(),
            "source must be set"
        );
    }

    #[test]
    fn from_reqwest_error_produces_http_client_build_variant() {
        let run_err = RunError::from(make_reqwest_error());
        assert!(matches!(run_err, RunError::HttpClientBuild(_)));
    }

    #[test]
    fn from_join_error_produces_drain_task_failed_variant() {
        let run_err = RunError::from(make_join_error());
        assert!(matches!(run_err, RunError::DrainTaskFailed(_)));
    }
}
