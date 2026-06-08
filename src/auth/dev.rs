//! Development-mode authenticator: `Authorization: Bearer scope=<scopes>`.

use std::collections::HashSet;

use crate::auth::{
    AuthContext, AuthError, Authenticator,
    scopes::{Scope, parse_scopes},
};

/// Grants scopes parsed from a `Bearer scope=…` token, or configured defaults.
#[derive(Debug, Clone)]
pub struct DevAuthenticator {
    default_scopes: HashSet<Scope>,
}

impl DevAuthenticator {
    /// Create a dev authenticator with fallback scopes for empty claims.
    #[must_use]
    pub const fn new(default_scopes: HashSet<Scope>) -> Self {
        Self { default_scopes }
    }
}

impl Authenticator for DevAuthenticator {
    fn authenticate(&self, authorization: Option<&str>) -> Result<AuthContext, AuthError> {
        let header = authorization.ok_or_else(|| AuthError::missing("Missing bearer token"))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AuthError::missing("Missing bearer token"))?;
        let claim = token.strip_prefix("scope=").unwrap_or("");
        let parsed = parse_scopes(claim);
        let scopes = if parsed.is_empty() {
            self.default_scopes.clone()
        } else {
            parsed
        };
        Ok(AuthContext {
            subject: "development".to_owned(),
            scopes,
            client_id: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_scopes() {
        let auth = DevAuthenticator::new(HashSet::new());
        let ctx = auth
            .authenticate(Some("Bearer scope=vault:read admin"))
            .unwrap();
        assert_eq!(ctx.subject, "development");
        assert!(ctx.scopes.contains(&Scope::VaultRead));
        assert!(ctx.scopes.contains(&Scope::Admin));
    }

    #[test]
    fn falls_back_to_defaults_when_no_scope_claim() {
        let auth = DevAuthenticator::new(HashSet::from([Scope::VaultRead]));
        let ctx = auth.authenticate(Some("Bearer scope=")).unwrap();
        assert_eq!(ctx.scopes, HashSet::from([Scope::VaultRead]));
    }

    #[test]
    fn missing_header_is_missing_token() {
        let auth = DevAuthenticator::new(HashSet::new());
        let err = auth.authenticate(None).unwrap_err();
        assert_eq!(err.code, crate::auth::AuthErrorCode::MissingToken);
    }
}
