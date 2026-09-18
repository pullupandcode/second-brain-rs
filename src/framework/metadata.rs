//! Atomic metadata persistence, independent of note cooldown and audit semantics.

use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::{Framework, invalid};
use crate::{
    runtime::DispatchError,
    vault::path::{canonical_vault_path_for_write, is_markdown_path},
};

impl Framework {
    // NOT cancel-safe: callers hold the framework metadata lock until publication and cleanup.
    pub(super) async fn write_metadata(
        &self,
        path: &str,
        source: &str,
        overwrite: bool,
    ) -> Result<bool, DispatchError> {
        let normalized = self.check_path(path)?;
        if is_markdown_path(&normalized) {
            return self
                .write_markdown_schema(&normalized, source, overwrite)
                .await;
        }
        if normalized.is_empty() {
            return Err(invalid("Metadata path must identify a file"));
        }
        let (canonical, target) = canonical_vault_path_for_write(&self.root, &normalized)
            .await
            .map_err(|e| invalid(e.to_string()))?;
        self.check_path(&canonical)?;
        // Do not follow symlinks through metadata writes. This also closes aliases to blocked paths.
        let mut component = self.root.clone();
        for segment in normalized.split('/') {
            component.push(segment);
            match tokio::fs::symlink_metadata(&component).await {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(invalid("Metadata path contains a symlink"));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(invalid("Metadata path could not be resolved")),
            }
        }
        let existed = tokio::fs::try_exists(&target)
            .await
            .map_err(|_| invalid("Metadata path could not be resolved"))?;
        if existed && !overwrite {
            return Err(crate::vault::writer::VaultWriteError::new(
                "path_exists",
                format!("Path already exists: {path}"),
            )
            .into());
        }
        let parent = target
            .parent()
            .ok_or_else(|| invalid("Invalid metadata path"))?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| invalid("Metadata directory creation failed"))?;
        let temporary = parent.join(format!(".framework-{}.tmp", Uuid::new_v4()));
        let result = async {
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .await
                .map_err(|_| invalid("Metadata temporary file creation failed"))?;
            file.write_all(source.as_bytes())
                .await
                .map_err(|_| invalid("Metadata write failed"))?;
            file.sync_all()
                .await
                .map_err(|_| invalid("Metadata flush failed"))?;
            drop(file);
            self.check_path(&normalized)?;
            // Revalidate ancestry after preparing the complete file, before publishing it.
            let (current_canonical, current_target) =
                canonical_vault_path_for_write(&self.root, &normalized)
                    .await
                    .map_err(|e| invalid(e.to_string()))?;
            self.check_path(&normalized)?;
            self.check_path(&current_canonical)?;
            if canonical != current_canonical || target != current_target {
                return Err(invalid("Metadata path changed during publication"));
            }
            if overwrite {
                tokio::fs::rename(&temporary, &target)
                    .await
                    .map_err(|_| invalid("Metadata publication failed"))?;
            } else {
                tokio::fs::hard_link(&temporary, &target)
                    .await
                    .map_err(|error| {
                        if error.kind() == std::io::ErrorKind::AlreadyExists {
                            crate::vault::writer::VaultWriteError::new(
                                "path_exists",
                                format!("Path already exists: {path}"),
                            )
                            .into()
                        } else {
                            invalid("Metadata publication failed")
                        }
                    })?;
            }
            Ok(existed)
        }
        .await;
        match tokio::fs::remove_file(&temporary).await {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) if result.is_ok() => {
                return Err(invalid("Metadata temporary file cleanup failed"));
            }
            Ok(()) | Err(_) => {}
        }
        result
    }
}

impl Framework {
    // NOT cancel-safe: note content always uses the audited writer, even for a schema.
    async fn write_markdown_schema(
        &self,
        path: &str,
        source: &str,
        overwrite: bool,
    ) -> Result<bool, DispatchError> {
        use sha2::{Digest, Sha256};
        let current = if overwrite {
            self.read_optional_text(path).await?
        } else {
            None
        };
        if let Some(content) = &current {
            let base = hex::encode(Sha256::digest(content.as_bytes()));
            self.writer.replace_note(path, source, &base, None).await?;
        } else {
            self.writer.create_note(path, source, None).await?;
        }
        Ok(current.is_some())
    }
}
