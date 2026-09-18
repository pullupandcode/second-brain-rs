//! Development-mode authenticator: `Authorization: Bearer scope=<scopes>`.

use std::collections::HashSet;

use crate::auth::{
    AuthContext, AuthFuture, Authenticator,
    scopes::{Scope, parse_scopes},
};

/// Grants scopes parsed from a `Bearer scope=…` token, or configured defaults.
#[derive(Debug, Clone)]
pub struct DevAuthenticator {
    default_scopes: HashSet<Scope>,
    audience: String,
}

impl DevAuthenticator {
    /// Create a dev authenticator with fallback scopes for empty claims.
    #[must_use]
    pub const fn new(default_scopes: HashSet<Scope>) -> Self {
        Self {
            default_scopes,
            audience: String::new(),
        }
    }

    /// Set the audience reported in the development context.
    #[must_use]
    pub fn with_audience(mut self, audience: &str) -> Self {
        audience.clone_into(&mut self.audience);
        self
    }
}

impl Authenticator for DevAuthenticator {
    fn authenticate<'a>(&'a self, authorization: Option<&'a str>) -> AuthFuture<'a> {
        let token = authorization
            .and_then(|header| header.strip_prefix("Bearer "))
            .unwrap_or("");
        let claim = token.strip_prefix("scope=").unwrap_or("");
        let parsed = parse_scopes(claim);
        let scopes = if parsed.is_empty() {
            self.default_scopes.clone()
        } else {
            parsed
        };
        Box::pin(std::future::ready(Ok(AuthContext {
            issuer: "urn:second-brain-mcp:development".to_owned(),
            audience: self.audience.clone(),
            token_id: None,
            subject: "development".to_owned(),
            scopes,
            client_id: None,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parses_explicit_scopes() {
        let auth = DevAuthenticator::new(HashSet::new());
        let ctx = auth
            .authenticate(Some("Bearer scope=vault:read admin"))
            .await
            .unwrap();
        assert_eq!(ctx.subject, "development");
        assert!(ctx.scopes.contains(&Scope::VaultRead));
        assert!(ctx.scopes.contains(&Scope::Admin));
    }

    #[tokio::test]
    async fn unicode_scope_separators_match_reference_fallback() {
        let auth = DevAuthenticator::new(HashSet::from([Scope::SkillsRead]));
        let fallback = auth
            .authenticate(Some("Bearer scope=vault:read\u{85}admin"))
            .await
            .unwrap();
        assert_eq!(fallback.scopes, HashSet::from([Scope::SkillsRead]));
        let explicit = auth
            .authenticate(Some("Bearer scope=vault:read\u{feff}admin"))
            .await
            .unwrap();
        assert_eq!(
            explicit.scopes,
            HashSet::from([Scope::VaultRead, Scope::Admin])
        );
    }
    #[tokio::test]
    async fn falls_back_to_defaults_when_no_scope_claim() {
        let auth = DevAuthenticator::new(HashSet::from([Scope::VaultRead]));
        let ctx = auth.authenticate(Some("Bearer scope=")).await.unwrap();
        assert_eq!(ctx.scopes, HashSet::from([Scope::VaultRead]));
    }

    #[tokio::test]
    async fn missing_header_uses_fallback() {
        let auth = DevAuthenticator::new(HashSet::new());
        let ctx = auth.authenticate(None).await.unwrap();
        assert!(ctx.scopes.is_empty());
    }
    #[tokio::test]
    async fn missing_and_malformed_headers_use_configured_fallback() {
        let auth = DevAuthenticator::new(HashSet::from([Scope::VaultRead]))
            .with_audience("configured-audience");
        for header in [
            None,
            Some("Basic secret"),
            Some("Bearer other"),
            Some("Bearer scope=unknown"),
        ] {
            let ctx = auth.authenticate(header).await.unwrap();
            assert_eq!(ctx.subject, "development");
            assert_eq!(ctx.issuer, "urn:second-brain-mcp:development");
            assert_eq!(ctx.audience, "configured-audience");
            assert_eq!(ctx.scopes, HashSet::from([Scope::VaultRead]));
            assert!(ctx.token_id.is_none());
            assert!(ctx.client_id.is_none());
        }
        let explicit = auth.authenticate(Some("Bearer scope=admin")).await.unwrap();
        assert_eq!(explicit.scopes, HashSet::from([Scope::Admin]));
    }
}
