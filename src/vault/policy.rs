//! Shared vault-path glob matcher used for ignored globs and blocked paths.
//!
//! Supports a deliberately small subset, checked in this order:
//! 1. `Folder/**` — the folder itself or anything beneath it.
//! 2. the literal `**/*.sync-conflict-*` — basename contains `.sync-conflict-`.
//! 3. `**/*<suffix>` — path ends with `<suffix>`.
//! 4. any pattern containing `*` — `*` matches any run of characters.
//! 5. otherwise — exact match.

use std::sync::{Arc, RwLock};

/// Shared effective denylist. Replacements are immediately visible to every clone.
#[derive(Clone, Debug, Default)]
pub struct PathPolicy(Arc<RwLock<Vec<String>>>);

impl PathPolicy {
    /// Construct a policy from normalized vault patterns.
    #[must_use]
    pub fn new(patterns: Vec<String>) -> Self {
        Self(Arc::new(RwLock::new(patterns)))
    }
    /// Test a vault-relative path against the current denylist.
    #[must_use]
    pub fn is_blocked(&self, path: &str) -> bool {
        path_matches_any_pattern(
            &self
                .0
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            path,
        )
    }
    /// Atomically replace the effective denylist. No guard escapes this method.
    pub fn replace(&self, patterns: Vec<String>) {
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = patterns;
    }
    /// Return a snapshot for diagnostics and policy composition.
    #[must_use]
    pub fn snapshot(&self) -> Vec<String> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// Whether `vault_path` matches any of `patterns`.
#[must_use]
pub fn path_matches_any_pattern(patterns: &[String], vault_path: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| matches_vault_path_pattern(pattern, vault_path))
}

/// Whether `vault_path` matches a single `pattern` from the supported subset.
#[must_use]
pub fn matches_vault_path_pattern(pattern: &str, vault_path: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return vault_path == prefix || vault_path.starts_with(&format!("{prefix}/"));
    }
    if pattern == "**/*.sync-conflict-*" {
        let basename = vault_path.rsplit('/').next().unwrap_or(vault_path);
        return basename.contains(".sync-conflict-");
    }
    if let Some(suffix) = pattern.strip_prefix("**/*") {
        return vault_path.ends_with(suffix);
    }
    if pattern.contains('*') {
        return matches_wildcard(pattern, vault_path);
    }
    pattern == vault_path
}

/// Match a `*`-wildcard pattern (literal between stars) against `text`, where
/// `*` matches any run of characters. Anchored on both ends.
fn matches_wildcard(pattern: &str, text: &str) -> bool {
    let mut segments = pattern.split('*');
    let Some(first) = segments.next() else {
        return false;
    };
    let Some(mut remaining) = text.strip_prefix(first) else {
        return false;
    };
    let tail: Vec<&str> = segments.collect();
    let Some((last, middles)) = tail.split_last() else {
        return remaining.is_empty();
    };
    for middle in middles {
        match remaining.find(middle) {
            Some(pos) => match remaining.get(pos + middle.len()..) {
                Some(rest) => remaining = rest,
                None => return false,
            },
            None => return false,
        }
    }
    remaining.ends_with(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_table() {
        let cases = [
            ("Private/**", "Private/a.md", true),
            ("Private/**", "Private", true),
            ("Private/**", "Public/a.md", false),
            ("Private/**", "PrivateZone/a.md", false),
            ("**/*.sync-conflict-*", "x/y.sync-conflict-123.md", true),
            ("**/*.sync-conflict-*", "x/y.md", false),
            ("**/*.secret.md", "deep/n.secret.md", true),
            ("**/*.secret.md", "deep/n.md", false),
            ("**/.DS_Store", "a/.DS_Store", true),
            ("**/.DS_Store", "a/b/.DS_Store", true),
            (".obsidian/workspace*", ".obsidian/workspace.json", true),
            (".obsidian/workspace*", ".obsidian/other.json", false),
            ("Templates/Daily.md", "Templates/Daily.md", true),
            ("Templates/Daily.md", "Templates/Daily2.md", false),
            ("a/*/c.md", "a/b/c.md", true),
            ("a/*/c.md", "a/b/d.md", false),
        ];
        for (pattern, path, expected) in cases {
            assert_eq!(
                matches_vault_path_pattern(pattern, path),
                expected,
                "pattern={pattern} path={path}"
            );
        }
    }

    #[test]
    fn any_pattern_helper() {
        let patterns = vec![".trash/**".to_owned(), "**/*.secret.md".to_owned()];
        assert!(path_matches_any_pattern(&patterns, ".trash/old.md"));
        assert!(path_matches_any_pattern(&patterns, "notes/x.secret.md"));
        assert!(!path_matches_any_pattern(&patterns, "notes/x.md"));
        assert!(!path_matches_any_pattern(&[], "anything.md"));
    }
}
