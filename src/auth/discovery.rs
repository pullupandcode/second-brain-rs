//! OAuth protected-resource discovery metadata.

use serde::Serialize;

use crate::auth::scopes::KNOWN_SCOPES;
use crate::config::ServerConfig;

/// Response body for `/.well-known/oauth-protected-resource`.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct ProtectedResourceMetadata {
    /// The protected resource (public base URL, no trailing slash).
    pub resource: String,
    /// Authorization servers advertised to clients.
    pub authorization_servers: Vec<String>,
    /// Supported scopes.
    pub scopes_supported: Vec<String>,
    /// Supported bearer methods.
    pub bearer_methods_supported: Vec<String>,
    /// Documentation URL.
    pub resource_documentation: String,
}

/// Build discovery metadata from server config.
#[must_use]
pub fn build_protected_resource_metadata(config: &ServerConfig) -> ProtectedResourceMetadata {
    let base = trim_trailing_slash(config.public_base_url.as_str());
    ProtectedResourceMetadata {
        resource: base.to_owned(),
        authorization_servers: vec![
            config.auth.discovery_authorization_server.as_str().to_owned(),
        ],
        scopes_supported: KNOWN_SCOPES.iter().map(|scope| scope.as_str().to_owned()).collect(),
        bearer_methods_supported: vec!["header".to_owned()],
        resource_documentation: format!("{base}/docs"),
    }
}

fn trim_trailing_slash(value: &str) -> &str {
    value.strip_suffix('/').unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;
    use crate::config::tests_support::MINIMAL;

    fn config() -> ServerConfig {
        parse_config(MINIMAL).unwrap()
    }

    #[test]
    fn builds_expected_metadata() {
        let meta = build_protected_resource_metadata(&config());
        assert_eq!(meta.resource, "http://127.0.0.1:3000");
        assert_eq!(meta.bearer_methods_supported, vec!["header"]);
        assert_eq!(meta.scopes_supported.len(), 5);
        assert_eq!(meta.scopes_supported.first().map(String::as_str), Some("vault:read"));
        assert_eq!(meta.resource_documentation, "http://127.0.0.1:3000/docs");
        assert_eq!(meta.authorization_servers, vec!["https://idp.example.com/o/sb/"]);
    }
}
