//! Runtime startup retention and validated mutation boundaries.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use std::sync::Arc;

use second_brain_rs::{
    config::{ServerConfig, parse_config},
    runtime::Runtime,
    vault::audit::{AuditInput, VaultWriteAuditStore},
};
use serde_json::json;

fn config(dir: &std::path::Path) -> Arc<ServerConfig> {
    Arc::new(
        parse_config(&format!(
            r#"
listen = "127.0.0.1:0"
public_base_url = "http://127.0.0.1:3000"
vault_path = "{}/vault"
state_path = "{}/state"
[auth]
mode = "development"
audience = "test"
trusted_issuers = ["https://issuer.example"]
discovery_authorization_server = "https://issuer.example"
jwks_cache_ttl_seconds = 60
[index]
sqlite_path = ":memory:"
watcher_polling = false
ignored_globs = []
[writes]
cooldown_seconds = 0
[deletes]
trash_path = "Trash/custom"
[audit]
retention_max_rows = 1
[daily_note]
capture_default_pattern = "B"
[logging]
log_args = false
"#,
            dir.display(),
            dir.display()
        ))
        .unwrap(),
    )
}

#[tokio::test]
async fn startup_rotates_oversized_audit_and_new_mutations_use_configured_trash() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    tokio::fs::create_dir_all(&config.vault_path).await.unwrap();
    tokio::fs::create_dir_all(&config.state_path).await.unwrap();
    let audit_path = dir.path().join("state/write-audit.sqlite");
    let audit = VaultWriteAuditStore::open(&audit_path).unwrap();
    let input = AuditInput {
        operation: "create_note".into(),
        path: "old.md".into(),
        base_sha256: None,
        metadata: json!({}),
    };
    audit.record_write(&input, "old1").unwrap();
    audit.record_write(&input, "old2").unwrap();
    drop(audit);
    let runtime = Runtime::create(config).await.unwrap();
    let mut archives = tokio::fs::read_dir(dir.path().join("state/audit-archive"))
        .await
        .unwrap();
    let entry = archives.next_entry().await.unwrap().unwrap();
    assert!(archives.next_entry().await.unwrap().is_none());
    assert_eq!(
        VaultWriteAuditStore::open(&entry.path())
            .unwrap()
            .list_recent_writes(None)
            .unwrap()
            .len(),
        2
    );
    assert!(
        VaultWriteAuditStore::open(&audit_path)
            .unwrap()
            .list_recent_writes(None)
            .unwrap()
            .is_empty()
    );
    let created = runtime
        .dispatch(
            "create_note",
            json!({"path":"new.md","content":"new"})
                .as_object()
                .unwrap(),
        )
        .await
        .unwrap();
    let deleted = runtime
        .dispatch(
            "delete_note",
            json!({"path":"new.md","base_sha256":created["resultSha256"]})
                .as_object()
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deleted["deletedPath"], "Trash/custom/new.md");
}

#[tokio::test]
async fn malformed_mutation_inputs_do_not_create_files_or_audit_success() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    tokio::fs::create_dir_all(&config.vault_path).await.unwrap();
    let runtime = Runtime::create(config).await.unwrap();
    for (input, message) in [
        (
            json!({"path":"bad.md","content":""}),
            "content must be a non-empty string",
        ),
        (
            json!({"path":"bad.md","content":"x","frontmatter":{"key":null}}),
            "frontmatter.key must be a frontmatter value",
        ),
        (
            json!({"path":"bad.md","content":"x","frontmatter":{"key":[1]}}),
            "frontmatter.key must be a frontmatter value",
        ),
        (
            json!({"path":"bad.md","content":"x","frontmatter":[]}),
            "frontmatter must be an object",
        ),
    ] {
        assert_eq!(
            runtime
                .dispatch("create_note", input.as_object().unwrap())
                .await
                .unwrap_err()
                .to_string(),
            message
        );
    }
    assert!(!dir.path().join("vault/bad.md").exists());
    assert!(
        VaultWriteAuditStore::open(&dir.path().join("state/write-audit.sqlite"))
            .unwrap()
            .list_recent_writes(None)
            .unwrap()
            .is_empty()
    );
}
