//! Recovery remains visible until completion and archive publication never clobbers.
#![allow(clippy::unwrap_used)]
use second_brain_rs::vault::audit::{
    AuditInput, VaultWriteAuditStore, rotate_write_audit_if_needed,
};
use serde_json::json;
fn input() -> AuditInput {
    AuditInput {
        operation: "create_note".into(),
        path: "pending.md".into(),
        base_sha256: None,
        metadata: json!({}),
    }
}
#[tokio::test]
async fn retention_defers_while_pending_attempts_need_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite");
    let archive = dir.path().join("archive");
    let store = VaultWriteAuditStore::open(&path).unwrap();
    store.record_write(&input(), "one").unwrap();
    store.record_write(&input(), "two").unwrap();
    let pending = store.record_write_started(&input()).unwrap();
    drop(store);
    assert!(
        rotate_write_audit_if_needed(&path, &archive, 1)
            .await
            .unwrap()
            .is_none()
    );
    let store = VaultWriteAuditStore::open(&path).unwrap();
    assert_eq!(store.list_incomplete_writes(None).unwrap().len(), 1);
    assert_eq!(store.list_recent_writes(None).unwrap().len(), 2);
    store
        .record_write_failed(&pending, "operator reconciled interrupted attempt")
        .unwrap();
    drop(store);
    assert!(
        rotate_write_audit_if_needed(&path, &archive, 1)
            .await
            .unwrap()
            .is_some()
    );
}
#[test]
fn completion_failure_keeps_attempt_in_recovery_and_rolls_back_success() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.sqlite");
    let store = VaultWriteAuditStore::open(&path).unwrap();
    let pending = store.record_write_started(&input()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_provenance BEFORE INSERT ON write_audit BEGIN SELECT RAISE(ABORT, 'simulated disk full'); END;").unwrap();
    assert!(
        store
            .record_write_completed(&input(), Some(&pending), "hash")
            .is_err()
    );
    assert_eq!(store.list_incomplete_writes(None).unwrap().len(), 1);
    assert!(store.list_recent_writes(None).unwrap().is_empty());
    let terminals: i64 = db
        .query_row(
            "SELECT count(*) FROM write_audit_attempts WHERE event_type='succeeded'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(terminals, 0);
    db.execute_batch("DROP TRIGGER fail_provenance;").unwrap();
    store
        .record_write_completed(&input(), Some(&pending), "hash")
        .unwrap();
    assert!(store.list_incomplete_writes(None).unwrap().is_empty());
    assert_eq!(store.list_recent_writes(None).unwrap().len(), 1);
}
#[test]
fn completion_without_start_still_records_best_effort_provenance() {
    let store = VaultWriteAuditStore::open(std::path::Path::new(":memory:")).unwrap();
    store
        .record_write_completed(&input(), None, "hash")
        .unwrap();
    assert_eq!(store.list_recent_writes(None).unwrap().len(), 1);
    assert!(store.list_incomplete_writes(None).unwrap().is_empty());
}

#[test]
fn terminal_insert_failure_does_not_create_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.sqlite");
    let store = VaultWriteAuditStore::open(&path).unwrap();
    let pending = store.record_write_started(&input()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_terminal BEFORE INSERT ON write_audit_attempts WHEN NEW.event_type='succeeded' BEGIN SELECT RAISE(ABORT, 'simulated disk full'); END;").unwrap();
    assert!(
        store
            .record_write_completed(&input(), Some(&pending), "hash")
            .is_err()
    );
    assert_eq!(store.list_incomplete_writes(None).unwrap().len(), 1);
    assert!(store.list_recent_writes(None).unwrap().is_empty());
}

#[tokio::test]
async fn writer_preserves_completed_note_and_pending_recovery_on_audit_failure() {
    use std::{collections::HashSet, sync::Arc};

    use second_brain_rs::vault::{
        policy::PathPolicy,
        writer::{VaultWriter, VaultWriterOptions},
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.sqlite");
    let audit = Arc::new(VaultWriteAuditStore::open(&path).unwrap());
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_provenance BEFORE INSERT ON write_audit BEGIN SELECT RAISE(ABORT, 'simulated disk full'); END;").unwrap();
    let writer = VaultWriter::new(
        VaultWriterOptions {
            vault_root: dir.path().into(),
            cooldown_seconds: 0,
            blocked_paths: PathPolicy::default(),
            quarantined_paths: HashSet::new(),
            trash_path: ".trash/mcp".into(),
        },
        Some(Arc::clone(&audit)),
    );
    let result = writer
        .create_note("completed.md", "complete", None)
        .await
        .unwrap();
    assert_eq!(result.path, "completed.md");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("completed.md")).unwrap(),
        "complete"
    );
    assert!(audit.list_recent_writes(None).unwrap().is_empty());
    assert_eq!(audit.list_incomplete_writes(None).unwrap().len(), 1);
}
