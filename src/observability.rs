//! Tracing setup and the structured operational-log event.
//!
//! No `println!`/`eprintln!` anywhere in the crate (lint-enforced); all output
//! goes through `tracing`.

use serde::Serialize;

/// Result of a tool call, for the operational log.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallResult {
    /// Success.
    Ok,
    /// Handler error.
    Error,
    /// Scope check failed.
    ForbiddenScope,
}

/// One operational-log line (emitted as a structured tracing event).
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct OperationalLogEntry {
    /// RFC3339 timestamp.
    pub ts: String,
    /// Subject.
    pub sub: String,
    /// Client id, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Tool name.
    pub tool: String,
    /// SHA-256 of the JSON arguments (`sha256:<hex>`).
    pub args_hash: String,
    /// Optional operator-enabled arguments; known credential fields are redacted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    /// Outcome.
    pub result: ToolCallResult,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// Initialize the global tracing subscriber with JSON output.
///
/// Safe to call once at startup; a second call is ignored.
pub fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().json())
        .try_init();
}

/// Emit one operational-log entry as a structured tracing event.
pub fn log_operational(entry: &OperationalLogEntry) {
    match serde_json::to_string(entry) {
        Ok(line) => tracing::info!(target: "operational", %line),
        Err(error) => tracing::error!(%error, "failed to serialize operational log entry"),
    }
}

/// Emit authentication outcome without token contents or internal error details.
pub fn log_authentication(
    result: &Result<crate::auth::AuthContext, crate::auth::AuthError>,
    source_ip: Option<std::net::IpAddr>,
) {
    match result {
        Ok(auth) => {
            tracing::info!(target: "authentication", result = "ok", subject = %auth.subject, issuer = %auth.issuer, source_ip = ?source_ip);
        }
        Err(error) => {
            tracing::warn!(target: "authentication", result = "denied", code = error.code.as_str(), source_ip = ?source_ip);
        }
    }
}

/// Redact conventional credential fields before explicitly enabled argument logging.
#[must_use]
pub fn redact_arguments(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let name = key.to_ascii_lowercase();
                    let sensitive = [
                        "authorization",
                        "token",
                        "secret",
                        "password",
                        "api_key",
                        "private_key",
                        "credential",
                    ]
                    .iter()
                    .any(|part| name.contains(part));
                    (
                        key.clone(),
                        if sensitive {
                            Value::String("[REDACTED]".to_owned())
                        } else {
                            redact_arguments(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact_arguments).collect()),
        Value::String(text) if text.starts_with("Bearer ") => {
            Value::String("[REDACTED]".to_owned())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
    }
}
#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn serializes_without_client_id() {
        let entry = OperationalLogEntry {
            ts: "2026-06-07T00:00:00Z".to_owned(),
            sub: "u1".to_owned(),
            client_id: None,
            tool: "read_note".to_owned(),
            args_hash: "sha256:abc".to_owned(),
            args: None,
            result: ToolCallResult::Ok,
            duration_ms: 5,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert!(json.get("client_id").is_none());
        assert_eq!(
            json.get("result").and_then(serde_json::Value::as_str),
            Some("ok")
        );
    }
    #[test]
    fn arguments_redact_nested_credentials_and_bearer_values() {
        let args = serde_json::json!({"path":"note.md", "content":"private note", "access_token":"secret", "nested":[{"password":"private", "other":"Bearer secret", "api_key":"key"}]});
        let safe = redact_arguments(&args);
        assert_eq!(safe["path"], "note.md");
        assert_eq!(safe["content"], "private note");
        assert_eq!(safe["access_token"], "[REDACTED]");
        assert_eq!(safe["nested"][0]["password"], "[REDACTED]");
        assert_eq!(safe["nested"][0]["other"], "[REDACTED]");
        assert_eq!(safe["nested"][0]["api_key"], "[REDACTED]");
    }

    #[test]
    fn authentication_logs_identity_and_code_without_secrets() {
        use std::{
            collections::HashSet,
            io::Write,
            sync::{Arc, Mutex},
        };
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
        tracing::subscriber::with_default(subscriber, || {
            log_authentication(
                &Ok(crate::auth::AuthContext {
                    subject: "user-123".into(),
                    issuer: "https://issuer.example/".into(),
                    audience: "test".into(),
                    scopes: HashSet::new(),
                    client_id: None,
                    token_id: Some("private-token-id".into()),
                }),
                Some(std::net::Ipv4Addr::LOCALHOST.into()),
            );
            log_authentication(
                &Err(crate::auth::AuthError::invalid(
                    "Bearer SECRET /private/key",
                )),
                None,
            );
        });
        let bytes = buffer.0.lock().unwrap().clone();
        let output = String::from_utf8(bytes).unwrap();
        let lines: Vec<serde_json::Value> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["fields"]["result"], "ok");
        assert_eq!(lines[0]["fields"]["subject"], "user-123");
        assert_eq!(lines[0]["fields"]["issuer"], "https://issuer.example/");
        assert_eq!(lines[1]["fields"]["result"], "denied");
        assert_eq!(lines[1]["fields"]["code"], "invalid_token");
        for secret in ["SECRET", "/private/key", "private-token-id", "Bearer"] {
            assert!(!output.contains(secret));
        }
    }
}
