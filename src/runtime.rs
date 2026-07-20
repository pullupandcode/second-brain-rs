//! Runtime: builds the vault reader and index, and dispatches tool calls.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::{Map, Value, json};

use crate::{
    config::ServerConfig,
    vault::{
        index::{ConflictPair, NoteRecord, SearchFilters, VaultIndex, canonical_conflict_path},
        path::is_markdown_path,
        reader::{VaultReader, VaultReaderOptions},
    },
};

/// Errors from building the runtime.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// Filesystem error during setup.
    #[error("runtime io error: {0}")]
    Io(#[from] std::io::Error),
    /// Vault read error during the cold rebuild.
    #[error("runtime read error")]
    Read(#[from] crate::vault::reader::VaultReaderError),
    /// Index error.
    #[error("runtime index error")]
    Index(#[from] crate::vault::index::IndexError),
    /// A blocking task failed to join.
    #[error("runtime task error")]
    Join(#[from] tokio::task::JoinError),
}

/// Errors from dispatching a tool call.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DispatchError {
    /// No such tool in the dispatch table.
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    /// Tool exists but its handler is not implemented yet (later phase).
    #[error("not_implemented")]
    NotImplemented,
    /// Invalid arguments (client-facing message).
    #[error("{0}")]
    Invalid(String),
    /// Internal failure (message is sanitized before reaching the client).
    #[error("{0}")]
    Internal(String),
}

/// Shared runtime state: reader, index, and config.
pub struct Runtime {
    config: Arc<ServerConfig>,
    reader: VaultReader,
    index: Arc<VaultIndex>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").finish_non_exhaustive()
    }
}

impl Runtime {
    /// Build the runtime: prepare state dirs, open the index, cold-rebuild it.
    ///
    /// # Errors
    /// [`RuntimeError`] on filesystem, read, or index failure.
    pub async fn create(config: Arc<ServerConfig>) -> Result<Arc<Self>, RuntimeError> {
        tokio::fs::create_dir_all(&config.state_path).await?;
        if config.index.sqlite_path != ":memory:"
            && let Some(parent) = Path::new(&config.index.sqlite_path).parent()
        {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut ignored = config.index.ignored_globs.clone();
        ignored.extend(config.index.blocked_paths.iter().cloned());
        let reader = VaultReader::new(VaultReaderOptions {
            vault_root: PathBuf::from(&config.vault_path),
            ignored_globs: ignored,
            blocked_paths: config.security.blocked_paths.clone(),
        });

        let sqlite_path = config.index.sqlite_path.clone();
        let blocked = config.security.blocked_paths.clone();
        let index =
            tokio::task::spawn_blocking(move || VaultIndex::open(&sqlite_path, blocked)).await??;
        let index = Arc::new(index);

        let runtime = Arc::new(Self {
            config,
            reader,
            index,
        });
        runtime.cold_rebuild().await?;
        Ok(runtime)
    }

    async fn cold_rebuild(&self) -> Result<(), RuntimeError> {
        let files = self.reader.list_folder("", true).await?;
        let mut notes = Vec::with_capacity(files.len());
        for entry in files {
            let read = self.reader.read_note(&entry.path).await?;
            let parsed = read.parsed;
            notes.push(NoteRecord {
                path: read.path,
                title: parsed.title,
                content: read.content,
                tags: parsed.tags,
                aliases: parsed.aliases,
                source_id: parsed.source_id,
                sha256: read.current_sha256,
                outgoing_links: parsed.outgoing_links,
            });
        }
        let conflicts = scan_conflicts(Path::new(&self.config.vault_path)).await?;
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || index.rebuild(&notes, &conflicts)).await??;
        Ok(())
    }

    /// Dispatch a read tool call, returning its structured result value.
    ///
    /// # Errors
    /// [`DispatchError`] for unknown/unimplemented tools, invalid arguments, or
    /// internal failures (already sanitized).
    pub async fn dispatch(
        &self,
        name: &str,
        args: &Map<String, Value>,
    ) -> Result<Value, DispatchError> {
        match name {
            "read_note" => self.read_note(args).await,
            "list_folder" => self.list_folder(args).await,
            "search" => self.search(args).await,
            "get_backlinks" => self.get_backlinks(args).await,
            "get_outgoing_links" => self.get_outgoing_links(args).await,
            "list_vault_conflicts" => self.list_vault_conflicts().await,
            "link_to_page" => self.link_to_page(args).await,
            "get_vault_structure" => self.get_vault_structure().await,
            // Write / framework / capture / daily / OCR tools arrive in later phases.
            _ if is_known_later_tool(name) => Err(DispatchError::NotImplemented),
            other => Err(DispatchError::UnknownTool(other.to_owned())),
        }
    }

    async fn read_note(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let path = require_string(args, "path")?;
        let result = self
            .reader
            .read_note(&path)
            .await
            .map_err(|error| self.reader_error(&error))?;
        to_value(&result)
    }

    async fn list_folder(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let path = require_string(args, "path")?;
        let recursive = optional_bool(args, "recursive")?.unwrap_or(false);
        let entries = self
            .reader
            .list_folder(&path, recursive)
            .await
            .map_err(|error| self.reader_error(&error))?;
        to_value(&entries)
    }

    async fn search(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let query = require_string(args, "query")?;
        let filters = args.get("filters").and_then(Value::as_object);
        let search_filters = SearchFilters {
            folder: filters
                .and_then(|f| f.get("folder"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            tag: filters
                .and_then(|f| f.get("tag"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        };
        let index = Arc::clone(&self.index);
        let results = tokio::task::spawn_blocking(move || index.search(&query, &search_filters))
            .await
            .map_err(|error| DispatchError::Internal(error.to_string()))?
            .map_err(|error| self.index_error(&error))?;
        to_value(&results)
    }

    async fn get_backlinks(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let path = require_string(args, "path")?;
        let index = Arc::clone(&self.index);
        let links = tokio::task::spawn_blocking(move || index.backlinks(&path))
            .await
            .map_err(|error| DispatchError::Internal(error.to_string()))?
            .map_err(|error| self.index_error(&error))?;
        Ok(json!({ "backlinks": links }))
    }

    async fn get_outgoing_links(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let path = require_string(args, "path")?;
        let index = Arc::clone(&self.index);
        let links = tokio::task::spawn_blocking(move || index.outgoing_links(&path))
            .await
            .map_err(|error| DispatchError::Internal(error.to_string()))?
            .map_err(|error| self.index_error(&error))?;
        Ok(json!({ "outgoingLinks": links }))
    }

    async fn list_vault_conflicts(&self) -> Result<Value, DispatchError> {
        let index = Arc::clone(&self.index);
        let conflicts = tokio::task::spawn_blocking(move || index.list_conflicts())
            .await
            .map_err(|error| DispatchError::Internal(error.to_string()))?
            .map_err(|error| self.index_error(&error))?;
        to_value(&json!({ "conflicts": conflicts }))
    }

    async fn link_to_page(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let notebook = require_string(args, "notebook")?;
        let page_uuid = require_string(args, "page_uuid")?;
        let notebook_uuid = notebook.strip_prefix("rmnotebook:").unwrap_or(&notebook);
        let source_id = format!("rmpage:{notebook_uuid}:{page_uuid}");
        let index = Arc::clone(&self.index);
        let path = tokio::task::spawn_blocking(move || index.find_by_source_id(&source_id))
            .await
            .map_err(|error| DispatchError::Internal(error.to_string()))?
            .map_err(|error| self.index_error(&error))?;
        let link = path.map(|path| format!("[[{path}|{page_uuid}]]"));
        Ok(json!({ "link": link }))
    }

    async fn get_vault_structure(&self) -> Result<Value, DispatchError> {
        let entries = self
            .reader
            .list_folder("", false)
            .await
            .map_err(|error| self.reader_error(&error))?;
        let folders = to_value(&entries)?;
        // Record types arrive with the framework schema in Phase 3.
        Ok(json!({ "folders": folders, "recordTypes": [] }))
    }

    fn reader_error(&self, error: &crate::vault::reader::VaultReaderError) -> DispatchError {
        DispatchError::Invalid(self.sanitize(&error.to_string()))
    }

    fn index_error(&self, error: &crate::vault::index::IndexError) -> DispatchError {
        DispatchError::Internal(self.sanitize(&error.to_string()))
    }

    /// Strip absolute vault/state/index paths from a message before it reaches
    /// the client.
    fn sanitize(&self, message: &str) -> String {
        let mut cleaned = message.to_owned();
        for secret in [
            self.config.vault_path.as_str(),
            self.config.state_path.as_str(),
            self.config.index.sqlite_path.as_str(),
        ] {
            if !secret.is_empty() {
                cleaned = cleaned.replace(secret, "<path>");
            }
        }
        cleaned
    }
}

async fn scan_conflicts(vault_root: &Path) -> Result<Vec<ConflictPair>, RuntimeError> {
    let mut pairs = Vec::new();
    let mut stack = vec![vault_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(mut read_dir) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Some(entry) = read_dir.next_entry().await? {
            let full = entry.path();
            let metadata = tokio::fs::symlink_metadata(&full).await?;
            if metadata.is_dir() {
                stack.push(full);
                continue;
            }
            let Ok(relative) = full.strip_prefix(vault_root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if is_markdown_path(&relative)
                && let Some(canonical) = canonical_conflict_path(&relative)
            {
                pairs.push(ConflictPair {
                    canonical,
                    conflict: relative,
                });
            }
        }
    }
    Ok(pairs)
}

fn is_known_later_tool(name: &str) -> bool {
    matches!(
        name,
        "create_note"
            | "replace_note"
            | "update_frontmatter"
            | "replace_section_by_marker"
            | "create_record"
            | "inbox_capture"
            | "capture_for_date"
            | "daily_note_get"
            | "daily_note_append"
            | "daily_note_repair_markers"
            | "list_record_types"
            | "find_maps"
            | "list_write_recovery_diagnostics"
            | "framework_init"
            | "framework_reload"
            | "framework_register"
            | "framework_unregister"
            | "framework_list"
            | "framework_compose"
            | "ocr_notebook"
            | "ocr_status"
            | "ocr_renumber_notebook"
    )
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, DispatchError> {
    serde_json::to_value(value).map_err(|error| DispatchError::Internal(error.to_string()))
}

fn require_string(args: &Map<String, Value>, key: &str) -> Result<String, DispatchError> {
    match args.get(key).and_then(Value::as_str) {
        Some(value) if !value.is_empty() => Ok(value.to_owned()),
        _ => Err(DispatchError::Invalid(format!(
            "{key} must be a non-empty string"
        ))),
    }
}

fn optional_bool(args: &Map<String, Value>, key: &str) -> Result<Option<bool>, DispatchError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(DispatchError::Invalid(format!("{key} must be a boolean"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;

    async fn runtime_with_vault() -> (tempfile::TempDir, Arc<Runtime>) {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        let state = dir.path().join("state");
        tokio::fs::create_dir_all(vault.join("Notes"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(vault.join("Private"))
            .await
            .unwrap();
        tokio::fs::write(
            vault.join("Notes/a.md"),
            "# Apple\n\nred #fruit see [[Notes/b]]",
        )
        .await
        .unwrap();
        tokio::fs::write(vault.join("Notes/b.md"), "# Banana\n\nyellow #fruit")
            .await
            .unwrap();
        tokio::fs::write(
            vault.join("Private/secret.md"),
            "# Secret\n\nclassified #fruit",
        )
        .await
        .unwrap();

        let toml = format!(
            "listen = \"127.0.0.1:0\"\npublic_base_url = \"http://127.0.0.1:3000\"\n\
             vault_path = \"{}\"\nstate_path = \"{}\"\n\
             [auth]\nmode = \"development\"\naudience = \"a\"\n\
             trusted_issuers = [\"https://i.example.com/\"]\n\
             discovery_authorization_server = \"https://i.example.com/\"\n\
             jwks_cache_ttl_seconds = 60\n\
             [index]\nsqlite_path = \":memory:\"\nwatcher_polling = false\n\
             ignored_globs = [\"**/*.sync-conflict-*\"]\n\
             [security]\nblocked_paths = [\"Private/**\"]\n\
             [writes]\ncooldown_seconds = 0\n[daily_note]\ncapture_default_pattern = \"B\"\n\
             [logging]\nlog_args = false\n",
            vault.display(),
            state.display()
        );
        let config = Arc::new(parse_config(&toml).unwrap());
        let runtime = Runtime::create(config).await.unwrap();
        (dir, runtime)
    }

    #[tokio::test]
    async fn dispatches_read_note() {
        let (_dir, runtime) = runtime_with_vault().await;
        let args = json!({ "path": "Notes/a.md" }).as_object().unwrap().clone();
        let value = runtime.dispatch("read_note", &args).await.unwrap();
        assert_eq!(
            value
                .get("currentSha256")
                .and_then(Value::as_str)
                .map(str::len),
            Some(64)
        );
        assert_eq!(
            value.pointer("/parsed/title").and_then(Value::as_str),
            Some("Apple")
        );
    }

    #[tokio::test]
    async fn search_and_backlinks_work_and_block_private() {
        let (_dir, runtime) = runtime_with_vault().await;
        let args = json!({ "query": "fruit" }).as_object().unwrap().clone();
        let value = runtime.dispatch("search", &args).await.unwrap();
        let paths: Vec<&str> = value
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.get("path").and_then(Value::as_str))
            .collect();
        assert!(paths.contains(&"Notes/a.md"));
        assert!(!paths.iter().any(|p| p.starts_with("Private/")));

        let args = json!({ "path": "Notes/b" }).as_object().unwrap().clone();
        let value = runtime.dispatch("get_backlinks", &args).await.unwrap();
        assert_eq!(
            value.pointer("/backlinks/0").and_then(Value::as_str),
            Some("Notes/a.md")
        );
    }

    #[tokio::test]
    async fn invalid_args_and_unknown_and_not_implemented() {
        let (_dir, runtime) = runtime_with_vault().await;
        let empty = Map::new();
        assert!(matches!(
            runtime.dispatch("read_note", &empty).await,
            Err(DispatchError::Invalid(_))
        ));
        assert!(matches!(
            runtime.dispatch("create_note", &empty).await,
            Err(DispatchError::NotImplemented)
        ));
        assert!(matches!(
            runtime.dispatch("nope", &empty).await,
            Err(DispatchError::UnknownTool(_))
        ));
    }
}
