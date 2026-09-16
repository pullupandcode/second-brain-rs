//! Audited, atomic vault mutations with optimistic concurrency and path policy.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::{Duration, SystemTime},
};

use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{io::AsyncWriteExt, sync::Mutex};

use super::{
    audit::{AuditInput, VaultWriteAuditStore, unique_id},
    markdown::{FrontmatterValue, parse_markdown},
    path::{normalize_vault_path, resolve_vault_path_for_write},
    policy::PathPolicy,
};

/// A classified mutation failure, safe to expose to clients.
#[derive(Debug, thiserror::Error, Serialize)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct VaultWriteError {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_sha256: Option<String>,
}
impl VaultWriteError {
    /// Stable protocol error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
    /// Current hash when optimistic concurrency detects a stale base.
    #[must_use]
    pub fn current_sha256(&self) -> Option<&str> {
        self.current_sha256.as_deref()
    }
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            current_sha256: None,
        }
    }
}
impl From<std::io::Error> for VaultWriteError {
    fn from(_: std::io::Error) -> Self {
        Self::new("write_failed", "Vault write failed")
    }
}
impl From<super::path::VaultPathError> for VaultWriteError {
    fn from(error: super::path::VaultPathError) -> Self {
        Self::new("invalid_path", error.to_string())
    }
}
/// Mutation result; hashes describe exact on-disk UTF-8 bytes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    /// Normalized vault-relative path.
    pub path: String,
    /// Trash destination after a soft deletion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_path: Option<String>,
    /// Hash supplied for optimistic concurrency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_sha256: Option<String>,
    /// New hash (original hash for deletion).
    pub result_sha256: String,
}
/// Shared writer policy and filesystem configuration.
#[derive(Debug, Clone)]
pub struct VaultWriterOptions {
    /// Existing root directory.
    pub vault_root: PathBuf,
    /// Minimum file age for an update or deletion.
    pub cooldown_seconds: u64,
    /// Reloadable effective hard-block policy.
    pub blocked_paths: PathPolicy,
    /// Canonical paths quarantined by sync conflicts.
    pub quarantined_paths: HashSet<String>,
    /// Vault-relative trash directory.
    pub trash_path: String,
}
/// One shared instance serializes mutations per normalized path.
#[derive(Debug)]
pub struct VaultWriter {
    options: VaultWriterOptions,
    locks: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    audit: Option<Arc<VaultWriteAuditStore>>,
}
enum Mutation<'a> {
    Create(&'a str, Option<&'a BTreeMap<String, FrontmatterValue>>),
    Replace(&'a str, Option<&'a BTreeMap<String, FrontmatterValue>>),
    Frontmatter(&'a BTreeMap<String, FrontmatterValue>),
    Marker(&'a str, &'a str),
    Delete,
    HardDelete,
}
impl Mutation<'_> {
    const fn name(&self) -> &'static str {
        match self {
            Self::Create(..) => "create_note",
            Self::Replace(..) => "replace_note",
            Self::Frontmatter(..) => "update_frontmatter",
            Self::Marker(..) => "replace_section_by_marker",
            Self::Delete => "delete_note",
            Self::HardDelete => "hard_delete_note",
        }
    }
}
impl VaultWriter {
    /// Construct a writer. Share the instance to share path serialization.
    #[must_use]
    pub fn new(options: VaultWriterOptions, audit: Option<Arc<VaultWriteAuditStore>>) -> Self {
        Self {
            options,
            locks: Mutex::new(HashMap::new()),
            audit,
        }
    }
    /// Create a note (or framework metadata file) without overwriting an existing path.
    /// # Errors
    /// Returns policy, filesystem, serialization, or existence errors.
    // NOT cancel-safe: interrupted operations leave an incomplete audit diagnostic.
    pub async fn create_note(
        &self,
        path: &str,
        content: &str,
        frontmatter: Option<&BTreeMap<String, FrontmatterValue>>,
    ) -> Result<WriteResult, VaultWriteError> {
        self.mutate(path, None, Mutation::Create(content, frontmatter))
            .await
    }
    /// Replace a file after validating its base hash and cooldown.
    /// # Errors
    /// Returns policy, filesystem, or optimistic-concurrency errors.
    // NOT cancel-safe: interrupted operations leave an incomplete audit diagnostic.
    pub async fn replace_note(
        &self,
        path: &str,
        content: &str,
        base_sha256: &str,
        frontmatter: Option<&BTreeMap<String, FrontmatterValue>>,
    ) -> Result<WriteResult, VaultWriteError> {
        self.mutate(
            path,
            Some(base_sha256),
            Mutation::Replace(content, frontmatter),
        )
        .await
    }
    /// Merge frontmatter keys while preserving the note body.
    /// # Errors
    /// Returns write guards or invalid-frontmatter errors.
    // NOT cancel-safe: interrupted operations leave an incomplete audit diagnostic.
    pub async fn update_frontmatter(
        &self,
        path: &str,
        updates: &BTreeMap<String, FrontmatterValue>,
        base_sha256: &str,
    ) -> Result<WriteResult, VaultWriteError> {
        self.mutate(path, Some(base_sha256), Mutation::Frontmatter(updates))
            .await
    }
    /// Replace the content between an existing ordered marker pair.
    /// # Errors
    /// Returns write guards or `markers_missing`.
    // NOT cancel-safe: interrupted operations leave an incomplete audit diagnostic.
    pub async fn replace_section_by_marker(
        &self,
        path: &str,
        marker: &str,
        content: &str,
        base_sha256: &str,
    ) -> Result<WriteResult, VaultWriteError> {
        self.mutate(path, Some(base_sha256), Mutation::Marker(marker, content))
            .await
    }
    /// Move a note into the configured trash without overwriting older trash.
    /// # Errors
    /// Returns write guards or filesystem errors.
    // NOT cancel-safe: interrupted operations leave an incomplete audit diagnostic.
    pub async fn delete_note(
        &self,
        path: &str,
        base_sha256: &str,
    ) -> Result<WriteResult, VaultWriteError> {
        self.mutate(path, Some(base_sha256), Mutation::Delete).await
    }
    /// Permanently remove a note after validating its base hash and cooldown.
    /// # Errors
    /// Returns write guards or filesystem errors.
    // NOT cancel-safe: interrupted operations leave an incomplete audit diagnostic.
    pub async fn hard_delete_note(
        &self,
        path: &str,
        base_sha256: &str,
    ) -> Result<WriteResult, VaultWriteError> {
        self.mutate(path, Some(base_sha256), Mutation::HardDelete)
            .await
    }
    async fn mutate(
        &self,
        path: &str,
        base: Option<&str>,
        mutation: Mutation<'_>,
    ) -> Result<WriteResult, VaultWriteError> {
        let path = normalize_vault_path(path)?;
        if path.is_empty() {
            return Err(VaultWriteError::new(
                "invalid_path",
                "Vault path must name a file",
            ));
        }
        let lock = {
            let mut locks = self.locks.lock().await;
            locks.retain(|_, lock| lock.strong_count() > 0);
            locks.get(&path).and_then(Weak::upgrade).unwrap_or_else(|| {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(path.clone(), Arc::downgrade(&lock));
                lock
            })
        };
        let _guard = lock.lock().await;
        let metadata = match &mutation {
            Mutation::Marker(marker, _) => json!({"markerName":marker}),
            Mutation::Create(..)
            | Mutation::Replace(..)
            | Mutation::Frontmatter(_)
            | Mutation::Delete
            | Mutation::HardDelete => json!({}),
        };
        let input = AuditInput {
            operation: mutation.name().into(),
            path: path.clone(),
            base_sha256: base.map(str::to_owned),
            metadata,
        };
        let attempt = if let Some(audit) = &self.audit {
            let audit = Arc::clone(audit);
            let input = input.clone();
            tokio::task::spawn_blocking(move || audit.record_write_started(&input))
                .await
                .ok()
                .and_then(Result::ok)
        } else {
            None
        };
        let result = self.apply(&path, base, mutation).await;
        if let Some(audit) = &self.audit {
            let audit = Arc::clone(audit);
            let hash = result.as_ref().ok().map(|v| v.result_sha256.clone());
            let error = result.as_ref().err().map(ToString::to_string);
            let _completion = tokio::task::spawn_blocking(move || {
                if let Some(hash) = hash {
                    if let Some(attempt) = attempt
                        && let Err(error) = audit.record_write_succeeded(&attempt, &hash)
                    {
                        tracing::error!(%error,"write audit completion failed");
                    }
                    if let Err(error) = audit.record_write(&input, &hash) {
                        tracing::error!(%error,"write provenance failed");
                    }
                } else if let (Some(attempt), Some(error)) = (attempt, error)
                    && let Err(error) = audit.record_write_failed(&attempt, &error)
                {
                    tracing::error!(%error,"write audit failure event failed");
                }
            })
            .await;
        }
        result
    }
    async fn guard(&self, path: &str) -> Result<PathBuf, VaultWriteError> {
        if self.options.blocked_paths.is_blocked(path) {
            return Err(VaultWriteError::new(
                "path_blocked",
                "Vault path is blocked",
            ));
        }
        if self.options.quarantined_paths.contains(path)
            || super::index::canonical_conflict_path(path).is_some()
        {
            return Err(VaultWriteError::new(
                "path_quarantined",
                format!("Path is quarantined: {path}"),
            ));
        }
        let absolute = resolve_vault_path_for_write(&self.options.vault_root, path).await?;
        // Refuse symlink aliases, including aliases to otherwise blocked in-vault paths.
        let mut segment = self.options.vault_root.clone();
        for component in path.split('/') {
            segment.push(component);
            match tokio::fs::symlink_metadata(&segment).await {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(VaultWriteError::new(
                        "invalid_path",
                        "Vault write path must not traverse symlinks",
                    ));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(absolute)
    }
    async fn apply(
        &self,
        path: &str,
        base: Option<&str>,
        mutation: Mutation<'_>,
    ) -> Result<WriteResult, VaultWriteError> {
        let absolute = self.guard(path).await?;
        let mut result = WriteResult {
            path: path.to_owned(),
            deleted_path: None,
            base_sha256: base.map(str::to_owned),
            result_sha256: String::new(),
        };
        if let Mutation::Create(content, frontmatter) = mutation {
            if tokio::fs::try_exists(&absolute).await? {
                return Err(VaultWriteError::new(
                    "path_exists",
                    format!("Path already exists: {path}"),
                ));
            }
            let content = with_frontmatter(content, frontmatter)?;
            self.atomic_write(path, &absolute, &content, true).await?;
            result.result_sha256 = sha256(&content);
            return Ok(result);
        }
        let meta = tokio::fs::metadata(&absolute).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                VaultWriteError::new("path_missing", format!("Path does not exist: {path}"))
            } else {
                error.into()
            }
        })?;
        if self.options.cooldown_seconds > 0
            && SystemTime::now()
                .duration_since(meta.modified()?)
                .unwrap_or_default()
                < Duration::from_secs(self.options.cooldown_seconds)
        {
            return Err(VaultWriteError::new(
                "retryable_conflict",
                "Path is inside write cooldown window",
            ));
        }
        let current = tokio::fs::read_to_string(&absolute).await?;
        let hash = sha256(&current);
        if Some(hash.as_str()) != base {
            return Err(VaultWriteError {
                code: "retryable_conflict",
                message: format!("Stale base_sha256 for path: {path}"),
                current_sha256: Some(hash),
            });
        }
        let next = match mutation {
            Mutation::Create(..) => {
                return Err(VaultWriteError::new(
                    "write_failed",
                    "Unexpected create operation",
                ));
            }
            Mutation::Replace(content, fm) => with_frontmatter(content, fm)?,
            Mutation::Frontmatter(updates) => {
                let mut parsed = parse_markdown(&current);
                parsed.frontmatter.extend(
                    updates
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
                with_frontmatter(&parsed.body, Some(&parsed.frontmatter))?
            }
            Mutation::Marker(marker, content) => replace_marker(&current, marker, content)?,
            Mutation::Delete => {
                let deleted = self.move_to_trash(path, &absolute).await?;
                result.deleted_path = Some(deleted);
                result.result_sha256 = hash;
                return Ok(result);
            }
            Mutation::HardDelete => {
                self.guard(path).await?;
                tokio::fs::remove_file(&absolute).await?;
                result.result_sha256 = hash;
                return Ok(result);
            }
        };
        self.atomic_write(path, &absolute, &next, false).await?;
        result.result_sha256 = sha256(&next);
        Ok(result)
    }
    async fn move_to_trash(&self, path: &str, absolute: &Path) -> Result<String, VaultWriteError> {
        let initial = normalize_vault_path(&format!("{}/{path}", self.options.trash_path))?;
        let mut target = initial;
        loop {
            let destination = self.guard(&target).await?;
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            self.guard(&target).await?;
            self.guard(path).await?;
            // Linking atomically reserves the destination and cannot overwrite old trash.
            match tokio::fs::hard_link(absolute, &destination).await {
                Ok(()) => {
                    tokio::fs::remove_file(absolute).await?;
                    return Ok(target);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let p = Path::new(path);
                    let stem = p.file_stem().and_then(|v| v.to_str()).unwrap_or("note");
                    let extension = p
                        .extension()
                        .and_then(|v| v.to_str())
                        .map_or_else(String::new, |v| format!(".{v}"));
                    let parent = p.parent().and_then(|v| v.to_str()).unwrap_or("");
                    target = normalize_vault_path(&format!(
                        "{}/{parent}/{stem}.{}{extension}",
                        self.options.trash_path,
                        unique_id()
                    ))?;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    async fn atomic_write(
        &self,
        path: &str,
        absolute: &Path,
        content: &str,
        create: bool,
    ) -> Result<(), VaultWriteError> {
        if let Some(parent) = absolute.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        self.guard(path).await?;
        let temp = absolute.with_file_name(format!(
            ".{}.{}.tmp",
            absolute
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("note"),
            unique_id()
        ));
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .await?;
        let prepared = async {
            file.write_all(content.as_bytes()).await?;
            file.sync_all().await?;
            Ok::<(), std::io::Error>(())
        }
        .await;
        drop(file);
        let result = async {
            prepared?;
            self.guard(path).await?;
            if create {
                tokio::fs::hard_link(&temp, absolute).await.map_err(|e| {
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        VaultWriteError::new("path_exists", format!("Path already exists: {path}"))
                    } else {
                        e.into()
                    }
                })?;
            } else {
                tokio::fs::rename(&temp, absolute).await?;
            }
            Ok(())
        }
        .await;
        if let Err(e) = tokio::fs::remove_file(&temp).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(error=%e,"temporary write cleanup failed");
        }
        result
    }
}
fn sha256(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}
fn replace_marker(current: &str, marker: &str, content: &str) -> Result<String, VaultWriteError> {
    let start_marker = format!("<!-- mcp:section {marker} start -->");
    let end_marker = format!("<!-- mcp:section {marker} end -->");
    let pair = current
        .find(&start_marker)
        .zip(current.find(&end_marker))
        .filter(|(start, end)| start < end);
    let Some((start, end)) = pair else {
        return Err(VaultWriteError::new(
            "markers_missing",
            format!("Markers missing for section: {marker}"),
        ));
    };
    Ok(format!(
        "{}\n{content}\n{}",
        current.get(..start + start_marker.len()).unwrap_or(""),
        current.get(end..).unwrap_or("")
    ))
}
/// Serialize reference-compatible scalar/list frontmatter around content.
/// # Errors
/// Rejects invalid keys and non-finite numeric values.
pub fn with_frontmatter(
    content: &str,
    frontmatter: Option<&BTreeMap<String, FrontmatterValue>>,
) -> Result<String, VaultWriteError> {
    let Some(frontmatter) = frontmatter.filter(|map| !map.is_empty()) else {
        return Ok(content.to_owned());
    };
    let mut output = String::from("---\n");
    for (key, value) in frontmatter {
        if key.is_empty()
            || !key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(VaultWriteError::new(
                "invalid_frontmatter",
                format!("Invalid frontmatter key: {key}"),
            ));
        }
        let encoded = match value {
            FrontmatterValue::Number(number) if !number.is_finite() => {
                return Err(VaultWriteError::new(
                    "invalid_frontmatter",
                    "Frontmatter numbers must be finite",
                ));
            }
            FrontmatterValue::Number(number) => number.to_string(),
            FrontmatterValue::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(|v| json!(v).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            FrontmatterValue::String(text) => json!(text).to_string(),
            FrontmatterValue::Bool(value) => value.to_string(),
        };
        output.push_str(key);
        output.push_str(": ");
        output.push_str(&encoded);
        output.push('\n');
    }
    output.push_str("---\n");
    output.push_str(content);
    Ok(output)
}
