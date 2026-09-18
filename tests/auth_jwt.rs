//! Signed fixtures generated using the pinned reference's jose implementation.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use second_brain_rs::{
    auth::{
        AuthError, AuthErrorCode, Authenticator, build_authenticator,
        jwt::{JwksFetcher, JwksFuture, JwtAuthenticator},
        scopes::KNOWN_SCOPES,
    },
    config::parse_config,
};
use serde_json::Value;
use url::Url;

#[derive(Default)]
struct Fetcher {
    calls: AtomicUsize,
    body: Mutex<String>,
    endpoints: Mutex<Vec<String>>,
}
impl JwksFetcher for Fetcher {
    fn fetch<'a>(&'a self, endpoint: &'a Url) -> JwksFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.endpoints.lock().unwrap().push(endpoint.to_string());
        let body = self.body.lock().unwrap().clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            if body == "failure" {
                Err(AuthError::invalid("secret upstream path"))
            } else {
                Ok(body)
            }
        })
    }
}
fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/jwt.json")).unwrap()
}
fn header(name: &str) -> String {
    format!("Bearer {}", fixtures()["tokens"][name].as_str().unwrap())
}
fn setup() -> (JwtAuthenticator, Arc<Fetcher>) {
    let fetcher = Arc::new(Fetcher {
        body: Mutex::new(fixtures()["jwks"].to_string()),
        ..Fetcher::default()
    });
    let auth = JwtAuthenticator::with_fetcher(&parse_config(CONFIG).unwrap().auth, fetcher.clone())
        .unwrap();
    (auth, fetcher)
}

const CONFIG: &str = include_str!("fixtures/auth-config.toml");
#[tokio::test]
async fn rejects_signed_invalid_claims_and_signatures() {
    let fixtures: Value = serde_json::from_str(include_str!("fixtures/jwt.json")).unwrap();
    let (auth, _) = setup();
    for name in [
        "wrong_signature",
        "expired",
        "future",
        "wrong_issuer",
        "wrong_audience",
        "empty_subject",
        "missing_subject",
        "missing_expiry",
        "unknown_kid",
        "missing_issuer",
        "missing_audience",
        "hmac",
    ] {
        let header = format!("Bearer {}", fixtures["tokens"][name].as_str().unwrap());
        let error = auth.authenticate(Some(&header)).await.expect_err(name);
        assert_eq!(error.code, AuthErrorCode::InvalidToken, "{name}");
        assert!(!error.message.contains(&header));
    }
}
#[tokio::test]
async fn production_never_accepts_development_scope_claims() {
    let auth = build_authenticator(&parse_config(CONFIG).unwrap());
    assert!(
        auth.authenticate(Some("Bearer scope=vault:read admin"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn verifies_rsa_ec_identity_scopes_and_no_kid() {
    let (auth, fetcher) = setup();
    for name in ["valid", "ec", "no_kid"] {
        let ctx = auth.authenticate(Some(&header(name))).await.unwrap();
        assert_eq!(ctx.subject, "user-123");
        assert_eq!(ctx.issuer, "https://idp.example.com/o/sb/");
        assert_eq!(ctx.audience, "second-brain-rs");
        assert_eq!(ctx.token_id.as_deref(), Some("token-123"));
        assert_eq!(ctx.client_id.as_deref(), Some("test-client"));
        assert_eq!(ctx.scopes.len(), 2);
    }
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    let ctx = auth
        .authenticate(Some(&header("scope_array")))
        .await
        .unwrap();
    assert_eq!(ctx.scopes.len(), KNOWN_SCOPES.len());
    for name in ["scope_mixed", "scope_absent"] {
        assert!(
            auth.authenticate(Some(&header(name)))
                .await
                .unwrap()
                .scopes
                .is_empty()
        );
    }
    assert_eq!(
        fetcher.endpoints.lock().unwrap().as_slice(),
        ["https://idp.example.com/o/sb/.well-known/jwks.json"]
    );
}

#[tokio::test(start_paused = true)]
async fn rollover_is_guarded_then_refreshed_and_expiry_fails_closed() {
    let (auth, fetcher) = setup();
    auth.authenticate(Some(&header("valid"))).await.unwrap();
    assert!(
        auth.authenticate(Some(&header("unknown_kid")))
            .await
            .is_err()
    );
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    let mut rotated = fixtures()["jwks"].clone();
    rotated["keys"][0]["kid"] = "unknown".into();
    *fetcher.body.lock().unwrap() = rotated.to_string();
    tokio::time::advance(Duration::from_secs(30)).await;
    auth.authenticate(Some(&header("unknown_kid")))
        .await
        .unwrap();
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 2);
    *fetcher.body.lock().unwrap() = "failure".into();
    tokio::time::advance(Duration::from_secs(3600)).await;
    let err = auth
        .authenticate(Some(&header("unknown_kid")))
        .await
        .unwrap_err();
    assert_eq!(err.message, "Invalid bearer token");
    assert!(
        auth.authenticate(Some(&header("unknown_kid")))
            .await
            .is_err()
    );
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 3);
    tokio::time::advance(Duration::from_secs(30)).await;
    *fetcher.body.lock().unwrap() = fixtures()["jwks"].to_string();
    auth.authenticate(Some(&header("valid"))).await.unwrap();
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn concurrent_requests_share_one_fetch() {
    let (auth, fetcher) = setup();
    let auth = Arc::new(auth);
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let auth = Arc::clone(&auth);
        tasks.push(tokio::spawn(async move {
            auth.authenticate(Some(&header("valid"))).await.unwrap()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap().subject, "user-123");
    }
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn multiple_trusted_issuers_are_verified_exactly() {
    let (_, fetcher) = setup();
    let mut config = parse_config(CONFIG).unwrap().auth;
    config
        .trusted_issuers
        .push(Url::parse("https://second.example/").unwrap());
    let auth = JwtAuthenticator::with_fetcher(&config, fetcher.clone()).unwrap();
    assert_eq!(
        auth.authenticate(Some(&header("second_issuer")))
            .await
            .unwrap()
            .issuer,
        "https://second.example/"
    );
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 2);
    assert!(
        auth.authenticate(Some(&header("wrong_issuer")))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn malformed_missing_algorithms_and_key_metadata_fail_closed() {
    let (auth, fetcher) = setup();
    for token in [None, Some("Basic secret")] {
        assert_eq!(
            auth.authenticate(token).await.unwrap_err().code,
            AuthErrorCode::MissingToken
        );
    }
    for token in ["Bearer ", "Bearer malformed", "Bearer scope=admin"] {
        assert_eq!(
            auth.authenticate(Some(token)).await.unwrap_err().code,
            AuthErrorCode::InvalidToken
        );
    }
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
    let mut config = parse_config(CONFIG).unwrap().auth;
    config.jwt_algorithms = vec![second_brain_rs::config::JwtAlgorithm::Rs256];
    let auth = JwtAuthenticator::with_fetcher(&config, fetcher.clone()).unwrap();
    assert!(auth.authenticate(Some(&header("ec"))).await.is_err());
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
    for (key, value) in [
        ("alg", serde_json::json!("ES256")),
        ("use", serde_json::json!("enc")),
        ("key_ops", serde_json::json!(["sign"])),
    ] {
        let mut body = fixtures()["jwks"].clone();
        body["keys"][0][key] = value;
        *fetcher.body.lock().unwrap() = body.to_string();
        let auth = JwtAuthenticator::with_fetcher(&config, fetcher.clone()).unwrap();
        assert!(
            auth.authenticate(Some(&header("valid"))).await.is_err(),
            "{key}"
        );
    }
    let mut body = fixtures()["jwks"].clone();
    let duplicate = body["keys"][0].clone();
    body["keys"].as_array_mut().unwrap().push(duplicate);
    *fetcher.body.lock().unwrap() = body.to_string();
    let auth = JwtAuthenticator::with_fetcher(&config, fetcher.clone()).unwrap();
    assert!(auth.authenticate(Some(&header("valid"))).await.is_err());
    for body in [
        "failure".to_owned(),
        "invalid JSON".to_owned(),
        "x".repeat(1024 * 1024 + 1),
    ] {
        *fetcher.body.lock().unwrap() = body;
        let auth = JwtAuthenticator::with_fetcher(&config, fetcher.clone()).unwrap();
        assert_eq!(
            auth.authenticate(Some(&header("valid")))
                .await
                .unwrap_err()
                .message,
            "Invalid bearer token"
        );
    }
}

fn signed(claims: &Value, mut header: jsonwebtoken::Header) -> String {
    header.kid = Some("rsa".into());
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!("fixtures/rsa-test-only.pem"))
        .unwrap();
    format!(
        "Bearer {}",
        jsonwebtoken::encode(&header, claims, &key).unwrap()
    )
}
#[tokio::test]
async fn enforces_strict_expiry_boundary_and_claim_types() {
    let (auth, _) = setup();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let base = serde_json::json!({"iss":"https://idp.example.com/o/sb/", "aud":"second-brain-rs", "sub":"user", "exp":4_102_444_800_u64});
    for (key, value) in [
        ("exp", serde_json::json!(now)),
        ("exp", serde_json::json!("4102444800")),
        ("nbf", serde_json::json!("bad")),
        ("sub", serde_json::json!(12)),
    ] {
        let mut claims = base.clone();
        claims[key] = value;
        let token = signed(
            &claims,
            jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        );
        assert!(
            auth.authenticate(Some(&token)).await.is_err(),
            "{key}: {claims}"
        );
    }
}

#[tokio::test]
async fn rejects_invalid_constructor_configuration() {
    let (_, fetcher) = setup();
    for field in [
        "audience",
        "issuers",
        "algorithms",
        "scheme",
        "ftp_scheme",
        "username",
        "password",
    ] {
        let mut config = parse_config(CONFIG).unwrap().auth;
        match field {
            "audience" => config.audience.clear(),
            "issuers" => config.trusted_issuers.clear(),
            "algorithms" => config.jwt_algorithms.clear(),
            "scheme" => config.trusted_issuers = vec![Url::parse("file:///tmp/issuer").unwrap()],
            "ftp_scheme" => {
                config.trusted_issuers = vec![Url::parse("ftp://issuer.example/").unwrap()];
            }
            "username" => {
                config.trusted_issuers = vec![Url::parse("https://user@issuer.example/").unwrap()];
            }
            _ => {
                config.trusted_issuers =
                    vec![Url::parse("https://:password@issuer.example/").unwrap()];
            }
        }
        assert!(
            JwtAuthenticator::with_fetcher(&config, fetcher.clone()).is_err(),
            "{field}"
        );
    }
    let (auth, _) = setup();
    assert_eq!(format!("{auth:?}"), "JwtAuthenticator { .. }");
}

#[tokio::test]
async fn jwt_and_jwks_size_limits_accept_boundary_and_reject_overflow() {
    let (auth, fetcher) = setup();
    let jwks = fixtures()["jwks"].to_string();
    *fetcher.body.lock().unwrap() = format!("{jwks}{}", " ".repeat(1024 * 1024 - jwks.len()));
    auth.authenticate(Some(&header("valid"))).await.unwrap();
    for length in [1024 * 1024 + 1, 2 * 1024 * 1024] {
        *fetcher.body.lock().unwrap() = format!("{jwks}{}", " ".repeat(length - jwks.len()));
        let auth =
            JwtAuthenticator::with_fetcher(&parse_config(CONFIG).unwrap().auth, fetcher.clone())
                .unwrap();
        assert!(auth.authenticate(Some(&header("valid"))).await.is_err());
    }
    let mut claims = serde_json::json!({"iss":"https://idp.example.com/o/sb/", "aud":"second-brain-rs", "sub":"user", "exp":4_102_444_800_u64, "padding":""});
    let mut boundary_found = false;
    for length in 11_800..12_100 {
        claims["padding"] = "x".repeat(length).into();
        let token = signed(
            &claims,
            jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        );
        let length = token.len() - "Bearer ".len();
        if length == 16 * 1024 {
            assert!(auth.authenticate(Some(&token)).await.is_ok());
            boundary_found = true;
        }
        if length > 16 * 1024 {
            assert!(auth.authenticate(Some(&token)).await.is_err());
            break;
        }
    }
    assert!(
        boundary_found,
        "test must exercise the exact token size boundary"
    );
}

#[tokio::test]
async fn rejects_unknown_critical_header_extensions() {
    let (auth, _) = setup();
    let claims = serde_json::json!({"iss":"https://idp.example.com/o/sb/", "aud":"second-brain-rs", "sub":"user", "exp":4_102_444_800_u64});
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.crit = Some(vec!["unknown".into()]);
    assert!(
        auth.authenticate(Some(&signed(&claims, header)))
            .await
            .is_err()
    );
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.crit = Some(vec![]);
    assert!(
        auth.authenticate(Some(&signed(&claims, header)))
            .await
            .is_err()
    );
}
