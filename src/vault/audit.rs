//! Append-only write provenance and recoverable lifecycle records.
#![allow(clippy::significant_drop_tightening)]
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use rusqlite::{Connection, params};
use serde::Serialize;
use serde_json::{Value, json};

/// Persistence failures; filesystem details are never exposed to clients.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    /// Database operation failed.
    #[error("audit database error")]
    Database(#[from] rusqlite::Error),
    /// Filesystem operation failed.
    #[error("audit filesystem error")]
    Io(#[from] std::io::Error),
    /// Background task failed.
    #[error("audit task error")]
    Join(#[from] tokio::task::JoinError),
}
/// Provenance for a write attempt. Note content is deliberately excluded.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditInput {
    /// Primitive operation name.
    pub operation: String,
    /// Vault-relative target.
    pub path: String,
    /// Expected old content hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_sha256: Option<String>,
    /// Non-content operation metadata.
    pub metadata: Value,
}
/// SQLite-backed append-only audit store. Use from blocking worker threads.
#[derive(Debug)]
pub struct VaultWriteAuditStore {
    db: Mutex<Connection>,
}
impl VaultWriteAuditStore {
    /// Open or initialize a durable audit database.
    /// # Errors
    /// Returns an error if SQLite cannot open or initialize the database.
    pub fn open(path: &Path) -> Result<Self, AuditError> {
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
CREATE TABLE IF NOT EXISTS write_audit(id INTEGER PRIMARY KEY AUTOINCREMENT,operation TEXT NOT NULL CHECK(operation IN ('create_note','replace_note','update_frontmatter','replace_section_by_marker','delete_note','hard_delete_note')),path TEXT NOT NULL,base_sha256 TEXT,result_sha256 TEXT NOT NULL,metadata_json TEXT NOT NULL DEFAULT '{}',created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')));
CREATE TABLE IF NOT EXISTS write_audit_attempts(id INTEGER PRIMARY KEY AUTOINCREMENT,attempt_id TEXT NOT NULL,event_type TEXT NOT NULL CHECK(event_type IN ('started','succeeded','failed')),operation TEXT CHECK(operation IN ('create_note','replace_note','update_frontmatter','replace_section_by_marker','delete_note','hard_delete_note')),path TEXT,base_sha256 TEXT,result_sha256 TEXT,error_message TEXT,metadata_json TEXT NOT NULL DEFAULT '{}',created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),CHECK((event_type='started' AND operation IS NOT NULL AND path IS NOT NULL) OR (event_type='succeeded' AND result_sha256 IS NOT NULL) OR (event_type='failed' AND error_message IS NOT NULL)));
CREATE INDEX IF NOT EXISTS write_audit_attempts_attempt_id_idx ON write_audit_attempts(attempt_id);")?;
        for table in ["write_audit", "write_audit_attempts"] {
            for event in ["UPDATE", "DELETE"] {
                db.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS {table}_no_{event} BEFORE {event} ON {table} BEGIN SELECT RAISE(ABORT,'{table} is append-only'); END;"))?;
            }
            db.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS {table}_no_existing_id_insert BEFORE INSERT ON {table} WHEN NEW.id IS NOT NULL AND EXISTS(SELECT 1 FROM {table} WHERE id=NEW.id) BEGIN SELECT RAISE(ABORT,'{table} is append-only'); END;"))?;
        }
        Ok(Self { db: Mutex::new(db) })
    }
    /// Persist a start event before a filesystem mutation.
    /// # Errors
    /// Returns a database error on persistence failure.
    pub fn record_write_started(&self, input: &AuditInput) -> Result<String, AuditError> {
        let id = unique_id();
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        db.execute("INSERT INTO write_audit_attempts(attempt_id,event_type,operation,path,base_sha256,metadata_json) VALUES(?,'started',?,?,?,?)",params![id,input.operation,input.path,input.base_sha256,input.metadata.to_string()])?;
        Ok(id)
    }
    /// Persist a terminal success independently from successful-write provenance.
    /// # Errors
    /// Returns a database error on persistence failure.
    pub fn record_write_succeeded(&self, id: &str, hash: &str) -> Result<(), AuditError> {
        self.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).execute("INSERT INTO write_audit_attempts(attempt_id,event_type,result_sha256) VALUES(?,'succeeded',?)",params![id,hash])?;
        Ok(())
    }
    /// Persist a failed attempt without creating a successful-write row.
    /// # Errors
    /// Returns a database error on persistence failure.
    pub fn record_write_failed(&self, id: &str, message: &str) -> Result<(), AuditError> {
        self.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).execute("INSERT INTO write_audit_attempts(attempt_id,event_type,error_message) VALUES(?,'failed',?)",params![id,message])?;
        Ok(())
    }
    /// Persist hashes and metadata for a completed mutation.
    /// # Errors
    /// Returns a database error on persistence failure.
    pub fn record_write(&self, input: &AuditInput, hash: &str) -> Result<(), AuditError> {
        self.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).execute("INSERT INTO write_audit(operation,path,base_sha256,result_sha256,metadata_json) VALUES(?,?,?,?,?)",params![input.operation,input.path,input.base_sha256,hash,input.metadata.to_string()])?;
        Ok(())
    }
    /// Persist successful provenance and its optional terminal lifecycle event together.
    /// # Errors
    /// Returns a database error when completion cannot be persisted.
    pub fn record_write_completed(
        &self,
        input: &AuditInput,
        attempt_id: Option<&str>,
        hash: &str,
    ) -> Result<(), AuditError> {
        let mut db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transaction = db.transaction()?;
        if let Some(id) = attempt_id {
            transaction.execute("INSERT INTO write_audit_attempts(attempt_id,event_type,result_sha256) VALUES(?,'succeeded',?)", params![id, hash])?;
        }
        transaction.execute("INSERT INTO write_audit(operation,path,base_sha256,result_sha256,metadata_json) VALUES(?,?,?,?,?)", params![input.operation,input.path,input.base_sha256,hash,input.metadata.to_string()])?;
        transaction.commit()?;
        Ok(())
    }
    /// Return incomplete lifecycle starts, newest first, with a bounded limit.
    /// # Errors
    /// Returns a database error if the query fails.
    pub fn list_incomplete_writes(&self, limit: Option<i64>) -> Result<Vec<Value>, AuditError> {
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt=db.prepare("SELECT attempt_id,operation,path,base_sha256,metadata_json,created_at FROM write_audit_attempts s WHERE event_type='started' AND NOT EXISTS(SELECT 1 FROM write_audit_attempts t WHERE t.attempt_id=s.attempt_id AND t.event_type IN ('succeeded','failed')) ORDER BY id DESC LIMIT ?")?;
        let rows=stmt.query_map([limit.unwrap_or(100).clamp(1,1000)],|r|{let base:Option<String>=r.get(3)?;let raw:String=r.get(4)?;let mut v=json!({"attemptId":r.get::<_,String>(0)?,"operation":r.get::<_,String>(1)?,"path":r.get::<_,String>(2)?,"metadata":serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({})),"startedAt":r.get::<_,String>(5)?});if let Some(base)=base && let Some(map)=v.as_object_mut(){map.insert("baseSha256".into(),json!(base));}Ok(v)})?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    /// Return successful writes, newest first, with a bounded limit.
    /// # Errors
    /// Returns a database error if the query fails.
    pub fn list_recent_writes(&self, limit: Option<i64>) -> Result<Vec<Value>, AuditError> {
        let db = self
            .db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt=db.prepare("SELECT id,operation,path,base_sha256,result_sha256,metadata_json,created_at FROM write_audit ORDER BY id DESC LIMIT ?")?;
        let rows=stmt.query_map([limit.unwrap_or(100).clamp(1,1000)],|r|{let base:Option<String>=r.get(3)?;let raw:String=r.get(5)?;let mut v=json!({"id":r.get::<_,i64>(0)?,"operation":r.get::<_,String>(1)?,"path":r.get::<_,String>(2)?,"resultSha256":r.get::<_,String>(4)?,"metadata":serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({})),"createdAt":r.get::<_,String>(6)?});if let Some(base)=base && let Some(map)=v.as_object_mut(){map.insert("baseSha256".into(),json!(base));}Ok(v)})?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
/// Rotate before opening the live store when successful rows exceed retention.
///
/// Returns the archive path when rotation occurs. Retention is a soft limit:
/// unfinished lifecycle attempts defer rotation until explicitly reconciled, so
/// recovery diagnostics remain visible in the live store. Archives remain on the
/// same filesystem as the live store. Filename collisions gain a UUID suffix.
/// # Errors
/// Returns database, filesystem, or background-task failures.
// NOT cancel-safe: rename may complete before cancellation is observed. Cancellation
// after exclusive destination reservation can leave an empty archive placeholder;
// it contains no historical records and cannot overwrite an earlier archive.
pub async fn rotate_write_audit_if_needed(
    path: &Path,
    archive: &Path,
    max_rows: u64,
) -> Result<Option<PathBuf>, AuditError> {
    if max_rows == 0 || path == Path::new(":memory:") || !tokio::fs::try_exists(path).await? {
        return Ok(None);
    }
    let owned = path.to_path_buf();
    let can_rotate = tokio::task::spawn_blocking(move || -> Result<bool, rusqlite::Error> {
        let db = Connection::open(owned)?;
        let count: i64 = db.query_row("SELECT count(*) FROM write_audit", [], |r| r.get(0)).unwrap_or(0);
        if u64::try_from(count).unwrap_or(0) <= max_rows {
            return Ok(false);
        }
        let pending: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM write_audit_attempts s WHERE s.event_type='started' AND NOT EXISTS(SELECT 1 FROM write_audit_attempts t WHERE t.attempt_id=s.attempt_id AND t.event_type IN ('succeeded','failed')))", [], |r| r.get(0))?;
        Ok(!pending)
    }).await??;
    if !can_rotate {
        return Ok(None);
    }
    tokio::fs::create_dir_all(archive).await?;
    archive_at(path, archive, time::OffsetDateTime::now_utc())
        .await
        .map(Some)
}
async fn archive_at(
    path: &Path,
    archive: &Path,
    now: time::OffsetDateTime,
) -> Result<PathBuf, AuditError> {
    let timestamp = archive_timestamp(now);
    let mut destination = archive.join(format!("write-audit.{timestamp}.sqlite"));
    loop {
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .await
        {
            Ok(reservation) => {
                drop(reservation);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                destination =
                    archive.join(format!("write-audit.{timestamp}.{}.sqlite", unique_id()));
            }
            Err(error) => return Err(error.into()),
        }
    }
    // Replace only the empty file this call exclusively reserved. Rename avoids
    // an archived hard link remaining aliased to a live database after cancellation.
    if let Err(error) = tokio::fs::rename(path, &destination).await {
        let _cleanup = tokio::fs::remove_file(&destination).await;
        return Err(error.into());
    }
    Ok(destination)
}

pub(crate) fn unique_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn archive_timestamp(now: time::OffsetDateTime) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}{:03}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond()
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod recovery_tests {
    use super::*;

    #[tokio::test]
    async fn archive_reservation_returns_noncollision_errors_promptly() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("live.sqlite");
        tokio::fs::write(&source, b"live history").await.unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            archive_at(
                &source,
                &dir.path().join("missing-directory"),
                time::OffsetDateTime::UNIX_EPOCH,
            ),
        )
        .await
        .unwrap();
        assert!(
            matches!(result, Err(AuditError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
        );
        assert_eq!(tokio::fs::read(&source).await.unwrap(), b"live history");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn failed_archive_rename_cleans_up_only_its_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("missing.sqlite");
        let now = time::OffsetDateTime::UNIX_EPOCH;
        let historical = dir
            .path()
            .join(format!("write-audit.{}.sqlite", archive_timestamp(now)));
        tokio::fs::write(&historical, b"old archive").await.unwrap();
        assert!(archive_at(&source, dir.path(), now).await.is_err());
        assert_eq!(tokio::fs::read(&historical).await.unwrap(), b"old archive");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[tokio::test]
    async fn archive_collision_preserves_old_archive_and_current_database() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("live.sqlite");
        let now = time::OffsetDateTime::UNIX_EPOCH;
        let expected = dir
            .path()
            .join(format!("write-audit.{}.sqlite", archive_timestamp(now)));
        tokio::fs::write(&source, b"new database").await.unwrap();
        tokio::fs::write(&expected, b"old archive").await.unwrap();
        let actual = archive_at(&source, dir.path(), now).await.unwrap();
        assert_eq!(tokio::fs::read(&expected).await.unwrap(), b"old archive");
        assert_ne!(actual, expected);
        assert_eq!(tokio::fs::read(&actual).await.unwrap(), b"new database");
        assert!(!source.exists());
    }
}
