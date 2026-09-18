//! Regression coverage for storage review findings.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use second_brain_rs::vault::path::{VaultPathError, normalize_vault_path};

#[test]
fn rejects_windows_drive_prefixes_on_every_platform() {
    for path in [
        r"C:\vault\note.md",
        "C:/vault/note.md",
        "C:note.md",
        "c:",
        "./C:note.md",
        r".\C:\note.md",
    ] {
        assert!(
            matches!(normalize_vault_path(path), Err(VaultPathError::NotRelative)),
            "accepted {path:?}"
        );
    }
    assert_eq!(
        normalize_vault_path("Notes/name:detail.md").unwrap(),
        "Notes/name:detail.md"
    );
    assert_eq!(
        normalize_vault_path("./Notes/ok.md").unwrap(),
        "Notes/ok.md"
    );
    assert_eq!(normalize_vault_path("1:note.md").unwrap(), "1:note.md");
}

#[cfg(unix)]
#[tokio::test]
async fn replacement_mutations_preserve_unix_permissions() {
    use std::{
        collections::{BTreeMap, HashSet},
        os::unix::fs::PermissionsExt,
    };

    use second_brain_rs::vault::{
        markdown::FrontmatterValue,
        policy::PathPolicy,
        writer::{VaultWriter, VaultWriterOptions},
    };

    let dir = tempfile::tempdir().unwrap();
    let writer = VaultWriter::new(
        VaultWriterOptions {
            vault_root: dir.path().to_path_buf(),
            cooldown_seconds: 0,
            blocked_paths: PathPolicy::default(),
            quarantined_paths: HashSet::new(),
            trash_path: ".trash/mcp".into(),
        },
        None,
    );
    for mode in [0o600, 0o640] {
        let path = format!("note-{mode}.md");
        let body = "<!-- mcp:section x start -->\nold\n<!-- mcp:section x end -->";
        let created = writer.create_note(&path, body, None).await.unwrap();
        let absolute = dir.path().join(&path);
        tokio::fs::set_permissions(&absolute, std::fs::Permissions::from_mode(mode))
            .await
            .unwrap();
        let replaced = writer
            .replace_note(&path, body, &created.result_sha256, None)
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::metadata(&absolute)
                .await
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            mode
        );
        let updated = writer
            .update_frontmatter(
                &path,
                &BTreeMap::from([("done".into(), FrontmatterValue::Bool(true))]),
                &replaced.result_sha256,
            )
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::metadata(&absolute)
                .await
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            mode
        );
        writer
            .replace_section_by_marker(&path, "x", "changed", &updated.result_sha256)
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::metadata(&absolute)
                .await
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            mode
        );
    }
}
