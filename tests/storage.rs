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
