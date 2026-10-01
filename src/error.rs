//! The one error type for everything that can go wrong against HelloAsso, and
//! its translation into MCP protocol errors.

use rmcp::ErrorData;
use serde_json::json;

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can fail between the environment and the HelloAsso API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The environment is missing something, or holds something unusable.
    #[error("configuration: {0}")]
    Config(String),

    /// The caller passed something the server refuses before any HTTP call.
    #[error("{0}")]
    Invalid(String),

    /// A name matched several objects and the server will not guess.
    #[error("{message}")]
    Ambiguous {
        /// What was ambiguous, and how to disambiguate it.
        message: String,
        /// The candidates, so the caller can pick one.
        candidates: Vec<Candidate>,
    },

    /// A name matched nothing at all.
    #[error("{0}")]
    NotFound(String),

    /// Getting an access token failed.
    #[error("authentication: {0}")]
    Auth(String),

    /// The transport failed: DNS, TLS, timeout, connection reset.
    #[error("transport: {0}")]
    Transport(String),

    /// HelloAsso answered with a 4xx or 5xx.
    #[error("{endpoint} → HTTP {status}{}{}", code.as_deref().map(|t| format!(" {t}")).unwrap_or_default(), message.as_deref().map(|d| format!(": {d}")).unwrap_or_default())]
    Api {
        /// The method and path that failed, for instance `GET /orders/42`.
        endpoint: String,
        /// The HTTP status code.
        status: u16,
        /// HelloAsso's machine-readable error code, when the body carried one.
        code: Option<String>,
        /// The human message, when the body carried one.
        message: Option<String>,
    },

    /// The response was a 2xx but not the JSON this client expected.
    #[error("decoding {endpoint}: {source}")]
    Decode {
        /// The endpoint whose response could not be decoded.
        endpoint: String,
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// Reading or writing a local file failed.
    #[error("{path}: {source}")]
    Io {
        /// The path involved.
        path: String,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },
}

/// One of several objects a name matched, as reported by [`Error::Ambiguous`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct Candidate {
    /// The object's identifier or slug.
    pub id: String,
    /// The object's human name.
    pub name: String,
}

impl Error {
    /// Shorthand for [`Error::Invalid`].
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    /// Shorthand for [`Error::NotFound`].
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// Shorthand for [`Error::Config`].
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// True when retrying the very same request could plausibly succeed.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Api { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }

    /// The HTTP status, when this is an API error.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// Translate a crate error into the MCP error the client will see.
///
/// Anything the caller can fix by calling again with different arguments is an
/// `invalid_params`; everything else is an `internal_error` carrying the
/// upstream status and code in `data`, so a model can tell a missing privilege
/// from an outage without parsing prose.
impl From<Error> for ErrorData {
    fn from(err: Error) -> Self {
        let message = err.to_string();
        match err {
            Error::Invalid(_) | Error::NotFound(_) => ErrorData::invalid_params(message, None),
            Error::Ambiguous { candidates, .. } => {
                ErrorData::invalid_params(message, Some(json!({ "candidates": candidates })))
            }
            Error::Config(_) => {
                ErrorData::internal_error(message, Some(json!({ "kind": "configuration" })))
            }
            Error::Auth(_) => {
                ErrorData::internal_error(message, Some(json!({ "kind": "authentication" })))
            }
            Error::Api {
                ref endpoint,
                status,
                ref code,
                ..
            } => {
                let data = json!({
                    "kind": "helloasso_api",
                    "endpoint": endpoint,
                    "status": status,
                    "code": code,
                });
                if matches!(status, 400 | 404 | 409 | 422) {
                    ErrorData::invalid_params(message, Some(data))
                } else {
                    ErrorData::internal_error(message, Some(data))
                }
            }
            Error::Transport(_) | Error::Decode { .. } | Error::Io { .. } => {
                ErrorData::internal_error(message, None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use super::*;

    #[test]
    fn ambiguous_carries_candidates_to_the_client() {
        let err = Error::Ambiguous {
            message: "two forms match \"gala\"".into(),
            candidates: vec![Candidate {
                id: "gala-2026".into(),
                name: "Gala 2026".into(),
            }],
        };
        let data: ErrorData = err.into();
        assert_eq!(data.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert_eq!(
            data.data.unwrap_or_default()["candidates"][0]["id"],
            "gala-2026"
        );
    }

    #[test]
    fn rate_limits_and_server_errors_are_transient() {
        let limited = Error::Api {
            endpoint: "GET /orders/1".into(),
            status: 429,
            code: None,
            message: None,
        };
        assert!(limited.is_transient());
        assert!(!Error::invalid("nope").is_transient());
    }

    #[test]
    fn a_forbidden_call_stays_an_internal_error_but_names_its_status() {
        let err = Error::Api {
            endpoint: "GET /organizations/x/payments".into(),
            status: 403,
            code: None,
            message: Some("missing privilege AccessTransactions".into()),
        };
        let data: ErrorData = err.into();
        assert_eq!(data.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert_eq!(data.data.unwrap_or_default()["status"], 403);
    }
}
