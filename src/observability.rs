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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_without_client_id() {
        let entry = OperationalLogEntry {
            ts: "2026-06-07T00:00:00Z".to_owned(),
            sub: "u1".to_owned(),
            client_id: None,
            tool: "read_note".to_owned(),
            args_hash: "sha256:abc".to_owned(),
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
}
