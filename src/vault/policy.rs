//! Shared vault-path glob matcher used for ignored globs and blocked paths.
//!
//! Supports a deliberately small subset, checked in this order:
//! 1. `Folder/**` — the folder itself or anything beneath it.
//! 2. the literal `**/*.sync-conflict-*` — basename contains `.sync-conflict-`.
//! 3. `**/*<suffix>` — path ends with `<suffix>`.
//! 4. any pattern containing `*` — `*` matches any run of characters.
//! 5. otherwise — exact match.

use std::sync::{Arc, RwLock};

/// Shared effective hard denylist, matched case-insensitively on every platform.
///
/// Ordinary vault path identity and soft ignored-glob matching remain unchanged.
/// Matching does not depend on files existing, so new paths receive the same
/// protection. Replacements are immediately visible to every clone.
#[derive(Clone, Debug, Default)]
pub struct PathPolicy(Arc<RwLock<DenyRules>>);

#[derive(Debug, Default)]
struct DenyRules {
    patterns: Vec<String>,
    // A compilation failure denies all paths, rather than silently losing a rule.
    matchers: Vec<Option<regex::Regex>>,
}

impl DenyRules {
    fn new(patterns: Vec<String>) -> Self {
        let matchers = patterns
            .iter()
            .map(|pattern| compile_deny_pattern(pattern).ok())
            .collect();
        Self { patterns, matchers }
    }
}

impl PathPolicy {
    /// Construct a policy from normalized vault patterns.
    #[must_use]
    pub fn new(patterns: Vec<String>) -> Self {
        Self(Arc::new(RwLock::new(DenyRules::new(patterns))))
    }
    /// Test a vault-relative path against the current case-insensitive hard denylist.
    #[must_use]
    pub fn is_blocked(&self, path: &str) -> bool {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .matchers
            .iter()
            .any(|matcher| {
                matcher
                    .as_ref()
                    .is_none_or(|matcher| matcher.is_match(path))
            })
    }
    /// Atomically replace the effective denylist. Compilation happens before locking.
    pub fn replace(&self, patterns: Vec<String>) {
        let rules = DenyRules::new(patterns);
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = rules;
    }
    /// Return original pattern spelling for diagnostics and policy composition.
    #[must_use]
    pub fn snapshot(&self) -> Vec<String> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .patterns
            .clone()
    }
}

// Preserve the glob subset below, changing only hard-deny case comparison.
fn compile_deny_pattern(pattern: &str) -> Result<regex::Regex, regex::Error> {
    let source = deny_pattern_source(pattern);
    regex::RegexBuilder::new(&format!(r"\A{source}\z"))
        .case_insensitive(true)
        .unicode(true)
        .dot_matches_new_line(true)
        .build()
}

fn deny_pattern_source(pattern: &str) -> String {
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return format!("{}(?:/.*)?", regex::escape(prefix));
    }
    if pattern.eq_ignore_ascii_case("**/*.sync-conflict-*") {
        return r"(?:.*/)?[^/]*\.sync-conflict-[^/]*".to_owned();
    }
    pattern.strip_prefix("**/*").map_or_else(
        || {
            pattern
                .split('*')
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join(".*")
        },
        |suffix| format!(".*{}", regex::escape(suffix)),
    )
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
            ("Private/**", "Private/a\nb.md", true),
            ("a/*/c.md", "a/line\nbreak/c.md", true),
            ("a/[literal].md", "a/[literal].md", true),
            ("a/[literal].md", "a/l.md", false),
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
            assert_eq!(
                PathPolicy::new(vec![pattern.to_owned()]).is_blocked(path),
                expected,
                "hard policy pattern={pattern} path={path}"
            );
        }
    }

    #[test]
    fn hard_denies_ignore_case_in_existing_and_future_paths() {
        let policy = PathPolicy::new(vec![
            "Private/**".to_owned(),
            "Future/**".to_owned(),
            "Skills/Σ.md".to_owned(),
            "**/*.Secret.md".to_owned(),
        ]);
        for path in [
            "private/secret.md",
            "PRIVATE",
            "future/not-created/new.md",
            "skills/ς.MD",
            "deep/n.SECRET.MD",
        ] {
            assert!(policy.is_blocked(path), "hard deny missed {path}");
        }
        assert!(!policy.is_blocked("PrivateZone/open.md"));
        assert!(!policy.is_blocked("public/open.md"));
        // Soft exclusions and generic matching retain reference case sensitivity.
        assert!(!matches_vault_path_pattern(
            "Private/**",
            "private/secret.md"
        ));
        assert!(!path_matches_any_pattern(
            &["**/*.Secret.md".to_owned()],
            "deep/n.secret.md"
        ));
        assert_eq!(
            policy.snapshot().first().map(String::as_str),
            Some("Private/**")
        );
        let clone = policy.clone();
        policy.replace(vec!["NewSkills/**".to_owned()]);
        assert!(clone.is_blocked("newskills/missing.md"));
        assert!(!clone.is_blocked("private/secret.md"));
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
