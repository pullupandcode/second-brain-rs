//! OAuth scopes governing access to MCP tools.

use std::collections::HashSet;

/// An OAuth scope governing access to MCP tools.
///
/// Marked `#[non_exhaustive]`: more scopes are anticipated. Internal matches
/// stay exhaustive (no wildcard arm) so a new variant is a compile error at
/// every match site until handled.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// `vault:read`
    VaultRead,
    /// `vault:write`
    VaultWrite,
    /// `vault:capture`
    VaultCapture,
    /// `daily:append`
    DailyAppend,
    /// `admin`
    Admin,
}

impl Scope {
    /// The wire string for this scope.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VaultRead => "vault:read",
            Self::VaultWrite => "vault:write",
            Self::VaultCapture => "vault:capture",
            Self::DailyAppend => "daily:append",
            Self::Admin => "admin",
        }
    }

    /// Parse a single scope token; unknown tokens return `None`.
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        KNOWN_SCOPES
            .into_iter()
            .find(|scope| scope.as_str() == token)
    }
}

/// All known scopes, in OAuth-discovery order.
pub const KNOWN_SCOPES: [Scope; 5] = [
    Scope::VaultRead,
    Scope::VaultWrite,
    Scope::VaultCapture,
    Scope::DailyAppend,
    Scope::Admin,
];

/// Parse a whitespace-separated scope claim, dropping unknown tokens.
#[must_use]
pub fn parse_scopes(claim: &str) -> HashSet<Scope> {
    claim
        .split_whitespace()
        .filter_map(Scope::from_wire)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_scopes_and_drops_unknown() {
        let scopes = parse_scopes("vault:read admin bogus daily:append");
        assert_eq!(scopes.len(), 3);
        assert!(scopes.contains(&Scope::VaultRead));
        assert!(scopes.contains(&Scope::Admin));
        assert!(scopes.contains(&Scope::DailyAppend));
    }

    #[test]
    fn empty_claim_is_empty_set() {
        assert!(parse_scopes("   ").is_empty());
    }

    #[test]
    fn roundtrip_wire_strings() {
        for scope in KNOWN_SCOPES {
            assert_eq!(Scope::from_wire(scope.as_str()), Some(scope));
        }
        assert_eq!(Scope::from_wire("nope"), None);
    }
}
