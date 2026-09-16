//! Framework workflows exercised through the production runtime.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::literal_string_with_formatting_args
)]
use std::sync::Arc;

use second_brain_rs::{config::parse_config, runtime::Runtime};
use serde_json::{Value, json};

async fn fixture() -> (tempfile::TempDir, Arc<Runtime>) {
    fixture_with_cooldown(0).await
}

async fn fixture_with_cooldown(cooldown: u64) -> (tempfile::TempDir, Arc<Runtime>) {
    let (dir, runtime, _) = fixture_config(cooldown).await;
    (dir, runtime)
}

async fn fixture_config(
    cooldown: u64,
) -> (
    tempfile::TempDir,
    Arc<Runtime>,
    Arc<second_brain_rs::config::ServerConfig>,
) {
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
cooldown_seconds = {cooldown}
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
    let config = Arc::new(parse_config(&config).unwrap());
    let runtime = Runtime::create(Arc::clone(&config)).await.unwrap();
    (dir, runtime, config)
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

#[tokio::test]
async fn metadata_does_not_inherit_note_cooldown_and_registry_updates_do_not_get_lost() {
    let (_dir, runtime) = fixture_with_cooldown(60).await;
    call(&runtime, "framework_init", json!({"framework":"lyt"})).await;
    call(
        &runtime,
        "framework_init",
        json!({"framework":"para","mode":"overwrite"}),
    )
    .await;
    let mut tasks = Vec::new();
    for i in 0..12 {
        let runtime = Arc::clone(&runtime);
        tasks.push(tokio::spawn(async move {
            call(&runtime,"framework_register",json!({"name":format!("overlay-{i:02}"),"path":format!("_meta/{i}.yaml"),"priority":i%3})).await
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let entries = call(&runtime, "framework_list", json!({})).await;
    assert_eq!(entries.as_array().unwrap().len(), 12);
    assert_eq!(entries[0]["name"], "overlay-00");
    assert_eq!(entries[1]["name"], "overlay-03");
}

#[cfg(unix)]
#[tokio::test]
async fn schema_registry_and_templates_cannot_escape_via_symlinks() {
    let (dir, runtime) = fixture().await;
    let outside = tempfile::tempdir().unwrap();
    tokio::fs::symlink(outside.path(), dir.path().join("vault/escape"))
        .await
        .unwrap();
    let error = runtime
        .dispatch(
            "framework_init",
            json!({"framework":"lyt","output_path":"escape/schema.yaml"})
                .as_object()
                .unwrap(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("outside") || error.to_string().contains("symlink"));
    assert!(!outside.path().join("schema.yaml").exists());
    call(&runtime, "framework_init", json!({"framework":"lyt"})).await;
    tokio::fs::write(outside.path().join("secret.json"), "{\"overlays\":[]}")
        .await
        .unwrap();
    tokio::fs::symlink(
        outside.path().join("secret.json"),
        dir.path().join("vault/_meta/schemas.json"),
    )
    .await
    .unwrap();
    assert!(
        runtime
            .dispatch(
                "framework_register",
                json!({"name":"bad","path":"x.yaml"}).as_object().unwrap()
            )
            .await
            .is_err()
    );
    assert_eq!(
        tokio::fs::read_to_string(outside.path().join("secret.json"))
            .await
            .unwrap(),
        "{\"overlays\":[]}"
    );
}

async fn http_rpc(base: &str, scope: &str, method: &str, params: Value) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .header("authorization", format!("Bearer scope={scope}"))
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {text}");
    if let Ok(value) = serde_json::from_str(&text) {
        return value;
    }
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .find_map(|data| serde_json::from_str(data).ok())
        .unwrap()
}

async fn http_tool(base: &str, scope: &str, name: &str, args: Value) -> Value {
    let rpc = http_rpc(
        base,
        scope,
        "tools/call",
        json!({"name":name,"arguments":args}),
    )
    .await;
    assert!(rpc.get("error").is_none(), "{rpc}");
    assert_ne!(rpc["result"]["isError"], true, "{rpc}");
    rpc["result"]["structuredContent"].clone()
}

#[tokio::test]
async fn http_framework_overlay_capture_daily_and_scope_acceptance() {
    use second_brain_rs::{
        auth::dev::DevAuthenticator,
        http::{AppState, build_router},
        mcp::SecondBrainHandler,
    };
    let (dir, runtime, config) = fixture_config(0).await;
    let handler = SecondBrainHandler::new(&config, runtime);
    let app = build_router(AppState {
        config,
        authenticator: Arc::new(DevAuthenticator::new(std::collections::HashSet::new())),
        handler,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    http_tool(&base, "admin", "framework_init", json!({"framework":"lyt"})).await;
    tokio::fs::write(
        dir.path().join("vault/_meta/work.yaml"),
        "version: 1\nschema_kind: overlay\ntypes:\n  decision:\n    folder: Decisions\n",
    )
    .await
    .unwrap();
    http_tool(
        &base,
        "admin",
        "framework_register",
        json!({"name":"work","path":"_meta/work.yaml"}),
    )
    .await;
    assert_eq!(
        http_tool(&base, "admin", "framework_reload", json!({})).await["ok"],
        true
    );
    assert_eq!(
        http_tool(&base, "admin", "framework_compose", json!({})).await["types"]["decision"]["folder"],
        "Decisions"
    );
    assert_eq!(
        http_tool(&base, "vault:read", "list_record_types", json!({})).await["recordTypes"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    let capture = json!({"content":"original","date":"2026-05-07","source_client":"http","source_id":"same","strategy":"replace_by_source_id","title":"HTTP"});
    let first = http_tool(&base, "vault:capture", "inbox_capture", capture).await;
    let second=http_tool(&base,"vault:capture","inbox_capture",json!({"content":"replacement","date":"2026-05-08","source_client":"http","source_id":"same","strategy":"replace_by_source_id"})).await;
    assert_eq!(first["path"], second["path"]);
    assert_eq!(first["resultSha256"], second["baseSha256"]);
    let daily = http_tool(
        &base,
        "vault:read",
        "daily_note_get",
        json!({"date":"2026-05-07"}),
    )
    .await;
    let appended=http_tool(&base,"daily:append","daily_note_append",json!({"date":"2026-05-07","content":"- HTTP capture","base_sha256":daily["currentSha256"]})).await;
    assert!(
        appended["content"]
            .as_str()
            .unwrap()
            .contains("- HTTP capture")
    );
    let denied = http_rpc(
        &base,
        "vault:read",
        "tools/call",
        json!({"name":"create_record","arguments":{"type":"decision","title":"Denied"}}),
    )
    .await;
    assert!(denied.get("error").is_some(), "{denied}");
    let denied = http_rpc(
        &base,
        "vault:capture",
        "tools/call",
        json!({"name":"framework_init","arguments":{"framework":"para"}}),
    )
    .await;
    assert!(denied.get("error").is_some(), "{denied}");
    let created = http_tool(
        &base,
        "vault:write",
        "create_record",
        json!({"type":"decision","title":"HTTP decision","body":"Approved"}),
    )
    .await;
    assert_eq!(created["path"], "Decisions/HTTP decision.md");
    server.abort();
}

#[tokio::test]
async fn markdown_framework_output_uses_audited_note_guards() {
    let (dir, runtime) = fixture_with_cooldown(60).await;
    call(
        &runtime,
        "framework_init",
        json!({"framework":"lyt","output_path":"Notes/schema.md"}),
    )
    .await;
    let audit_path = dir.path().join("state/write-audit.sqlite");
    let writes = tokio::task::spawn_blocking(move || {
        second_brain_rs::vault::audit::VaultWriteAuditStore::open(&audit_path)
            .unwrap()
            .list_recent_writes(None)
            .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(writes.len(), 1);
    let error = runtime
        .dispatch(
            "framework_init",
            json!({"framework":"para","output_path":"Notes/schema.md","mode":"overwrite"})
                .as_object()
                .unwrap(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cooldown"));
    assert!(
        tokio::fs::read_to_string(dir.path().join("vault/Notes/schema.md"))
            .await
            .unwrap()
            .contains("framework: lyt")
    );
}

async fn fixture_with_skill_map() -> (tempfile::TempDir, Arc<Runtime>) {
    let (dir, previous, config) = fixture_config(0).await;
    drop(previous);
    tokio::fs::write(dir.path().join("vault/Skill Map.md"), "# Skills\n")
        .await
        .unwrap();
    let mut config = (*config).clone();
    config.skills.map_paths = vec!["Skill Map.md".into()];
    let runtime = Runtime::create(Arc::new(config)).await.unwrap();
    (dir, runtime)
}

#[tokio::test]
async fn skills_reload_updates_template_capture_and_daily_privacy() {
    let (dir, runtime) = fixture_with_skill_map().await;
    call(&runtime, "framework_init", json!({"framework":"lyt"})).await;
    tokio::fs::write(dir.path().join("vault/_meta/framework.yaml"),"version: 1\nschema_kind: base\ntypes:\n  capture:\n    folder: Captures\n  note:\n    folder: Notes\n    template: x/Templates/Record.md\n").await.unwrap();
    tokio::fs::write(
        dir.path().join("vault/x/Templates/Record.md"),
        "# Private template\n",
    )
    .await
    .unwrap();
    call(
        &runtime,
        "create_record",
        json!({"type":"note","title":"Before"}),
    )
    .await;
    call(&runtime,"inbox_capture",json!({"title":"Secret","content":"before","source_client":"test","source_id":"same","strategy":"replace_by_source_id"})).await;
    let daily = call(&runtime, "daily_note_get", json!({"date":"2026-05-07"})).await;
    tokio::fs::write(dir.path().join("vault/Skill Map.md"),"[[x/Templates/Record]]\n[[Captures/Secret]]\n[[Calendar/Days/2026-05-07]]\n[[x/Templates/Daily Template]]\n").await.unwrap();
    call(&runtime, "skills_reload", json!({})).await;
    for (tool, args) in [
        ("create_record", json!({"type":"note","title":"After"})),
        (
            "inbox_capture",
            json!({"title":"Secret","content":"after","source_client":"test","source_id":"same","strategy":"replace_by_source_id"}),
        ),
        (
            "capture_for_date",
            json!({"title":"Secret","content":"after","source_client":"test"}),
        ),
        ("daily_note_get", json!({"date":"2026-05-07"})),
        ("daily_note_get", json!({"date":"2026-05-08"})),
        (
            "daily_note_append",
            json!({"date":"2026-05-07","content":"after","base_sha256":daily["currentSha256"]}),
        ),
        (
            "daily_note_repair_markers",
            json!({"date":"2026-05-07","base_sha256":daily["currentSha256"]}),
        ),
    ] {
        let error = runtime
            .dispatch(tool, args.as_object().unwrap())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("blocked"), "{tool}: {error}");
    }
    assert!(!dir.path().join("vault/Notes/After.md").exists());
    assert!(
        !dir.path()
            .join("vault/Calendar/Days/2026-05-08.md")
            .exists()
    );
    assert!(
        tokio::fs::read_to_string(dir.path().join("vault/Captures/Secret.md"))
            .await
            .unwrap()
            .ends_with("before")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn skills_reload_protects_canonical_schema_and_registry_aliases() {
    let (dir, runtime) = fixture_with_skill_map().await;
    call(&runtime, "framework_init", json!({"framework":"lyt"})).await;
    call(
        &runtime,
        "framework_register",
        json!({"name":"work","path":"_meta/work.yaml"}),
    )
    .await;
    tokio::fs::create_dir_all(dir.path().join("vault/Aliases"))
        .await
        .unwrap();
    tokio::fs::symlink(
        "../_meta/framework.yaml",
        dir.path().join("vault/Aliases/schema.md"),
    )
    .await
    .unwrap();
    tokio::fs::symlink(
        "../_meta/schemas.json",
        dir.path().join("vault/Aliases/registry.md"),
    )
    .await
    .unwrap();
    tokio::fs::write(
        dir.path().join("vault/Skill Map.md"),
        "[[Aliases/schema]]\n[[Aliases/registry]]\n",
    )
    .await
    .unwrap();
    call(&runtime, "skills_reload", json!({})).await;
    for (tool, args) in [
        ("framework_compose", json!({})),
        ("list_record_types", json!({})),
        ("find_maps", json!({})),
        ("get_vault_structure", json!({})),
        (
            "framework_init",
            json!({"framework":"para","mode":"overwrite"}),
        ),
        (
            "framework_register",
            json!({"name":"next","path":"_meta/next.yaml"}),
        ),
        ("framework_unregister", json!({"name":"work"})),
        ("framework_list", json!({})),
        ("framework_reload", json!({})),
    ] {
        let error = runtime
            .dispatch(tool, args.as_object().unwrap())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("blocked"), "{tool}: {error}");
    }
    assert!(
        tokio::fs::read_to_string(dir.path().join("vault/_meta/framework.yaml"))
            .await
            .unwrap()
            .contains("framework: lyt")
    );
    assert!(
        !tokio::fs::read_to_string(dir.path().join("vault/_meta/schemas.json"))
            .await
            .unwrap()
            .contains("next")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn skills_reload_blocks_unlisted_template_alias_to_private_target() {
    let (dir, runtime) = fixture_with_skill_map().await;
    call(&runtime, "framework_init", json!({"framework":"lyt"})).await;
    tokio::fs::create_dir_all(dir.path().join("vault/Aliases"))
        .await
        .unwrap();
    tokio::fs::write(
        dir.path().join("vault/x/Templates/Record.md"),
        "# Confidential template",
    )
    .await
    .unwrap();
    tokio::fs::symlink(
        "../x/Templates/Record.md",
        dir.path().join("vault/Aliases/template.md"),
    )
    .await
    .unwrap();
    tokio::fs::write(dir.path().join("vault/_meta/framework.yaml"),"version: 1\nschema_kind: base\ntypes:\n  note:\n    folder: Notes\n    template: Aliases/template.md\n").await.unwrap();
    call(
        &runtime,
        "create_record",
        json!({"type":"note","title":"Before"}),
    )
    .await;
    tokio::fs::write(
        dir.path().join("vault/Skill Map.md"),
        "[[x/Templates/Record]]",
    )
    .await
    .unwrap();
    call(&runtime, "skills_reload", json!({})).await;
    let error = runtime
        .dispatch(
            "create_record",
            json!({"type":"note","title":"After"}).as_object().unwrap(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("blocked"));
    assert!(!dir.path().join("vault/Notes/After.md").exists());
}
