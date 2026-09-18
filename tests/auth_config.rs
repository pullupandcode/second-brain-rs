//! Authentication configuration boundaries, without mutating process environment.
#![allow(clippy::unwrap_used)]
use second_brain_rs::config::{AuthMode, JwtAlgorithm, parse_config};

const CONFIG: &str = include_str!("fixtures/auth-config.toml");

#[test]
fn auth_validation_rejects_empty_or_unrecognized_trust_configuration() {
    for replacement in ["", "HS256", "none", "RS512", "rs256"] {
        let algs = if replacement.is_empty() {
            "[]".to_owned()
        } else {
            format!("[\"{replacement}\"]")
        };
        assert!(parse_config(&CONFIG.replace("[\"RS256\", \"ES256\"]", &algs)).is_err());
    }
    for (old, new) in [
        ("audience = \"second-brain-rs\"", "audience = \"\""),
        (
            "trusted_issuers = [\"https://idp.example.com/o/sb/\"]",
            "trusted_issuers = []",
        ),
        ("mode = \"jwt\"", "mode = \"auto\""),
        (
            "jwks_cache_ttl_seconds = 3600",
            "jwks_cache_ttl_seconds = -1",
        ),
        (
            "jwks_cache_ttl_seconds = 3600",
            "jwks_cache_ttl_seconds = 1.5",
        ),
    ] {
        assert!(parse_config(&CONFIG.replace(old, new)).is_err(), "{new}");
    }
    let config =
        parse_config(&CONFIG.replace("jwt_algorithms = [\"RS256\", \"ES256\"]", "")).unwrap();
    assert_eq!(config.auth.jwt_algorithms, [JwtAlgorithm::Rs256]);
    assert_eq!(config.auth.mode, AuthMode::Jwt);
}

#[test]
fn urls_must_be_http_or_https() {
    for bad in [
        "ftp://issuer.example/",
        "file:///tmp/issuer",
        "invalid url",
        "urn:issuer",
    ] {
        assert!(
            parse_config(&CONFIG.replace("https://idp.example.com/o/sb/", bad)).is_err(),
            "{bad}"
        );
        assert!(
            parse_config(&CONFIG.replace("http://127.0.0.1:3000", bad)).is_err(),
            "{bad}"
        );
    }
    for good in ["http://issuer.example/", "https://issuer.example/path/"] {
        assert!(parse_config(&CONFIG.replace("https://idp.example.com/o/sb/", good)).is_ok());
    }
}

#[test]
fn development_requires_loopback_while_jwt_allows_network_binding() {
    let dev = CONFIG.replace("mode = \"jwt\"", "mode = \"development\"");
    for host in ["127.0.0.1", "localhost", "[::1]"] {
        assert!(
            parse_config(&dev.replace(
                "listen = \"127.0.0.1:0\"",
                &format!("listen = \"{host}:0\"")
            ))
            .is_ok()
        );
    }
    for host in ["0.0.0.0", "example.com", "127.0.0.1.evil", "[::]"] {
        let replacement = format!("listen = \"{host}:0\"");
        assert!(parse_config(&dev.replace("listen = \"127.0.0.1:0\"", &replacement)).is_err());
        assert!(parse_config(&CONFIG.replace("listen = \"127.0.0.1:0\"", &replacement)).is_ok());
    }
}

#[test]
fn explicit_development_override_is_process_isolated() {
    let executable = std::env::current_exe().unwrap();
    for setting in ["1", "0", "true"] {
        let result = std::process::Command::new(&executable)
            .args(["--exact", "development_override_child", "--nocapture"])
            .env("SECOND_BRAIN_ALLOW_DEV_AUTH", setting)
            .env("AUTH_OVERRIDE_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}

#[test]
fn development_override_child() {
    if std::env::var("AUTH_OVERRIDE_CHILD").as_deref() != Ok("1") {
        return;
    }
    let config = CONFIG
        .replace("mode = \"jwt\"", "mode = \"development\"")
        .replace("listen = \"127.0.0.1:0\"", "listen = \"0.0.0.0:0\"");
    assert_eq!(
        parse_config(&config).is_ok(),
        std::env::var("SECOND_BRAIN_ALLOW_DEV_AUTH").as_deref() == Ok("1")
    );
}
