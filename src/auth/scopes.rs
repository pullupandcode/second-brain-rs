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
    /// `skills:read`
    SkillsRead,
    /// `vault:delete`
    VaultDelete,
    /// `vault:delete:hard`
    VaultDeleteHard,
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
            Self::SkillsRead => "skills:read",
            Self::VaultDelete => "vault:delete",
            Self::VaultDeleteHard => "vault:delete:hard",
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
pub const KNOWN_SCOPES: [Scope; 8] = [
    Scope::VaultRead,
    Scope::SkillsRead,
    Scope::VaultWrite,
    Scope::VaultDelete,
    Scope::VaultDeleteHard,
    Scope::VaultCapture,
    Scope::DailyAppend,
    Scope::Admin,
];

/// Parse an ECMAScript-whitespace-separated scope claim, dropping unknown tokens.
#[must_use]
pub fn parse_scopes(claim: &str) -> HashSet<Scope> {
    claim
        .split(|c| {
            matches!(c,
                '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
                '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
                '\u{205f}' | '\u{3000}' | '\u{feff}'
            )
        })
        .filter_map(Scope::from_wire)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
    fn scope_separators_match_ecmascript_whitespace_exactly() {
        for scalar in [
            0x9, 0xa, 0xb, 0xc, 0xd, 0x20, 0xa0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004,
            0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000,
            0xfeff,
        ] {
            let separator = char::from_u32(scalar).unwrap();
            assert_eq!(
                parse_scopes(&format!("{separator}vault:read{separator}admin{separator}")),
                HashSet::from([Scope::VaultRead, Scope::Admin]),
                "U+{scalar:04X}"
            );
        }
        for scalar in [0x8, 0xe, 0x1c, 0x85, 0x180e, 0x200b, 0x202a, 0x2060, 0xfe00] {
            let separator = char::from_u32(scalar).unwrap();
            assert!(
                parse_scopes(&format!("vault:read{separator}admin")).is_empty(),
                "U+{scalar:04X}"
            );
        }
    }
    #[test]
    fn roundtrip_wire_strings() {
        for scope in KNOWN_SCOPES {
            assert_eq!(Scope::from_wire(scope.as_str()), Some(scope));
        }
        assert_eq!(Scope::from_wire("nope"), None);
    }
}
