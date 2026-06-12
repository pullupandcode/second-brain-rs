//! End-to-end HTTP tests against an in-process server.
//!
//! Integration tests are not compiled under `#[cfg(test)]`, so the
//! `allow-unwrap-in-tests` clippy config does not apply; allow the
//! test-appropriate lints here (a panic on failure fails the test).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::{collections::HashSet, sync::Arc};

use second_brain_rs::{
    auth::dev::DevAuthenticator,
    config::parse_config,
    http::{AppState, build_router},
    mcp::SecondBrainHandler,
};

const CONFIG: &str = r#"
listen = "127.0.0.1:0"
public_base_url = "http://127.0.0.1:3000"
vault_path = "/tmp/vault"
state_path = "/tmp/state"

[auth]
mode = "development"
audience = "second-brain-rs"
trusted_issuers = ["https://idp.example.com/o/sb/"]
discovery_authorization_server = "https://idp.example.com/o/sb/"
jwks_cache_ttl_seconds = 3600

[index]
watcher_polling = false
ignored_globs = ["**/.DS_Store"]

[writes]
cooldown_seconds = 2

[daily_note]
capture_default_pattern = "B"

[logging]
log_args = false
"#;

async fn spawn() -> String {
    let config = parse_config(CONFIG).unwrap();
    let handler = SecondBrainHandler::new(&config);
    let state = AppState {
        config: Arc::new(config),
        authenticator: Arc::new(DevAuthenticator::new(HashSet::new())),
        handler,
    };
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn healthz_ok() {
    let base = spawn().await;
    let resp = reqwest::get(format!("{base}/healthz")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.json::<serde_json::Value>().await.unwrap()["ok"], true);
}

#[tokio::test]
async fn discovery_has_five_scopes() {
    let base = spawn().await;
    let body: serde_json::Value =
        reqwest::get(format!("{base}/.well-known/oauth-protected-resource"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(body["resource"], "http://127.0.0.1:3000");
    assert_eq!(body["scopes_supported"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn tools_list_is_scope_filtered() {
    let base = spawn().await;
    let client = reqwest::Client::new();
    let body: serde_json::Value = client
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
    assert!(!names.contains(&"create_note".to_owned()));
}

#[tokio::test]
async fn tools_list_requires_auth() {
    let base = spawn().await;
    let resp = reqwest::get(format!("{base}/tools")).await.unwrap();
    assert_eq!(resp.status(), 401);
    assert!(resp.headers().contains_key("www-authenticate"));
}

#[tokio::test]
async fn security_headers_present() {
    let base = spawn().await;
    let resp = reqwest::get(format!("{base}/healthz")).await.unwrap();
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert!(resp.headers().contains_key("content-security-policy"));
}
