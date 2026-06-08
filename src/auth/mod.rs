//! Authentication: the `Authenticator` seam, request context, and errors.

pub mod dev;
pub mod discovery;
pub mod scopes;

use std::collections::HashSet;

use crate::auth::scopes::Scope;

/// Authenticated request context inserted into axum request extensions.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuthContext {
    /// Canonical subject identity.
    pub subject: String,
    /// Granted scopes.
    pub scopes: HashSet<Scope>,
    /// Optional client id.
    pub client_id: Option<String>,
}

/// Authentication error codes (RFC 6750 style).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthErrorCode {
    /// No bearer token present.
    MissingToken,
    /// Token present but invalid.
    InvalidToken,
}

impl AuthErrorCode {
    /// The `error` value for the `WWW-Authenticate` header.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingToken => "missing_token",
            Self::InvalidToken => "invalid_token",
        }
    }
}

/// An authentication failure. Always maps to HTTP 401.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{code:?}: {message}")]
#[non_exhaustive]
pub struct AuthError {
    /// Error code.
    pub code: AuthErrorCode,
    /// Human-readable message.
    pub message: String,
}

impl AuthError {
    /// Construct a `missing_token` error.
    #[must_use]
    pub fn missing(message: impl Into<String>) -> Self {
        Self {
            code: AuthErrorCode::MissingToken,
            message: message.into(),
        }
    }

    /// Construct an `invalid_token` error.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: AuthErrorCode::InvalidToken,
            message: message.into(),
        }
    }
}

/// Authenticates an incoming request from its `Authorization` header value.
///
/// Implementations must be cheap to clone or wrapped in `Arc`.
pub trait Authenticator: Send + Sync {
    /// Authenticate using the raw `Authorization` header value (if any).
    ///
    /// # Errors
    /// Returns [`AuthError`] when the token is missing or invalid.
    fn authenticate(&self, authorization: Option<&str>) -> Result<AuthContext, AuthError>;
}
