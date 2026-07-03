//! Async vault reader: note reads (with hash + parse) and folder listing.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::vault::{
    markdown::{ParsedMarkdown, parse_markdown},
    path::{VaultPathError, is_markdown_path, normalize_vault_path, resolve_existing_vault_path},
    policy::path_matches_any_pattern,
};

/// Errors from reading the vault.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VaultReaderError {
    /// Path matches a configured ignored glob.
    #[error("Vault path is ignored: {0}")]
    Ignored(String),
    /// Path matches a configured blocked path.
    #[error("Vault path is blocked")]
    Blocked,
    /// Path is not a markdown note.
    #[error("Vault path is not a markdown note: {0}")]
    NotMarkdown(String),
    /// Path validation/resolution failure.
    #[error(transparent)]
    Path(#[from] VaultPathError),
    /// Filesystem error.
    #[error("Vault read failed")]
    Io(#[from] std::io::Error),
}

/// A single filesystem entry kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EntryKind {
    /// A markdown file.
    File,
    /// A directory.
    Directory,
}

/// A folder listing entry.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FolderEntry {
    /// Vault-relative path.
    pub path: String,
    /// Entry kind.
    pub kind: EntryKind,
}

/// A note read result.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReadNoteResult {
    /// Vault-relative (normalized) path.
    pub path: String,
    /// Raw file content.
    pub content: String,
    /// Hex SHA-256 of the content.
    pub current_sha256: String,
    /// Parsed markdown view.
    pub parsed: ParsedMarkdown,
}

/// Options for constructing a [`VaultReader`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct VaultReaderOptions {
    /// Absolute vault root.
    pub vault_root: PathBuf,
    /// Ignored globs (excluded from reads and listings).
    pub ignored_globs: Vec<String>,
    /// Blocked paths (hard denylist).
    pub blocked_paths: Vec<String>,
}

/// Reads notes and folders from a local vault with ignore/block enforcement.
#[derive(Debug, Clone)]
pub struct VaultReader {
    vault_root: PathBuf,
    ignored_globs: Vec<String>,
    blocked_paths: Vec<String>,
}

impl VaultReader {
    /// Construct a reader.
    #[must_use]
    pub fn new(options: VaultReaderOptions) -> Self {
        Self {
            vault_root: options.vault_root,
            ignored_globs: options.ignored_globs,
            blocked_paths: options.blocked_paths,
        }
    }

    /// Whether a normalized vault path is ignored.
    #[must_use]
    pub fn is_ignored(&self, vault_path: &str) -> bool {
        path_matches_any_pattern(&self.ignored_globs, vault_path)
    }

    /// Whether a normalized vault path is blocked.
    #[must_use]
    pub fn is_blocked(&self, vault_path: &str) -> bool {
        path_matches_any_pattern(&self.blocked_paths, vault_path)
    }

    /// Read a note: content, hex SHA-256, and parsed markdown.
    ///
    /// # Errors
    /// [`VaultReaderError`] for blocked/ignored/non-markdown/missing paths.
    pub async fn read_note(&self, input: &str) -> Result<ReadNoteResult, VaultReaderError> {
        let normalized = normalize_vault_path(input)?;
        if self.is_blocked(&normalized) {
            return Err(VaultReaderError::Blocked);
        }
        if self.is_ignored(&normalized) {
            return Err(VaultReaderError::Ignored(normalized));
        }
        if !is_markdown_path(&normalized) {
            return Err(VaultReaderError::NotMarkdown(normalized));
        }
        let resolved = resolve_existing_vault_path(&self.vault_root, &normalized).await?;
        let content = tokio::fs::read_to_string(&resolved).await?;
        let current_sha256 = sha256_hex(&content);
        let parsed = parse_markdown(&content);
        Ok(ReadNoteResult {
            path: normalized,
            content,
            current_sha256,
            parsed,
        })
    }

    /// List a folder. Non-recursive yields immediate markdown files and
    /// subdirectories; recursive yields all markdown files (directories
    /// traversed, not emitted). Ignored/blocked/symlink entries are skipped.
    ///
    /// # Errors
    /// [`VaultReaderError`] on path validation or filesystem failure.
    pub async fn list_folder(
        &self,
        input: &str,
        recursive: bool,
    ) -> Result<Vec<FolderEntry>, VaultReaderError> {
        let normalized = normalize_vault_path(input)?;
        let start = if normalized.is_empty() {
            self.vault_root.clone()
        } else {
            self.vault_root.join(&normalized)
        };

        let mut entries = Vec::new();
        let mut stack = vec![start];
        while let Some(dir) = stack.pop() {
            let Ok(mut read_dir) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            while let Some(entry) = read_dir.next_entry().await? {
                let full = entry.path();
                let metadata = tokio::fs::symlink_metadata(&full).await?;
                let Some(relative) = self.relative_path(&full) else {
                    continue;
                };
                if self.is_ignored(&relative) || self.is_blocked(&relative) {
                    continue;
                }
                if metadata.is_dir() {
                    if recursive {
                        stack.push(full);
                    } else {
                        entries.push(FolderEntry {
                            path: relative,
                            kind: EntryKind::Directory,
                        });
                    }
                } else if metadata.is_file() && is_markdown_path(&relative) {
                    entries.push(FolderEntry {
                        path: relative,
                        kind: EntryKind::File,
                    });
                }
            }
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }

    fn relative_path(&self, full: &Path) -> Option<String> {
        let relative = full.strip_prefix(&self.vault_root).ok()?;
        Some(relative.to_string_lossy().replace('\\', "/"))
    }
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn vault() -> (tempfile::TempDir, VaultReader) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir_all(root.join("Notes")).await.unwrap();
        tokio::fs::create_dir_all(root.join("Private"))
            .await
            .unwrap();
        tokio::fs::write(root.join("Notes/a.md"), "# A\n\n#tag body")
            .await
            .unwrap();
        tokio::fs::write(root.join("Notes/skip.txt"), "not md")
            .await
            .unwrap();
        tokio::fs::write(root.join("Private/secret.md"), "# S")
            .await
            .unwrap();
        tokio::fs::write(root.join("Notes/.DS_Store"), "junk")
            .await
            .unwrap();
        let reader = VaultReader::new(VaultReaderOptions {
            vault_root: root.to_path_buf(),
            ignored_globs: vec!["**/.DS_Store".to_owned()],
            blocked_paths: vec!["Private/**".to_owned()],
        });
        (dir, reader)
    }

    #[tokio::test]
    async fn reads_note_with_hash_and_parse() {
        let (_dir, reader) = vault().await;
        let result = reader.read_note("Notes/a.md").await.unwrap();
        assert_eq!(result.path, "Notes/a.md");
        assert_eq!(result.current_sha256.len(), 64);
        assert_eq!(result.parsed.title.as_deref(), Some("A"));
        assert!(result.parsed.tags.contains(&"tag".to_owned()));
    }

    #[tokio::test]
    async fn rejects_blocked_ignored_and_non_markdown() {
        let (_dir, reader) = vault().await;
        assert!(matches!(
            reader.read_note("Private/secret.md").await,
            Err(VaultReaderError::Blocked)
        ));
        assert!(matches!(
            reader.read_note("Notes/.DS_Store").await,
            Err(VaultReaderError::Ignored(_))
        ));
        assert!(matches!(
            reader.read_note("Notes/skip.txt").await,
            Err(VaultReaderError::NotMarkdown(_))
        ));
    }

    #[tokio::test]
    async fn lists_recursively_skipping_blocked_and_non_markdown() {
        let (_dir, reader) = vault().await;
        let entries = reader.list_folder("", true).await.unwrap();
        let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, vec!["Notes/a.md"]);
    }

    #[tokio::test]
    async fn lists_top_level_files_and_dirs_non_recursive() {
        let (_dir, reader) = vault().await;
        let entries = reader.list_folder("", false).await.unwrap();
        let dirs: Vec<&str> = entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::Directory)
            .map(|entry| entry.path.as_str())
            .collect();
        assert!(dirs.contains(&"Notes"));
        assert!(!dirs.contains(&"Private")); // blocked
    }
}
