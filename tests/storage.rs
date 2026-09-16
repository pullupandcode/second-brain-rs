//! Filesystem mutation behavior and adversarial policy checks.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use second_brain_rs::vault::{
    markdown::FrontmatterValue,
    policy::PathPolicy,
    writer::{VaultWriter, VaultWriterOptions},
};

fn writer(root: &std::path::Path, cooldown: u64) -> VaultWriter {
    VaultWriter::new(
        VaultWriterOptions {
            vault_root: root.to_path_buf(),
            cooldown_seconds: cooldown,
            blocked_paths: PathPolicy::new(vec!["Private/**".into()]),
            quarantined_paths: HashSet::from(["Conflict.md".into()]),
            trash_path: ".trash/mcp".into(),
        },
        None,
    )
}
#[tokio::test]
async fn atomic_crud_and_hash_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let w = writer(dir.path(), 0);
    let created = w.create_note("Notes/a.md", "# A", None).await.unwrap();
    assert_eq!(created.result_sha256.len(), 64);
    assert_eq!(
        w.create_note("Notes/a.md", "bad", None)
            .await
            .unwrap_err()
            .code(),
        "path_exists"
    );
    let stale = w
        .replace_note("Notes/a.md", "bad", "stale", None)
        .await
        .unwrap_err();
    assert_eq!(stale.code(), "retryable_conflict");
    assert_eq!(stale.current_sha256(), Some(created.result_sha256.as_str()));
    let updated = w
        .replace_note("Notes/a.md", "# B", &created.result_sha256, None)
        .await
        .unwrap();
    assert_eq!(updated.base_sha256, Some(created.result_sha256));
    let deleted = w
        .delete_note("Notes/a.md", &updated.result_sha256)
        .await
        .unwrap();
    assert_eq!(
        deleted.deleted_path.as_deref(),
        Some(".trash/mcp/Notes/a.md")
    );
    assert!(!dir.path().join("Notes/a.md").exists());
    w.hard_delete_note(
        deleted.deleted_path.as_deref().unwrap(),
        &updated.result_sha256,
    )
    .await
    .unwrap();
    assert!(!dir.path().join(".trash/mcp/Notes/a.md").exists());
}
#[tokio::test]
async fn guards_and_cooldown() {
    let dir = tempfile::tempdir().unwrap();
    let w = writer(dir.path(), 60);
    for (path, code) in [
        ("Private/a.md", "path_blocked"),
        ("Conflict.md", "path_quarantined"),
    ] {
        assert_eq!(
            w.create_note(path, "x", None).await.unwrap_err().code(),
            code
        );
    }
    assert!(w.create_note("../escape.md", "x", None).await.is_err());
    let c = w.create_note("a.md", "x", None).await.unwrap();
    assert_eq!(
        w.replace_note("a.md", "y", &c.result_sha256, None)
            .await
            .unwrap_err()
            .code(),
        "retryable_conflict"
    );
    assert_eq!(
        w.delete_note("missing.md", "none")
            .await
            .unwrap_err()
            .code(),
        "path_missing"
    );
}
#[tokio::test]
async fn frontmatter_markers_and_per_path_serialization() {
    let dir = tempfile::tempdir().unwrap();
    let w = Arc::new(writer(dir.path(), 0));
    let fm = BTreeMap::from([("title".into(), FrontmatterValue::String("Hello".into()))]);
    let c = w
        .create_note(
            "a.md",
            "before\n<!-- mcp:section x start -->\nold\n<!-- mcp:section x end -->\nafter",
            Some(&fm),
        )
        .await
        .unwrap();
    let a = w
        .replace_section_by_marker("a.md", "x", "new", &c.result_sha256)
        .await
        .unwrap();
    let patch = BTreeMap::from([("done".into(), FrontmatterValue::Bool(true))]);
    let b = w
        .update_frontmatter("a.md", &patch, &a.result_sha256)
        .await
        .unwrap();
    let text = tokio::fs::read_to_string(dir.path().join("a.md"))
        .await
        .unwrap();
    assert!(text.contains("new\n<!--"));
    assert!(text.contains("done: true"));
    assert!(text.contains("title: \"Hello\""));
    let (one, two) = tokio::join!(
        w.replace_note("a.md", "one", &b.result_sha256, None),
        w.replace_note("./a.md", "two", &b.result_sha256, None)
    );
    assert_ne!(one.is_ok(), two.is_ok());
}

#[tokio::test]
async fn changed_policy_symlinks_and_trash_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let policy = PathPolicy::new(vec![]);
    let w = VaultWriter::new(
        VaultWriterOptions {
            vault_root: dir.path().into(),
            cooldown_seconds: 0,
            blocked_paths: policy.clone(),
            quarantined_paths: HashSet::new(),
            trash_path: ".trash/mcp".into(),
        },
        None,
    );
    let first = w.create_note("a.md", "one", None).await.unwrap();
    policy.replace(vec!["a.md".into()]);
    assert_eq!(
        w.replace_note("a.md", "no", &first.result_sha256, None)
            .await
            .unwrap_err()
            .code(),
        "path_blocked"
    );
    policy.replace(vec![]);
    let deleted = w.delete_note("a.md", &first.result_sha256).await.unwrap();
    let second = w.create_note("a.md", "two", None).await.unwrap();
    let deleted2 = w.delete_note("a.md", &second.result_sha256).await.unwrap();
    assert_ne!(deleted.deleted_path, deleted2.deleted_path);
    assert_eq!(
        tokio::fs::read_to_string(dir.path().join(deleted.deleted_path.unwrap()))
            .await
            .unwrap(),
        "one"
    );
    assert_eq!(
        tokio::fs::read_to_string(dir.path().join(deleted2.deleted_path.unwrap()))
            .await
            .unwrap(),
        "two"
    );
    #[cfg(unix)]
    {
        tokio::fs::symlink(outside.path(), dir.path().join("escape"))
            .await
            .unwrap();
        assert!(w.create_note("escape/a.md", "bad", None).await.is_err());
        assert!(!outside.path().join("a.md").exists());
        tokio::fs::symlink(dir.path().join(".trash"), dir.path().join("alias"))
            .await
            .unwrap();
        assert!(w.create_note("alias/new.md", "bad", None).await.is_err());
    }
}

#[tokio::test]
async fn markers_failure_and_empty_content_leave_original_intact() {
    let dir = tempfile::tempdir().unwrap();
    let w = writer(dir.path(), 0);
    for (i, text) in [
        "no markers",
        "<!-- mcp:section x start -->",
        "<!-- mcp:section x end -->\n<!-- mcp:section x start -->",
    ]
    .iter()
    .enumerate()
    {
        let path = format!("{i}.md");
        let c = w.create_note(&path, text, None).await.unwrap();
        assert_eq!(
            w.replace_section_by_marker(&path, "x", "bad", &c.result_sha256)
                .await
                .unwrap_err()
                .code(),
            "markers_missing"
        );
        assert_eq!(
            tokio::fs::read_to_string(dir.path().join(&path))
                .await
                .unwrap(),
            *text
        );
    }
    let empty = w.create_note("empty.md", "", None).await.unwrap();
    w.hard_delete_note("empty.md", &empty.result_sha256)
        .await
        .unwrap();
}

#[tokio::test]
async fn audit_lifecycle_provenance_and_append_only_guards() {
    use second_brain_rs::vault::audit::{AuditInput, VaultWriteAuditStore};
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("audit.sqlite");
    let audit = Arc::new(VaultWriteAuditStore::open(&db_path).unwrap());
    let w = VaultWriter::new(
        VaultWriterOptions {
            vault_root: dir.path().into(),
            cooldown_seconds: 0,
            blocked_paths: PathPolicy::default(),
            quarantined_paths: HashSet::new(),
            trash_path: ".trash/mcp".into(),
        },
        Some(Arc::clone(&audit)),
    );
    let created = w
        .create_note("a.md", "SECRET NEVER IN AUDIT", None)
        .await
        .unwrap();
    w.replace_note("a.md", "second", &created.result_sha256, None)
        .await
        .unwrap();
    assert!(w.create_note("a.md", "failed", None).await.is_err());
    let rows = audit.list_recent_writes(None).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.first().unwrap().get("operation").unwrap(),
        "replace_note"
    );
    assert!(!serde_json::to_string(&rows).unwrap().contains("SECRET"));
    assert_eq!(audit.list_recent_writes(Some(-1)).unwrap().len(), 1);
    assert!(audit.list_incomplete_writes(None).unwrap().is_empty());
    let input = AuditInput {
        operation: "replace_note".into(),
        path: "crashed.md".into(),
        base_sha256: Some("old".into()),
        metadata: serde_json::json!({"markerName":"section"}),
    };
    let id = audit.record_write_started(&input).unwrap();
    assert_eq!(uuid::Uuid::parse_str(&id).unwrap().get_version_num(), 4);
    let incomplete = audit.list_incomplete_writes(None).unwrap();
    assert_eq!(incomplete.len(), 1);
    assert_eq!(incomplete.first().unwrap().get("attemptId").unwrap(), &id);
    drop(w);
    drop(audit);
    let db = rusqlite::Connection::open(&db_path).unwrap();
    for table in ["write_audit", "write_audit_attempts"] {
        for sql in [
            format!("UPDATE {table} SET path='tampered'"),
            format!("DELETE FROM {table}"),
        ] {
            assert!(
                db.execute_batch(&sql)
                    .unwrap_err()
                    .to_string()
                    .contains("append-only")
            );
        }
    }
    assert!(
        db.execute_batch("INSERT OR REPLACE INTO write_audit SELECT * FROM write_audit WHERE id=1")
            .is_err()
    );
    assert!(db.execute_batch("INSERT OR REPLACE INTO write_audit_attempts SELECT * FROM write_audit_attempts WHERE id=1").is_err());
    drop(db);
    let reopened = VaultWriteAuditStore::open(&db_path).unwrap();
    assert_eq!(reopened.list_incomplete_writes(None).unwrap().len(), 1);
    reopened.record_write_failed(&id, "recovered").unwrap();
    assert!(reopened.list_incomplete_writes(None).unwrap().is_empty());
}

#[tokio::test]
async fn audit_rotation_threshold_archive_name_and_new_store() {
    use second_brain_rs::vault::audit::{
        AuditInput, VaultWriteAuditStore, rotate_write_audit_if_needed,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.sqlite");
    let archive = dir.path().join("archive");
    tokio::fs::write(&path, "").await.unwrap();
    assert!(
        rotate_write_audit_if_needed(&path, &archive, 1)
            .await
            .unwrap()
            .is_none()
    );
    let store = VaultWriteAuditStore::open(&path).unwrap();
    let input = AuditInput {
        operation: "create_note".into(),
        path: "a.md".into(),
        base_sha256: None,
        metadata: serde_json::json!({}),
    };
    store.record_write(&input, "hash1").unwrap();
    drop(store);
    assert!(
        rotate_write_audit_if_needed(&path, &archive, 1)
            .await
            .unwrap()
            .is_none()
    );
    let store = VaultWriteAuditStore::open(&path).unwrap();
    store.record_write(&input, "hash2").unwrap();
    drop(store);
    assert!(
        rotate_write_audit_if_needed(&path, &archive, 0)
            .await
            .unwrap()
            .is_none()
    );
    let archived = rotate_write_audit_if_needed(&path, &archive, 1)
        .await
        .unwrap()
        .unwrap();
    assert!(
        regex::Regex::new(r"^write-audit\.\d{8}T\d{9}Z\.sqlite$")
            .unwrap()
            .is_match(archived.file_name().unwrap().to_str().unwrap())
    );
    assert!(!path.exists());
    assert_eq!(
        VaultWriteAuditStore::open(&archived)
            .unwrap()
            .list_recent_writes(None)
            .unwrap()
            .len(),
        2
    );
    assert!(
        VaultWriteAuditStore::open(&path)
            .unwrap()
            .list_recent_writes(None)
            .unwrap()
            .is_empty()
    );
}

proptest::proptest! {
    #[test]
    fn frontmatter_simple_values_roundtrip(text in "[a-zA-Z0-9 _-]{0,80}", flag in proptest::bool::ANY, number in -10000i32..10000) {
        use second_brain_rs::vault::{writer::with_frontmatter,markdown::parse_markdown};
        let map=BTreeMap::from([("text".into(),FrontmatterValue::String(text)),("flag".into(),FrontmatterValue::Bool(flag)),("number".into(),FrontmatterValue::Number(f64::from(number)))]);
        let serialized=with_frontmatter("body",Some(&map)).unwrap();
        let parsed=parse_markdown(&serialized);
        proptest::prop_assert_eq!(parsed.frontmatter,map);
        proptest::prop_assert_eq!(parsed.body,"body");
    }
}

#[tokio::test]
async fn audit_failure_does_not_turn_completed_write_into_retry() {
    use second_brain_rs::vault::audit::VaultWriteAuditStore;
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("audit.sqlite");
    let audit = Arc::new(VaultWriteAuditStore::open(&db_path).unwrap());
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TRIGGER simulate_audit_failure BEFORE INSERT ON write_audit BEGIN SELECT RAISE(ABORT,'disk full'); END; CREATE TRIGGER simulate_attempt_failure BEFORE INSERT ON write_audit_attempts BEGIN SELECT RAISE(ABORT,'disk full'); END;").unwrap();
    let w = VaultWriter::new(
        VaultWriterOptions {
            vault_root: dir.path().into(),
            cooldown_seconds: 0,
            blocked_paths: PathPolicy::default(),
            quarantined_paths: HashSet::new(),
            trash_path: ".trash/mcp".into(),
        },
        Some(audit),
    );
    let result = w.create_note("safe.md", "complete", None).await.unwrap();
    assert_eq!(result.path, "safe.md");
    assert_eq!(
        tokio::fs::read_to_string(dir.path().join("safe.md"))
            .await
            .unwrap(),
        "complete"
    );
}

#[tokio::test]
async fn concurrent_creates_from_separate_writers_cannot_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let one = writer(dir.path(), 0);
    let two = writer(dir.path(), 0);
    let (first, second) = tokio::join!(
        one.create_note("a.md", "first", None),
        two.create_note("a.md", "second", None)
    );
    assert_ne!(first.is_ok(), second.is_ok());
    let expected = if first.is_ok() { "first" } else { "second" };
    assert_eq!(
        tokio::fs::read_to_string(dir.path().join("a.md"))
            .await
            .unwrap(),
        expected
    );
    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    let mut count = 0;
    while entries.next_entry().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 1);
}

#[test]
fn frontmatter_escaped_string_and_list_roundtrip() {
    use second_brain_rs::vault::{markdown::parse_markdown, writer::with_frontmatter};
    let values = BTreeMap::from([
        (
            "text".into(),
            FrontmatterValue::String("line one\nline \\\" two".into()),
        ),
        (
            "list".into(),
            FrontmatterValue::List(vec!["comma, inside".into(), "a\\b".into(), "a\"b".into()]),
        ),
    ]);
    assert_eq!(
        parse_markdown(&with_frontmatter("body", Some(&values)).unwrap()).frontmatter,
        values
    );
    let invalid = BTreeMap::from([("bad:key".into(), FrontmatterValue::Bool(true))]);
    assert_eq!(
        with_frontmatter("body", Some(&invalid))
            .unwrap_err()
            .to_string(),
        "Invalid frontmatter key: bad:key"
    );
}

#[tokio::test]
async fn canonical_case_aliases_cannot_bypass_blocking_or_quarantine() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    tokio::fs::create_dir(root.join("Private")).await.unwrap();
    tokio::fs::write(root.join("Private/Secret.md"), "original")
        .await
        .unwrap();
    tokio::fs::write(root.join("Conflict.md"), "original")
        .await
        .unwrap();
    let w = writer(root, 0);
    if tokio::fs::try_exists(root.join("private")).await.unwrap() {
        assert_eq!(
            w.create_note("private/injected.md", "bad", None)
                .await
                .unwrap_err()
                .code(),
            "path_blocked"
        );
        assert_eq!(
            w.replace_note("private/secret.md", "bad", "anything", None)
                .await
                .unwrap_err()
                .code(),
            "path_blocked"
        );
        assert_eq!(
            w.replace_note("conflict.md", "bad", "anything", None)
                .await
                .unwrap_err()
                .code(),
            "path_quarantined"
        );
        assert!(!root.join("Private/injected.md").exists());
    } else {
        // Hard-deny patterns deliberately ignore case on every filesystem.
        assert_eq!(
            w.create_note("private/allowed.md", "good", None)
                .await
                .unwrap_err()
                .code(),
            "path_blocked"
        );
    }
}

#[tokio::test]
async fn canonical_case_aliases_share_optimistic_concurrency_lock() {
    let dir = tempfile::tempdir().unwrap();
    let w = writer(dir.path(), 0);
    let created = w.create_note("Case.md", "original", None).await.unwrap();
    if tokio::fs::try_exists(dir.path().join("case.md"))
        .await
        .unwrap()
    {
        let (a, b) = tokio::join!(
            w.replace_note("Case.md", "first", &created.result_sha256, None),
            w.replace_note("case.md", "second", &created.result_sha256, None)
        );
        assert_ne!(
            a.is_ok(),
            b.is_ok(),
            "aliases must not both accept the same base hash"
        );
        let rejected = a.err().or_else(|| b.err()).unwrap();
        assert_eq!(rejected.code(), "retryable_conflict");
    } else {
        let distinct = w.create_note("case.md", "different", None).await.unwrap();
        let (a, b) = tokio::join!(
            w.replace_note("Case.md", "first", &created.result_sha256, None),
            w.replace_note("case.md", "second", &distinct.result_sha256, None)
        );
        assert!(a.is_ok() && b.is_ok());
    }
}
