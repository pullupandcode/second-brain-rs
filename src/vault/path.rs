//! Vault path normalization and resolution with traversal and symlink protection.

use std::path::{Path, PathBuf};

/// Maximum accepted input path length (boundary input validation).
pub const MAX_VAULT_PATH_LEN: usize = 1024;

/// Errors from vault path validation and resolution.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VaultPathError {
    /// Absolute paths are rejected.
    #[error("Vault path must be relative")]
    NotRelative,
    /// `..` segments are rejected.
    #[error("Vault path must not contain traversal segments")]
    Traversal,
    /// Input exceeds [`MAX_VAULT_PATH_LEN`].
    #[error("Vault path is too long")]
    TooLong,
    /// The path escapes the vault root (including via symlinks).
    #[error("Vault path resolves outside the vault root")]
    OutsideRoot,
    /// Underlying filesystem error.
    #[error("Vault path could not be resolved")]
    Io(#[from] std::io::Error),
}

/// Normalize a vault-relative path: `\`→`/`, reject absolute and `..`,
/// collapse `.` and empty segments. `.` normalizes to the empty string.
///
/// # Errors
/// [`VaultPathError::NotRelative`], [`VaultPathError::Traversal`], or
/// [`VaultPathError::TooLong`].
pub fn normalize_vault_path(input: &str) -> Result<String, VaultPathError> {
    if input.len() > MAX_VAULT_PATH_LEN {
        return Err(VaultPathError::TooLong);
    }
    let unified = input.replace('\\', "/");
    if unified.starts_with('/') {
        return Err(VaultPathError::NotRelative);
    }
    let mut segments: Vec<&str> = Vec::new();
    for segment in unified.split('/') {
        match segment {
            "" | "." => {}
            ".." => return Err(VaultPathError::Traversal),
            other => segments.push(other),
        }
    }
    Ok(segments.join("/"))
}

/// Resolve a normalized vault path against the vault root (lexical; the
/// result is `root` itself for the empty path).
///
/// # Errors
/// Propagates [`normalize_vault_path`] errors.
pub fn resolve_vault_path(vault_root: &Path, input: &str) -> Result<PathBuf, VaultPathError> {
    let normalized = normalize_vault_path(input)?;
    if normalized.is_empty() {
        return Ok(vault_root.to_path_buf());
    }
    Ok(vault_root.join(normalized))
}

/// Resolve a vault path that must already exist, canonicalizing both sides so
/// symlinks cannot escape the root.
///
/// # Errors
/// [`VaultPathError::OutsideRoot`] when the real path leaves the real root;
/// [`VaultPathError::Io`] when either side cannot be canonicalized.
pub async fn resolve_existing_vault_path(
    vault_root: &Path,
    input: &str,
) -> Result<PathBuf, VaultPathError> {
    let joined = resolve_vault_path(vault_root, input)?;
    let real_root = tokio::fs::canonicalize(vault_root).await?;
    let real_path = tokio::fs::canonicalize(&joined).await?;
    if real_path == real_root || real_path.starts_with(&real_root) {
        Ok(real_path)
    } else {
        Err(VaultPathError::OutsideRoot)
    }
}

/// Resolve a vault path for writing: the target need not exist, but its first
/// existing ancestor must canonicalize to inside the real root. Returns the
/// (unresolved) absolute target path.
///
/// # Errors
/// [`VaultPathError::OutsideRoot`] when the existing ancestor leaves the real
/// root; [`VaultPathError::Io`] on canonicalization failure.
pub async fn resolve_vault_path_for_write(
    vault_root: &Path,
    input: &str,
) -> Result<PathBuf, VaultPathError> {
    let joined = resolve_vault_path(vault_root, input)?;
    let real_root = tokio::fs::canonicalize(vault_root).await?;

    let mut ancestor = joined.parent().map(Path::to_path_buf);
    let existing = loop {
        match ancestor {
            None => break real_root.clone(),
            Some(candidate) => {
                if tokio::fs::try_exists(&candidate).await? {
                    break tokio::fs::canonicalize(&candidate).await?;
                }
                ancestor = candidate.parent().map(Path::to_path_buf);
            }
        }
    };

    if existing == real_root || existing.starts_with(&real_root) {
        Ok(joined)
    } else {
        Err(VaultPathError::OutsideRoot)
    }
}

/// Whether the path has a (case-insensitive) `.md` extension.
#[must_use]
pub fn is_markdown_path(input: &str) -> bool {
    Path::new(input)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_separators_and_dot() {
        assert_eq!(normalize_vault_path("a\\b\\c.md").unwrap(), "a/b/c.md");
        assert_eq!(normalize_vault_path(".").unwrap(), "");
        assert_eq!(normalize_vault_path("a/./b//c.md").unwrap(), "a/b/c.md");
    }

    #[test]
    fn rejects_absolute_and_traversal() {
        assert!(matches!(
            normalize_vault_path("/abs"),
            Err(VaultPathError::NotRelative)
        ));
        assert!(matches!(
            normalize_vault_path("../x"),
            Err(VaultPathError::Traversal)
        ));
        assert!(matches!(
            normalize_vault_path("a/../x"),
            Err(VaultPathError::Traversal)
        ));
        assert!(matches!(
            normalize_vault_path("\\abs"),
            Err(VaultPathError::NotRelative)
        ));
    }

    #[test]
    fn rejects_overlong_input() {
        let long = "a/".repeat(MAX_VAULT_PATH_LEN);
        assert!(matches!(
            normalize_vault_path(&long),
            Err(VaultPathError::TooLong)
        ));
    }

    #[test]
    fn is_markdown_is_case_insensitive() {
        assert!(is_markdown_path("Note.MD"));
        assert!(is_markdown_path("a/b/note.md"));
        assert!(!is_markdown_path("note.txt"));
        assert!(!is_markdown_path("md"));
    }

    #[tokio::test]
    async fn symlink_escape_is_blocked() {
        let outside = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.md");
        tokio::fs::write(&secret, "secret").await.unwrap();
        #[cfg(unix)]
        {
            tokio::fs::symlink(&secret, vault.path().join("link.md"))
                .await
                .unwrap();
            let err = resolve_existing_vault_path(vault.path(), "link.md")
                .await
                .unwrap_err();
            assert!(matches!(err, VaultPathError::OutsideRoot));
        }
    }

    #[tokio::test]
    async fn existing_path_inside_root_resolves() {
        let vault = tempfile::tempdir().unwrap();
        tokio::fs::create_dir(vault.path().join("sub"))
            .await
            .unwrap();
        tokio::fs::write(vault.path().join("sub/n.md"), "x")
            .await
            .unwrap();
        let resolved = resolve_existing_vault_path(vault.path(), "sub/n.md")
            .await
            .unwrap();
        assert!(resolved.ends_with("sub/n.md"));
    }

    #[tokio::test]
    async fn write_resolution_checks_first_existing_ancestor() {
        let vault = tempfile::tempdir().unwrap();
        let target = resolve_vault_path_for_write(vault.path(), "new/deep/n.md")
            .await
            .unwrap();
        assert!(target.ends_with("new/deep/n.md"));
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            tokio::fs::symlink(outside.path(), vault.path().join("esc"))
                .await
                .unwrap();
            let err = resolve_vault_path_for_write(vault.path(), "esc/n.md")
                .await
                .unwrap_err();
            assert!(matches!(err, VaultPathError::OutsideRoot));
        }
    }
}
