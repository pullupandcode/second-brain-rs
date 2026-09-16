//! End-to-end tests: the aux HTTP surface and the vault read path.
//!
//! Integration tests are not compiled under `#[cfg(test)]`, so the
//! `allow-unwrap-in-tests` clippy config does not apply; allow the
//! test-appropriate lints here (a panic on failure fails the test).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::{collections::HashSet, sync::Arc};

use second_brain_rs::{
    auth::dev::DevAuthenticator,
    config::{ServerConfig, parse_config},
    http::{AppState, build_router},
    mcp::SecondBrainHandler,
    runtime::{DispatchError, Runtime},
};
use serde_json::{Map, Value, json};

/// Build a vault fixture and the matching config.
async fn fixture() -> (tempfile::TempDir, Arc<ServerConfig>) {
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
        vault.join("Notes/apple.md"),
        "---\ntags: [fruit]\n---\n# Apple\n\nA red apple. See [[Notes/banana]].",
    )
    .await
    .unwrap();
    tokio::fs::write(
        vault.join("Notes/banana.md"),
        "# Banana\n\nA yellow #fruit.",
    )
    .await
    .unwrap();
    tokio::fs::write(
        vault.join("Private/secret.md"),
        "# Secret\n\nclassified fruit",
    )
    .await
    .unwrap();
    tokio::fs::write(vault.join("Notes/n.md"), "# N\n\ncanonical")
        .await
        .unwrap();
    tokio::fs::write(
        vault.join("Notes/n.sync-conflict-7.md"),
        "# N\n\nconflicted",
    )
    .await
    .unwrap();

    let toml = format!(
        "listen = \"127.0.0.1:0\"\npublic_base_url = \"http://127.0.0.1:3000\"\n\
         vault_path = \"{vault}\"\nstate_path = \"{state}\"\n\
         [auth]\nmode = \"development\"\naudience = \"second-brain-rs\"\n\
         trusted_issuers = [\"https://idp.example.com/o/sb/\"]\n\
         discovery_authorization_server = \"https://idp.example.com/o/sb/\"\n\
         jwks_cache_ttl_seconds = 3600\n\
         [index]\nsqlite_path = \":memory:\"\nwatcher_polling = false\n\
         ignored_globs = [\"**/.DS_Store\", \"**/*.sync-conflict-*\"]\n\
         [security]\nblocked_paths = [\"Private/**\"]\n\
         [writes]\ncooldown_seconds = 0\n[daily_note]\ncapture_default_pattern = \"B\"\n\
         [logging]\nlog_args = false\n",
        vault = vault.display(),
        state = state.display()
    );
    (dir, Arc::new(parse_config(&toml).unwrap()))
}

async fn spawn() -> (tempfile::TempDir, String) {
    let (dir, config) = fixture().await;
    let runtime = Runtime::create(Arc::clone(&config)).await.unwrap();
    let handler = SecondBrainHandler::new(config.as_ref(), runtime);
    let state = AppState {
        config,
        authenticator: Arc::new(DevAuthenticator::new(HashSet::new())),
        handler,
    };
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (dir, format!("http://{addr}"))
}

async fn runtime() -> (tempfile::TempDir, Arc<Runtime>) {
    let (dir, config) = fixture().await;
    let runtime = Runtime::create(config).await.unwrap();
    (dir, runtime)
}

fn args(value: Value) -> Map<String, Value> {
    if let Value::Object(map) = value {
        map
    } else {
        unreachable!("test arguments are objects")
    }
}

// --- aux HTTP surface --------------------------------------------------------

#[tokio::test]
async fn healthz_ok() {
    let (_dir, base) = spawn().await;
    let resp = reqwest::get(format!("{base}/healthz")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.json::<Value>().await.unwrap()["ok"], true);
}

#[tokio::test]
async fn discovery_has_eight_scopes() {
    let (_dir, base) = spawn().await;
    let body: Value = reqwest::get(format!("{base}/.well-known/oauth-protected-resource"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["resource"], "http://127.0.0.1:3000");
    assert_eq!(body["scopes_supported"].as_array().unwrap().len(), 8);
}

#[tokio::test]
async fn tools_list_is_scope_filtered() {
    let (_dir, base) = spawn().await;
    let client = reqwest::Client::new();
    let body: Value = client
        .get(format!("{base}/tools"))
        .header("authorization", "Bearer scope=vault:read")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<String> = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"read_note".to_owned()));
    assert!(names.contains(&"search".to_owned()));
    assert!(!names.contains(&"create_note".to_owned()));
    assert!(!names.contains(&"list_vault_conflicts".to_owned()));
}

#[tokio::test]
async fn tools_list_requires_auth() {
    let (_dir, base) = spawn().await;
    let resp = reqwest::get(format!("{base}/tools")).await.unwrap();
    assert_eq!(resp.status(), 401);
    assert!(resp.headers().contains_key("www-authenticate"));
}

#[tokio::test]
async fn security_headers_present() {
    let (_dir, base) = spawn().await;
    let resp = reqwest::get(format!("{base}/healthz")).await.unwrap();
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert!(resp.headers().contains_key("content-security-policy"));
    assert!(!resp.headers().contains_key("x-powered-by"));
}

// --- vault read path ---------------------------------------------------------

#[tokio::test]
async fn read_note_returns_content_hash_and_parse() {
    let (_dir, runtime) = runtime().await;
    let value = runtime
        .dispatch("read_note", &args(json!({ "path": "Notes/apple.md" })))
        .await
        .unwrap();
    assert_eq!(value["currentSha256"].as_str().unwrap().len(), 64);
    assert_eq!(value["parsed"]["title"], "Apple");
    assert_eq!(value["parsed"]["outgoingLinks"][0], "Notes/banana");
}

#[tokio::test]
async fn search_matches_content_and_respects_folder_filter() {
    let (_dir, runtime) = runtime().await;
    let hits = runtime
        .dispatch("search", &args(json!({ "query": "apple" })))
        .await
        .unwrap();
    let paths: Vec<&str> = hits
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| hit["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["Notes/apple.md"]);

    let filtered = runtime
        .dispatch(
            "search",
            &args(json!({ "query": "", "filters": { "folder": "Notes" } })),
        )
        .await
        .unwrap();
    assert!(
        filtered
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["path"].as_str().unwrap().starts_with("Notes/"))
    );
}

#[tokio::test]
async fn backlinks_follow_wikilinks() {
    let (_dir, runtime) = runtime().await;
    let value = runtime
        .dispatch("get_backlinks", &args(json!({ "path": "Notes/banana" })))
        .await
        .unwrap();
    assert_eq!(value["backlinks"][0], "Notes/apple.md");
}

#[tokio::test]
async fn blocked_paths_are_invisible_and_unreadable() {
    let (_dir, runtime) = runtime().await;
    let hits = runtime
        .dispatch("search", &args(json!({ "query": "fruit" })))
        .await
        .unwrap();
    assert!(
        hits.as_array()
            .unwrap()
            .iter()
            .all(|hit| !hit["path"].as_str().unwrap().starts_with("Private/"))
    );

    let error = runtime
        .dispatch("read_note", &args(json!({ "path": "Private/secret.md" })))
        .await
        .unwrap_err();
    assert!(matches!(error, DispatchError::Invalid(message) if message.contains("blocked")));
}

#[tokio::test]
async fn conflicts_are_reported_and_excluded_from_search() {
    let (_dir, runtime) = runtime().await;
    let value = runtime
        .dispatch("list_vault_conflicts", &Map::new())
        .await
        .unwrap();
    let conflicts = value["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0]["canonical"], "Notes/n.md");
    assert_eq!(conflicts[0]["conflicts"][0], "Notes/n.sync-conflict-7.md");

    let hits = runtime
        .dispatch("search", &args(json!({ "query": "conflicted" })))
        .await
        .unwrap();
    assert!(hits.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn list_folder_and_vault_structure_skip_blocked() {
    let (_dir, runtime) = runtime().await;
    let value = runtime
        .dispatch("get_vault_structure", &Map::new())
        .await
        .unwrap();
    let folders: Vec<&str> = value["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap())
        .collect();
    assert!(folders.contains(&"Notes"));
    assert!(!folders.contains(&"Private"));

    let listed = runtime
        .dispatch(
            "list_folder",
            &args(json!({ "path": "Notes", "recursive": true })),
        )
        .await
        .unwrap();
    let paths: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"Notes/apple.md"));
    assert!(!paths.iter().any(|path| path.contains("sync-conflict")));
}

#[tokio::test]
async fn invalid_arguments_are_rejected() {
    let (_dir, runtime) = runtime().await;
    let error = runtime
        .dispatch("read_note", &Map::new())
        .await
        .unwrap_err();
    assert!(
        matches!(error, DispatchError::Invalid(message) if message == "path must be a non-empty string")
    );
}

async fn rpc(base: &str, scope: &str, method: &str, params: Value) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .header("authorization", format!("Bearer scope={scope}"))
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

#[tokio::test]
async fn mcp_roundtrip_enforces_each_request_scope() {
    let (_dir, base) = spawn().await;
    let init = rpc(&base, "vault:read", "initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"parity","version":"1"}})).await;
    assert!(init["result"]["capabilities"]["prompts"].is_object());
    assert_eq!(init["result"]["serverInfo"]["name"], env!("CARGO_PKG_NAME"));
    assert_eq!(
        init["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    let list = rpc(&base, "vault:read", "tools/list", json!({})).await;
    assert!(
        list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "read_note")
    );
    let read = rpc(
        &base,
        "vault:read",
        "tools/call",
        json!({"name":"read_note","arguments":{"path":"Notes/apple.md"}}),
    )
    .await;
    assert_eq!(
        read["result"]["structuredContent"]["parsed"]["title"],
        "Apple"
    );
    let denied = rpc(
        &base,
        "admin",
        "tools/call",
        json!({"name":"read_note","arguments":{"path":"Notes/apple.md"}}),
    )
    .await;
    assert_eq!(denied["error"]["code"], -32003);
    let root = rpc(
        &base,
        "vault:read",
        "tools/call",
        json!({"name":"list_folder","arguments":{"path":""}}),
    )
    .await;
    assert!(root["result"]["structuredContent"]["result"].is_array());
    let search = rpc(
        &base,
        "vault:read",
        "tools/call",
        json!({"name":"search","arguments":{"query":""}}),
    )
    .await;
    assert_eq!(
        search["result"]["structuredContent"]["result"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let unknown = rpc(
        &base,
        "vault:read",
        "tools/call",
        json!({"name":"not_a_tool","arguments":{}}),
    )
    .await;
    assert_eq!(unknown["error"]["code"], -32601);
    let pending = rpc(
        &base,
        "vault:write",
        "tools/call",
        json!({"name":"create_note","arguments":{"path":"x.md","content":"x"}}),
    )
    .await;
    assert!(pending["error"].is_object() || pending["result"]["isError"] == true);
}

#[tokio::test]
async fn mcp_rejects_unauthenticated_requests() {
    let (_dir, base) = spawn().await;
    let response = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    assert!(response.headers().contains_key("www-authenticate"));
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
}

#[test]
fn complete_tool_and_scope_registry() {
    use second_brain_rs::{auth::scopes::KNOWN_SCOPES, tools::registry::create_tool_registry};
    assert_eq!(KNOWN_SCOPES.len(), 8);
    assert_eq!(create_tool_registry(false).len(), 31);
    assert_eq!(create_tool_registry(true).len(), 34);
}

#[tokio::test]
async fn skill_reload_and_ocr_contract() {
    let (dir, config) = fixture().await;
    let vault = dir.path().join("vault");
    tokio::fs::write(vault.join("Map.md"), "[[Coach]]\n[[Broken]]")
        .await
        .unwrap();
    tokio::fs::write(
        vault.join("Coach.md"),
        "---\nname: coach\ndescription: Guidance\n---\nAsk a question.",
    )
    .await
    .unwrap();
    tokio::fs::write(vault.join("Broken.md"), "Secret invalid skill")
        .await
        .unwrap();
    let mut config = (*config).clone();
    // Exercise deserialization as well as runtime configuration.
    let raw = format!(
        "listen='127.0.0.1:0'\npublic_base_url='http://localhost:3000'\nvault_path='{}'\nstate_path='{}'\n[auth]\nmode='development'\naudience='a'\ntrusted_issuers=['https://issuer.example']\ndiscovery_authorization_server='https://issuer.example'\njwks_cache_ttl_seconds=60\n[index]\nsqlite_path=':memory:'\nwatcher_polling=false\nignored_globs=[]\n[writes]\ncooldown_seconds=0\n[daily_note]\ncapture_default_pattern='B'\n[logging]\nlog_args=false\n[skills]\nmap_paths=['./Map.md']\n[ocr]\nenabled=true",
        config.vault_path, config.state_path
    );
    config = parse_config(&raw).unwrap();
    let config = Arc::new(config);
    let runtime = Runtime::create(Arc::clone(&config)).await.unwrap();
    let handler = SecondBrainHandler::new(&config, Arc::clone(&runtime));
    let app = build_router(AppState {
        config,
        authenticator: Arc::new(DevAuthenticator::new(HashSet::new())),
        handler,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let skills = rpc(
        &base,
        "admin",
        "tools/call",
        json!({"name":"skills_list","arguments":{}}),
    )
    .await;
    assert_eq!(
        skills["result"]["structuredContent"]["skills"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let prompts = rpc(&base, "skills:read", "prompts/list", json!({})).await;
    assert_eq!(prompts["result"]["prompts"][0]["name"], "coach");
    let denied = rpc(
        &base,
        "vault:read admin",
        "prompts/get",
        json!({"name":"coach"}),
    )
    .await;
    assert_eq!(denied["error"]["code"], -32003);
    let prompt = rpc(&base, "skills:read", "prompts/get", json!({"name":"coach"})).await;
    assert_eq!(
        prompt["result"]["messages"][0]["content"]["text"],
        "Ask a question."
    );
    for path in ["Map.md", "Coach.md", "Broken.md"] {
        assert!(
            runtime
                .dispatch("read_note", &args(json!({"path":path})))
                .await
                .is_err()
        );
    }
    tokio::fs::write(
        vault.join("New.md"),
        "---\nname: new\ndescription: New skill\n---\nFresh.",
    )
    .await
    .unwrap();
    tokio::fs::write(vault.join("Map.md"), "[[New]]")
        .await
        .unwrap();
    rpc(
        &base,
        "admin",
        "tools/call",
        json!({"name":"skills_reload","arguments":{}}),
    )
    .await;
    let prompts = rpc(&base, "skills:read", "prompts/list", json!({})).await;
    assert_eq!(prompts["result"]["prompts"][0]["name"], "new");
    assert!(
        runtime
            .dispatch("read_note", &args(json!({"path":"New.md"})))
            .await
            .is_err()
    );
    assert!(
        runtime
            .dispatch("read_note", &args(json!({"path":"Coach.md"})))
            .await
            .is_ok()
    );
    check_ocr_contract(&base).await;
}

async fn check_ocr_contract(base: &str) {
    let job = rpc(base,"admin","tools/call",json!({"name":"ocr_notebook","arguments":{"identifier":"notebook", "pages":[1,3],"force":true}})).await;
    let job_id = &job["result"]["structuredContent"]["job_id"];
    assert!(job_id.is_string());
    let status = rpc(
        base,
        "admin",
        "tools/call",
        json!({"name":"ocr_status","arguments":{"job_id":job_id}}),
    )
    .await;
    assert_eq!(
        status["result"]["structuredContent"]["input"]["pages"],
        json!([1, 3])
    );
    assert_eq!(status["result"]["structuredContent"]["state"], "queued");
    let invalid = rpc(
        base,
        "admin",
        "tools/call",
        json!({"name":"ocr_notebook","arguments":{"identifier":"n","pages":[1.5]}}),
    )
    .await;
    assert_eq!(invalid["error"]["code"], -32602);
    let missing = rpc(
        base,
        "admin",
        "tools/call",
        json!({"name":"ocr_status","arguments":{"job_id":"absent"}}),
    )
    .await;
    assert_eq!(missing["error"]["data"]["code"], "job_missing");
    let renumber = rpc(
        base,
        "admin",
        "tools/call",
        json!({"name":"ocr_renumber_notebook","arguments":{"notebook_id":"n"}}),
    )
    .await;
    assert_eq!(renumber["result"]["structuredContent"]["type"], "renumber");
}

#[cfg(unix)]
#[tokio::test]
async fn skill_and_map_aliases_protect_canonical_targets() {
    let (dir, config) = fixture().await;
    let vault = dir.path().join("vault");
    tokio::fs::write(
        vault.join("Notes/skill.md"),
        "---\nname: alias\ndescription: Private skill\n---\nSecret instructions.",
    )
    .await
    .unwrap();
    tokio::fs::write(vault.join("Notes/map.md"), "[[AliasSkill]]")
        .await
        .unwrap();
    std::os::unix::fs::symlink(vault.join("Notes/skill.md"), vault.join("AliasSkill.md")).unwrap();
    std::os::unix::fs::symlink(vault.join("Notes/map.md"), vault.join("AliasMap.md")).unwrap();
    let mut config = (*config).clone();
    config.skills.map_paths = vec!["AliasMap.md".to_owned()];
    let runtime = Runtime::create(Arc::new(config)).await.unwrap();
    for path in [
        "AliasSkill.md",
        "AliasMap.md",
        "Notes/skill.md",
        "Notes/map.md",
    ] {
        assert!(
            runtime
                .dispatch("read_note", &args(json!({"path":path})))
                .await
                .is_err(),
            "{path}"
        );
        assert!(
            runtime.path_policy().is_blocked(path),
            "writer policy: {path}"
        );
    }
    let hits = runtime
        .dispatch("search", &args(json!({"query":"Secret"})))
        .await
        .unwrap();
    assert!(hits.as_array().unwrap().is_empty());
}

#[test]
fn every_schema_matches_pinned_reference_fixture() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/v1.1.1-tool-schemas.json")).unwrap();
    for tool in second_brain_rs::tools::registry::create_tool_registry(true) {
        assert_eq!(
            second_brain_rs::tools::registry::input_schema_for_tool(tool.name),
            reference[tool.name],
            "{}",
            tool.name
        );
    }
}

#[tokio::test]
async fn mcp_preflight_limits_parsing_and_header_binding() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let (_dir, config) = fixture().await;
    let runtime = Runtime::create(Arc::clone(&config)).await.unwrap();
    let router = build_router(AppState {
        handler: SecondBrainHandler::new(&config, runtime),
        authenticator: Arc::new(DevAuthenticator::new(HashSet::new())),
        config,
    });
    for (body, method_header, name_header, status, code) in [
        ("x".repeat(1_000_001), "", "", 413, -32700),
        ("{".to_owned(), "", "", 400, -32700),
        (json!({"jsonrpc":"2.0","method":true}).to_string(), "", "", 400, -32700),
        (json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string(), "tools/call", "", 400, -32600),
        (json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"read_note","arguments":{"path":"Notes/apple.md"}}}).to_string(), "tools/call", "different_tool", 400, -32600),
    ] {
        let mut builder = Request::builder().method("POST").uri("/mcp").header("accept", "application/json, text/event-stream").header("content-type", "application/json");
        if !method_header.is_empty() { builder = builder.header("mcp-method", method_header); }
        if !name_header.is_empty() { builder = builder.header("mcp-name", name_header); }
        let response = router.clone().oneshot(builder.body(Body::from(body)).unwrap()).await.unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        let bytes = to_bytes(response.into_body(), 10_000).await.unwrap();
        let error: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error["error"]["code"], code);
    }
}

#[tokio::test]
async fn listing_a_file_is_an_error() {
    let (_dir, runtime) = runtime().await;
    assert!(
        runtime
            .dispatch("list_folder", &args(json!({"path":"Notes/apple.md"})))
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn conflict_scan_never_opens_blocked_directories() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, config) = fixture().await;
    let private = dir.path().join("vault/Private");
    tokio::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o000))
        .await
        .unwrap();
    let result = Runtime::create(config).await;
    tokio::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700))
        .await
        .unwrap();
    assert!(
        result.is_ok(),
        "blocked subtree must not be traversed: {result:?}"
    );
}

#[tokio::test]
async fn malformed_search_filters_never_broaden_results() {
    let (_dir, runtime) = runtime().await;
    for filters in [
        json!({"tag":123}),
        json!({"folder":false}),
        json!({"tag":null}),
        json!([]),
    ] {
        assert!(
            runtime
                .dispatch("search", &args(json!({"query":"", "filters":filters})))
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn hard_denies_cover_configured_case_aliases_and_skill_reload() {
    let (dir, config) = fixture().await;
    let vault = dir.path().join("vault");
    tokio::fs::create_dir(vault.join("restricted"))
        .await
        .unwrap();
    tokio::fs::write(vault.join("restricted/secret.md"), "# Hidden\ncasecanary")
        .await
        .unwrap();
    tokio::fs::write(vault.join("SkillMap.md"), "# Initially empty")
        .await
        .unwrap();
    tokio::fs::write(
        vault.join("CaseSkill.md"),
        "---\nname: case-skill\ndescription: Private after reload\n---\nskillcanary",
    )
    .await
    .unwrap();
    let mut config = (*config).clone();
    config
        .security
        .blocked_paths
        .extend(["ReStricted/**".to_owned(), "Future/**".to_owned()]);
    config.skills.map_paths = vec!["SkillMap.md".to_owned()];
    let runtime = Runtime::create(Arc::new(config)).await.unwrap();
    let policy = runtime.path_policy();
    let before = runtime
        .dispatch("search", &args(json!({"query":"skillcanary"})))
        .await
        .unwrap();
    assert_eq!(before.as_array().unwrap().len(), 1);
    for reload in [false, true] {
        if reload {
            tokio::fs::write(vault.join("SkillMap.md"), "[[CaseSkill]]")
                .await
                .unwrap();
            runtime
                .dispatch("skills_reload", &Map::new())
                .await
                .unwrap();
            assert_eq!(runtime.loaded_skills().await.len(), 1);
            assert!(policy.is_blocked("caseskill.MD"));
            assert!(
                runtime
                    .dispatch("read_note", &args(json!({"path":"CaseSkill.md"})))
                    .await
                    .is_err()
            );
            assert!(
                runtime
                    .dispatch("search", &args(json!({"query":"skillcanary"})))
                    .await
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
        assert!(
            policy.is_blocked("future/missing/new.md"),
            "new write paths must stay blocked"
        );
        assert!(
            runtime
                .dispatch("read_note", &args(json!({"path":"restricted/secret.md"})))
                .await
                .is_err()
        );
        assert!(
            runtime
                .dispatch("list_folder", &args(json!({"path":"restricted"})))
                .await
                .is_err()
        );
        let listing = runtime
            .dispatch("list_folder", &args(json!({"path":"", "recursive":true})))
            .await
            .unwrap();
        assert!(
            !listing
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"].as_str().unwrap().starts_with("restricted/"))
        );
        assert!(
            runtime
                .dispatch("search", &args(json!({"query":"casecanary"})))
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            runtime
                .dispatch("read_note", &args(json!({"path":"Notes/apple.md"})))
                .await
                .is_ok()
        );
    }
}

#[tokio::test]
async fn hard_denies_normalize_unicode_across_reads_and_skill_reload() {
    let (dir, config) = fixture().await;
    let vault = dir.path().join("vault");
    let folder = "Future\u{301}";
    let private_path = format!("{folder}/private.md");
    tokio::fs::create_dir(vault.join(folder)).await.unwrap();
    tokio::fs::write(vault.join(&private_path), "normalizationcanary")
        .await
        .unwrap();
    tokio::fs::write(vault.join("Map.md"), "# Empty map")
        .await
        .unwrap();
    tokio::fs::write(
        vault.join("Cafe\u{301}.md"),
        "---\nname: cafe\ndescription: Normalized privacy\n---\nunicodeskillcanary",
    )
    .await
    .unwrap();
    let mut config = (*config).clone();
    config
        .security
        .blocked_paths
        .extend(["Futuré/**".to_owned(), "Uncréated/**".to_owned()]);
    config.skills.map_paths = vec!["Map.md".to_owned()];
    let runtime = Runtime::create(Arc::new(config)).await.unwrap();
    let policy = runtime.path_policy();
    for reload in [false, true] {
        if reload {
            tokio::fs::write(vault.join("Map.md"), "[[Cafe\u{301}.md]]")
                .await
                .unwrap();
            runtime
                .dispatch("skills_reload", &Map::new())
                .await
                .unwrap();
            assert_eq!(runtime.loaded_skills().await.len(), 1);
            assert!(policy.is_blocked("CAFÉ.MD"));
            assert!(
                runtime
                    .dispatch("read_note", &args(json!({"path":"Cafe\u{301}.md"})))
                    .await
                    .is_err()
            );
            assert!(
                runtime
                    .dispatch("search", &args(json!({"query":"unicodeskillcanary"})))
                    .await
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
        assert!(
            runtime
                .dispatch("read_note", &args(json!({"path":private_path})))
                .await
                .is_err()
        );
        assert!(
            runtime
                .dispatch("list_folder", &args(json!({"path":folder})))
                .await
                .is_err()
        );
        let listing = runtime
            .dispatch("list_folder", &args(json!({"path":"", "recursive":true})))
            .await
            .unwrap();
        assert!(
            !listing
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == private_path)
        );
        assert!(
            runtime
                .dispatch("search", &args(json!({"query":"normalizationcanary"})))
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(policy.is_blocked("Uncre\u{301}ated/missing/new.md"));
        assert!(
            runtime
                .dispatch("read_note", &args(json!({"path":"Notes/apple.md"})))
                .await
                .is_ok()
        );
    }
}
