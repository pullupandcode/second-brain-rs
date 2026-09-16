//! Authentication over actual loopback HTTP, including the production JWKS client.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use second_brain_rs::{
    auth::{build_authenticator, scopes::KNOWN_SCOPES},
    config::parse_config,
    http::{AppState, build_router},
    mcp::SecondBrainHandler,
    runtime::Runtime,
};
use serde_json::{Value, json};
use tokio::task::JoinHandle;

struct Server {
    base: String,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(router: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Server { base, task }
}
async fn idp() -> (Server, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let router = Router::new().route(
        "/issuer/.well-known/jwks.json",
        get(move || {
            let counted = Arc::clone(&counted);
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                let fixtures: Value =
                    serde_json::from_str(include_str!("fixtures/jwt.json")).unwrap();
                axum::Json(fixtures["jwks"].clone())
            }
        }),
    );
    (serve(router).await, calls)
}
fn config(issuer: &str) -> second_brain_rs::config::ServerConfig {
    parse_config(
        &include_str!("fixtures/auth-config.toml").replace("https://idp.example.com/o/sb/", issuer),
    )
    .unwrap()
}
fn token(issuer: &str, scopes: &str, algorithm: Algorithm) -> String {
    let mut header = Header::new(algorithm);
    let key = if algorithm == Algorithm::RS256 {
        header.kid = Some("rsa".into());
        EncodingKey::from_rsa_pem(include_bytes!("fixtures/rsa-test-only.pem")).unwrap()
    } else {
        header.kid = Some("ec".into());
        EncodingKey::from_ec_pem(include_bytes!("fixtures/ec-test-only.pem")).unwrap()
    };
    let claims = json!({"iss":issuer,"aud":"second-brain-rs","sub":"http-user","exp":4_102_444_800_u64,"scope":scopes});
    jsonwebtoken::encode(&header, &claims, &key).unwrap()
}
async fn app(issuer: &str) -> (tempfile::TempDir, Server) {
    app_with_public_url(issuer, None).await
}

async fn app_with_public_url(issuer: &str, public: Option<&str>) -> (tempfile::TempDir, Server) {
    app_with_options(issuer, public, false).await
}

async fn app_with_options(
    issuer: &str,
    public: Option<&str>,
    log_args: bool,
) -> (tempfile::TempDir, Server) {
    let dir = tempfile::tempdir().unwrap();
    tokio::fs::write(dir.path().join("test.md"), "# Authentication fixture\n")
        .await
        .unwrap();
    let mut config = config(issuer);
    config.logging.log_args = log_args;
    if let Some(public) = public {
        config.public_base_url = url::Url::parse(public).unwrap();
    }
    config.vault_path = dir.path().to_str().unwrap().to_owned();
    dir.path()
        .join("state")
        .to_str()
        .unwrap()
        .clone_into(&mut config.state_path);
    config.index.sqlite_path = ":memory:".into();
    let authenticator = build_authenticator(&config);
    let config = Arc::new(config);
    let runtime = Runtime::create(Arc::clone(&config)).await.unwrap();
    let handler = SecondBrainHandler::new(&config, runtime);
    let app = build_router(AppState {
        config,
        authenticator,
        handler,
    });
    (dir, serve(app).await)
}
async fn rpc(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    method: &str,
    params: Value,
) -> Value {
    let response = client
        .post(format!("{base}/mcp"))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, 200, "{text}");
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn real_jwks_client_verifies_both_algorithms_and_caches_keys() {
    let (idp, calls) = idp().await;
    let issuer = format!("{}/issuer/", idp.base);
    let auth = build_authenticator(&config(&issuer));
    for alg in [Algorithm::RS256, Algorithm::ES256] {
        let header = format!("Bearer {}", token(&issuer, "vault:read", alg));
        assert_eq!(
            auth.authenticate(Some(&header)).await.unwrap().subject,
            "http-user"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn production_mcp_authenticates_every_request_and_scope() {
    let (idp, calls) = idp().await;
    let issuer = format!("{}/issuer/", idp.base);
    let (_dir, server) = app(&issuer).await;
    let client = reqwest::Client::new();
    let reader = token(&issuer, "vault:read", Algorithm::RS256);
    let admin = token(&issuer, "admin", Algorithm::ES256);
    let init = rpc(&client, &server.base, &reader, "initialize", json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"jwt-test","version":"1"}})).await;
    assert!(init["result"]["capabilities"].is_object());
    let params = json!({"name":"read_note","arguments":{"path":"test.md"}});
    let read = rpc(&client, &server.base, &reader, "tools/call", params.clone()).await;
    assert_eq!(
        read["result"]["structuredContent"]["parsed"]["title"],
        "Authentication fixture"
    );
    let denied = rpc(&client, &server.base, &admin, "tools/call", params.clone()).await;
    assert_eq!(denied["error"]["code"], -32003);
    let read = rpc(&client, &server.base, &reader, "tools/call", params).await;
    assert!(read["result"].is_object());
    for scope in KNOWN_SCOPES {
        let scoped = token(&issuer, scope.as_str(), Algorithm::RS256);
        let list = rpc(&client, &server.base, &scoped, "tools/list", json!({})).await;
        assert!(list["result"]["tools"].is_array());
        if scope.as_str() == "skills:read" {
            assert!(rpc(&client, &server.base, &scoped, "prompts/list", json!({})).await["result"]["prompts"].is_array());
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn production_errors_are_401_sanitized_and_keep_security_headers() {
    let (idp, _) = idp().await;
    let issuer = format!("{}/issuer/", idp.base);
    let (_dir, server) = app(&issuer).await;
    let client = reqwest::Client::new();
    for path in ["/tools", "/mcp"] {
        for credential in [
            None,
            Some("Bearer secret-/private/path"),
            Some("Bearer scope=admin"),
        ] {
            let request = if path == "/tools" {
                client.get(format!("{}{path}", server.base))
            } else {
                client
                    .post(format!("{}{path}", server.base))
                    .header("accept", "application/json, text/event-stream")
                    .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            };
            let request = credential.map_or_else(
                || request.try_clone().unwrap(),
                |value| request.try_clone().unwrap().header("authorization", value),
            );
            let response = request.send().await.unwrap();
            assert_eq!(response.status(), 401);
            assert_eq!(response.headers()["x-content-type-options"], "nosniff");
            assert!(response.headers().contains_key("www-authenticate"));
            let text = response.text().await.unwrap();
            assert!(!text.contains("secret"));
            assert!(!text.contains("/private/path"));
            assert!(text.contains(if credential.is_none() {
                "missing_token"
            } else {
                "invalid_token"
            }));
        }
    }
}

#[tokio::test]
async fn jwks_network_failures_redirects_and_oversized_responses_are_denied() {
    for kind in ["status", "redirect", "oversized", "malformed"] {
        let router = Router::new().route(
            "/issuer/.well-known/jwks.json",
            get(move || async move {
                match kind {
                    "status" => (StatusCode::INTERNAL_SERVER_ERROR, "secret upstream failure")
                        .into_response(),
                    "redirect" => (
                        StatusCode::FOUND,
                        [(header::LOCATION, "http://127.0.0.1:1/private")],
                    )
                        .into_response(),
                    "oversized" => "x".repeat(1024 * 1024 + 1).into_response(),
                    _ => "not JSON /private/key".into_response(),
                }
            }),
        );
        let server = serve(router).await;
        let issuer = format!("{}/issuer/", server.base);
        let auth = build_authenticator(&config(&issuer));
        let header = format!("Bearer {}", token(&issuer, "admin", Algorithm::RS256));
        assert_eq!(
            auth.authenticate(Some(&header)).await.unwrap_err().message,
            "Invalid bearer token",
            "{kind}"
        );
    }
}

#[tokio::test]
async fn jwks_exact_payload_limit_is_accepted_over_http() {
    let fixtures: Value = serde_json::from_str(include_str!("fixtures/jwt.json")).unwrap();
    let body = fixtures["jwks"].to_string();
    let body = format!("{body}{}", " ".repeat(1024 * 1024 - body.len()));
    let router = Router::new().route(
        "/issuer/.well-known/jwks.json",
        get(move || {
            let body = body.clone();
            async move { body }
        }),
    );
    let server = serve(router).await;
    let issuer = format!("{}/issuer/", server.base);
    let auth = build_authenticator(&config(&issuer));
    let header = format!("Bearer {}", token(&issuer, "vault:read", Algorithm::RS256));
    assert!(auth.authenticate(Some(&header)).await.is_ok());
}

#[tokio::test]
async fn configured_public_host_is_allowed_and_other_hosts_remain_denied() {
    let (idp, _) = idp().await;
    let issuer = format!("{}/issuer/", idp.base);
    let (_dir, server) = app_with_public_url(&issuer, Some("https://brain.example:8443")).await;
    let client = reqwest::Client::new();
    let bearer = token(&issuer, "vault:read", Algorithm::RS256);
    for (host, status) in [
        ("brain.example:8443", 200),
        ("evil.example:8443", 403),
        ("brain.example:8444", 403),
    ] {
        let response = client
            .post(format!("{}/mcp", server.base))
            .header("host", host)
            .bearer_auth(&bearer)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{host}");
    }
}

#[test]
fn operational_argument_logging_is_opt_in_and_redacts_credentials() {
    // Global tracing callsite interest and spawned connection tasks must share a
    // stable dispatcher. Isolate this capture in its own test process so parallel
    // HTTP tests cannot register the same callsites without this subscriber.
    const CHILD: &str = "SECOND_BRAIN_AUTH_LOG_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "operational_argument_logging_is_opt_in_and_redacts_credentials",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "logging child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(check_operational_logs());
}

async fn check_operational_logs() {
    use std::{io::Write, sync::Mutex};
    #[derive(Clone)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buffer = Buffer(Arc::new(Mutex::new(Vec::new())));
    let captured = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(move || captured.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    for enabled in [false, true] {
        buffer.0.lock().unwrap().clear();
        let (idp, _) = idp().await;
        let issuer = format!("{}/issuer/", idp.base);
        let (_dir, server) = app_with_options(&issuer, None, enabled).await;
        let bearer = token(&issuer, "vault:read", Algorithm::RS256);
        let result = rpc(&reqwest::Client::new(), &server.base, &bearer, "tools/call", json!({"name":"read_note","arguments":{"path":"test.md","authorization":"Bearer SECRET","nested":{"password":"HIDDEN"}}})).await;
        assert!(result["result"].is_object());
        let bytes = buffer.0.lock().unwrap().clone();
        let output = String::from_utf8(bytes).unwrap();
        assert!(!output.contains("SECRET"));
        assert!(!output.contains("HIDDEN"));
        assert!(!output.contains(&bearer));
        let entries: Vec<Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let operational = entries
            .iter()
            .find(|entry| entry["target"] == "operational")
            .unwrap();
        let line: Value =
            serde_json::from_str(operational["fields"]["line"].as_str().unwrap()).unwrap();
        assert_eq!(line.get("args").is_some(), enabled);
        if enabled {
            assert_eq!(line["args"]["authorization"], "[REDACTED]");
            assert_eq!(line["args"]["path"], "test.md");
        }
        let auth = entries
            .iter()
            .find(|entry| entry["target"] == "authentication")
            .unwrap();
        assert_eq!(auth["fields"]["subject"], "http-user");
        assert!(
            auth["fields"]["source_ip"]
                .as_str()
                .unwrap()
                .contains("127.0.0.1")
        );
    }
}

#[tokio::test]
async fn public_default_port_accepts_implicit_authority_without_widening_ports() {
    let (idp, _) = idp().await;
    let issuer = format!("{}/issuer/", idp.base);
    let client = reqwest::Client::new();
    let bearer = token(&issuer, "vault:read", Algorithm::RS256);
    for (public, hosts) in [
        (
            "https://brain.example",
            ["brain.example", "brain.example:443", "brain.example:8444"],
        ),
        (
            "http://brain.example",
            ["brain.example", "brain.example:80", "brain.example:8444"],
        ),
        (
            "https://[2001:db8::1]",
            ["[2001:db8::1]", "[2001:db8::1]:443", "[2001:db8::1]:8444"],
        ),
    ] {
        let (_dir, server) = app_with_public_url(&issuer, Some(public)).await;
        for (index, host) in hosts.into_iter().enumerate() {
            let response = client
                .post(format!("{}/mcp", server.base))
                .header("host", host)
                .bearer_auth(&bearer)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if index == 2 { 403 } else { 200 },
                "{public} / {host}"
            );
        }
    }
}
