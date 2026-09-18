//! Authentication: the `Authenticator` seam, request context, and errors.

pub mod dev;
pub mod discovery;
pub mod jwt;
pub mod scopes;

use std::{collections::HashSet, future::Future, pin::Pin, sync::Arc};

use crate::{
    auth::{dev::DevAuthenticator, scopes::Scope},
    config::{AuthMode, ServerConfig},
};

/// Build the authenticator selected by `config.auth.mode`.
///
/// JWT mode fails closed if the verification client cannot initialize.
#[must_use]
pub fn build_authenticator(config: &ServerConfig) -> Arc<dyn Authenticator> {
    match config.auth.mode {
        AuthMode::Development => Arc::new(
            DevAuthenticator::new(
                config
                    .auth
                    .development_default_scopes
                    .iter()
                    .copied()
                    .collect(),
            )
            .with_audience(&config.auth.audience),
        ),
        AuthMode::Jwt => match jwt::JwtAuthenticator::new(&config.auth) {
            Ok(auth) => Arc::new(auth),
            Err(_) => Arc::new(UnavailableAuthenticator),
        },
    }
}

/// Authenticated request context inserted into axum request extensions.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuthContext {
    /// Validated issuer identifier.
    pub issuer: String,
    /// Expected token audience.
    pub audience: String,
    /// Optional token identifier.
    pub token_id: Option<String>,
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
    fn authenticate<'a>(&'a self, authorization: Option<&'a str>) -> AuthFuture<'a>;
}

/// Boxed asynchronous authentication result for object-safe dispatch.
pub type AuthFuture<'a> = Pin<Box<dyn Future<Output = Result<AuthContext, AuthError>> + Send + 'a>>;

struct UnavailableAuthenticator;
impl Authenticator for UnavailableAuthenticator {
    fn authenticate<'a>(&'a self, _authorization: Option<&'a str>) -> AuthFuture<'a> {
        Box::pin(std::future::ready(Err(AuthError::invalid(
            "Invalid bearer token",
        ))))
    }
}
