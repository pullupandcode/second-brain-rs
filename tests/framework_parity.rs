//! Framework workflows exercised through the production runtime.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use std::sync::Arc;

use second_brain_rs::{config::parse_config, runtime::Runtime};
use serde_json::{Value, json};

async fn fixture() -> (tempfile::TempDir, Arc<Runtime>) {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    tokio::fs::create_dir_all(vault.join("x/Templates"))
        .await
        .unwrap();
    tokio::fs::write(vault.join("x/Templates/Daily Template.md"), "# Day\n<!-- mcp:section daily-captures start -->\n<!-- mcp:section daily-captures end -->\n<!-- mcp:section last-light-summary start -->\n<!-- mcp:section last-light-summary end -->\n").await.unwrap();
    let config = format!(
        r#"listen = "127.0.0.1:0"
public_base_url = "http://127.0.0.1:3000"
vault_path = "{}"
state_path = "{}"
[auth]
mode = "development"
audience = "test"
trusted_issuers = ["https://idp.example.com/"]
discovery_authorization_server = "https://idp.example.com/"
jwks_cache_ttl_seconds = 60
[index]
sqlite_path = ":memory:"
watcher_polling = false
ignored_globs = []
[writes]
cooldown_seconds = 0
[daily_note]
capture_default_pattern = "B"
[logging]
log_args = false
[security]
blocked_paths = ["Private/**"]
"#,
        vault.display(),
        dir.path().join("state").display()
    );
    let runtime = Runtime::create(Arc::new(parse_config(&config).unwrap()))
        .await
        .unwrap();
    (dir, runtime)
}

async fn call(runtime: &Runtime, name: &str, args: Value) -> Value {
    runtime
        .dispatch(name, args.as_object().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn initializes_presets_exclusively_and_overwrites() {
    let (_dir, runtime) = fixture().await;
    assert_eq!(
        call(&runtime, "framework_init", json!({"framework":"lyt"})).await,
        json!({"path":"_meta/framework.yaml","framework":"lyt","created":true,"overwritten":false})
    );
    assert!(
        runtime
            .dispatch(
                "framework_init",
                json!({"framework":"para"}).as_object().unwrap()
            )
            .await
            .is_err()
    );
    let result = call(
        &runtime,
        "framework_init",
        json!({"framework":"para","mode":"overwrite"}),
    )
    .await;
    assert_eq!(result["overwritten"], true);
    assert_eq!(
        call(&runtime, "list_record_types", json!({})).await["recordTypes"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        call(&runtime, "get_vault_structure", json!({})).await["recordTypes"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[tokio::test]
async fn overlays_persist_sort_replace_reload_and_compose_without_restart() {
    let (dir, runtime) = fixture().await;
    call(&runtime, "framework_init", json!({"framework":"para"})).await;
    tokio::fs::write(
        dir.path().join("vault/_meta/work.yaml"),
        "version: 1\nschema_kind: overlay\ntypes:\n  meeting:\n    folder: Meetings\n",
    )
    .await
    .unwrap();
    call(
        &runtime,
        "framework_register",
        json!({"name":"work","path":"_meta/work.yaml","priority":50}),
    )
    .await;
    call(
        &runtime,
        "framework_register",
        json!({"name":"missing","path":"_meta/missing.yaml"}),
    )
    .await;
    let reload = call(&runtime, "framework_reload", json!({})).await;
    assert_eq!(reload["ok"], false);
    assert_eq!(reload["overlays"][0]["status"], "loaded");
    assert_eq!(reload["overlays"][1]["status"], "error");
    assert_eq!(
        call(&runtime, "framework_unregister", json!({"name":"missing"})).await,
        json!({"removed":true})
    );
    assert_eq!(
        call(&runtime, "framework_unregister", json!({"name":"missing"})).await,
        json!({"removed":false})
    );
    call(
        &runtime,
        "framework_register",
        json!({"name":"work","path":"_meta/work.yaml","priority":2}),
    )
    .await;
    let listed = call(&runtime, "framework_list", json!({})).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["priority"], 2);
    assert_eq!(
        call(&runtime, "framework_compose", json!({})).await["types"]["meeting"]["folder"],
        "Meetings"
    );
    let registry: Value = serde_json::from_str(
        &tokio::fs::read_to_string(dir.path().join("vault/_meta/schemas.json"))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(registry["overlays"][0]["name"], "work");
}

#[tokio::test]
async fn creates_records_with_utc_tokens_template_and_scheduled_attendees() {
    let (dir, runtime) = fixture().await;
    tokio::fs::create_dir_all(dir.path().join("vault/_meta"))
        .await
        .unwrap();
    tokio::fs::write(dir.path().join("vault/_meta/framework.yaml"),"version: 1\nschema_kind: base\nframework: custom\ntypes:\n  meeting:\n    folder: Meetings\n    filename: \"{date:YYYY} Q{quarter}/{date:YYYY-MM}/{date:YYYY-MM-DD HH-mm} {title}.md\"\n    template: x/Templates/Meeting.md\n    frontmatter:\n      scheduled: { format: \"YYYY-MM-DD hh:mm a\" }\n").await.unwrap();
    tokio::fs::write(
        dir.path().join("vault/x/Templates/Meeting.md"),
        "# Meeting\n\nAgenda:\n",
    )
    .await
    .unwrap();
    let result = call(&runtime,"create_record",json!({"type":"meeting","title":"Planning/test","date":"2026-05-07T15:30:00Z","body":"Decisions","fields":{"attendees":[" Ada ","[[Grace]]"]}})).await;
    assert_eq!(
        result["path"],
        "Meetings/2026 Q2/2026-05/2026-05-07 15-30 Planning-test.md"
    );
    let note = call(&runtime, "read_note", json!({"path":result["path"]})).await;
    assert_eq!(
        note["parsed"]["frontmatter"]["scheduled"],
        "2026-05-07 03:30 PM"
    );
    assert_eq!(
        note["parsed"]["frontmatter"]["attendees"],
        json!(["[[Ada]]", "[[Grace]]"])
    );
    assert!(
        note["content"]
            .as_str()
            .unwrap()
            .ends_with("Agenda:\n\nDecisions")
    );
    assert!(
        runtime
            .dispatch(
                "create_record",
                json!({"type":"meeting","title":"Bad","date":"bad"})
                    .as_object()
                    .unwrap()
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn capture_replaces_same_source_and_keeps_daily_untouched() {
    let (dir, runtime) = fixture().await;
    call(&runtime, "framework_init", json!({"framework":"lyt"})).await;
    let first = call(&runtime,"inbox_capture",json!({"content":"first","date":"2026-05-07","source_client":"codex","source_id":"rmpage:book:page","title":"Original","strategy":"replace_by_source_id"})).await;
    let second = call(&runtime,"inbox_capture",json!({"content":"second","date":"2026-05-08","source_client":"codex","source_id":"rmpage:book:page","title":"Changed","strategy":"replace_by_source_id"})).await;
    assert_eq!(first["path"], second["path"]);
    assert_eq!(first["resultSha256"], second["baseSha256"]);
    assert_eq!(
        call(
            &runtime,
            "link_to_page",
            json!({"notebook":"rmnotebook:book","page_uuid":"page"})
        )
        .await["link"],
        format!("[[{}|page]]", first["path"].as_str().unwrap())
    );
    assert!(
        !dir.path()
            .join("vault/Calendar/Days/2026-05-07.md")
            .exists()
    );
    assert!(
        runtime
            .dispatch(
                "inbox_capture",
                json!({"content":"x","source_client":"c","strategy":"replace_by_source_id"})
                    .as_object()
                    .unwrap()
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn daily_get_append_conflict_protected_sections_and_repair() {
    let (dir, runtime) = fixture().await;
    let daily = call(&runtime, "daily_note_get", json!({"date":"2026-05-07"})).await;
    let args =
        json!({"date":"2026-05-07","content":"- captured","base_sha256":daily["currentSha256"]});
    let appended = call(&runtime, "daily_note_append", args.clone()).await;
    assert!(
        appended["content"]
            .as_str()
            .unwrap()
            .contains("start -->\n- captured\n<!-- mcp:section daily-captures end")
    );
    assert!(
        runtime
            .dispatch("daily_note_append", args.as_object().unwrap())
            .await
            .is_err()
    );
    for section in ["agenda", "missing"] {
        assert!(runtime.dispatch("daily_note_append",json!({"date":"2026-05-07","section":section,"content":"x","base_sha256":appended["currentSha256"]}).as_object().unwrap()).await.is_err());
    }
    tokio::fs::write(
        dir.path().join("vault/Calendar/Days/2026-05-07.md"),
        "# Day\nhuman text\n",
    )
    .await
    .unwrap();
    let note = call(&runtime, "daily_note_get", json!({"date":"2026-05-07"})).await;
    let repaired = call(
        &runtime,
        "daily_note_repair_markers",
        json!({"date":"2026-05-07","base_sha256":note["currentSha256"]}),
    )
    .await;
    assert!(
        repaired["content"]
            .as_str()
            .unwrap()
            .contains("human text\n")
    );
    assert!(
        repaired["content"]
            .as_str()
            .unwrap()
            .contains("last-light-summary start")
    );
}

#[tokio::test]
async fn framework_paths_obey_privacy_and_traversal_guards() {
    let (_dir, runtime) = fixture().await;
    for path in ["Private/framework.yaml", "../outside.yaml"] {
        assert!(
            runtime
                .dispatch(
                    "framework_init",
                    json!({"framework":"lyt","output_path":path})
                        .as_object()
                        .unwrap()
                )
                .await
                .is_err()
        );
        assert!(
            runtime
                .dispatch(
                    "framework_register",
                    json!({"name":"bad","path":path}).as_object().unwrap()
                )
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn sample_schema_is_parseable_and_map_search_uses_schema_folders() {
    let (dir, runtime) = fixture().await;
    tokio::fs::create_dir_all(dir.path().join("vault/_meta"))
        .await
        .unwrap();
    tokio::fs::write(
        dir.path().join("vault/_meta/framework.yaml"),
        include_str!("../examples/vault/_meta/framework.lyt.yaml"),
    )
    .await
    .unwrap();
    let schema = call(&runtime, "framework_compose", json!({})).await;
    assert_eq!(schema["name"], "lyt-starter");
    assert_eq!(schema["inbox"]["folder"], "+");
    assert_eq!(
        schema["types"]["meeting"]["frontmatter"]["collection"]["defaultValue"],
        json!(["[[Meetings]]"])
    );
    call(
        &runtime,
        "create_record",
        json!({"type":"map","title":"Rust","body":"Rust systems"}),
    )
    .await;
    call(
        &runtime,
        "create_record",
        json!({"type":"person","title":"Rust person","body":"Rust systems"}),
    )
    .await;
    let maps = call(&runtime, "find_maps", json!({"topic":"Rust"})).await;
    assert_eq!(maps["maps"].as_array().unwrap().len(), 1);
    assert_eq!(maps["maps"][0]["path"], "Atlas/Maps/Rust.md");
}

#[tokio::test]
async fn missing_record_template_is_tolerated_but_private_template_is_denied() {
    let (dir, runtime) = fixture().await;
    tokio::fs::create_dir_all(dir.path().join("vault/_meta"))
        .await
        .unwrap();
    tokio::fs::write(dir.path().join("vault/_meta/framework.yaml"),"version: 1\nschema_kind: base\ntypes:\n  missing:\n    folder: Notes\n    template: absent.md\n  private:\n    folder: Notes\n    template: Private/secret.md\n").await.unwrap();
    let result = call(
        &runtime,
        "create_record",
        json!({"type":"missing","title":"Works","body":"Body"}),
    )
    .await;
    assert_eq!(result["path"], "Notes/Works.md");
    let error = runtime
        .dispatch(
            "create_record",
            json!({"type":"private","title":"Denied"})
                .as_object()
                .unwrap(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("blocked"));
    assert!(!dir.path().join("vault/Notes/Denied.md").exists());
}

#[tokio::test]
async fn invalid_framework_arguments_reject_without_mutating_files() {
    let (dir, runtime) = fixture().await;
    for args in [
        json!({"framework":"custom"}),
        json!({"framework":"lyt","mode":"wrong"}),
        json!({"framework":"lyt","output_path":null}),
    ] {
        assert!(
            runtime
                .dispatch("framework_init", args.as_object().unwrap())
                .await
                .is_err()
        );
    }
    assert!(!dir.path().join("vault/_meta/framework.yaml").exists());
    for priority in [json!(null), json!("100"), json!(1.5)] {
        assert!(
            runtime
                .dispatch(
                    "framework_register",
                    json!({"name":"x","path":"_meta/x.yaml","priority":priority})
                        .as_object()
                        .unwrap()
                )
                .await
                .is_err()
        );
    }
    assert!(!dir.path().join("vault/_meta/schemas.json").exists());
}
