//! TOML runtime configuration: parsing, validation, and defaults.

use std::path::PathBuf;

use serde::Deserialize;
use url::Url;

use crate::auth::scopes::Scope;

/// Capture default pattern. `A` = inline daily append; `B` = dated capture records.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureDefaultPattern {
    /// Inline marker append.
    A,
    /// Captures-as-files (default).
    B,
}

/// Authentication mode.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Validate bearer JWTs against issuer JWKS.
    Jwt,
    /// Local-loopback development tokens.
    Development,
}

/// Accepted JWT signing algorithm.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwtAlgorithm {
    /// RS256.
    Rs256,
    /// ES256.
    Es256,
}

/// Fully validated server configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ServerConfig {
    /// `host:port` listen address.
    pub listen: String,
    /// Public base URL used in discovery metadata.
    pub public_base_url: Url,
    /// Absolute vault directory.
    pub vault_path: String,
    /// Absolute runtime-state directory.
    pub state_path: String,
    /// Auth settings.
    pub auth: AuthConfig,
    /// Index settings.
    pub index: IndexConfig,
    /// Write settings.
    pub writes: WritesConfig,
    /// Audit settings.
    pub audit: AuditConfig,
    /// Framework settings.
    pub framework: FrameworkConfig,
    /// Daily-note settings.
    pub daily_note: DailyNoteConfig,
    /// OCR feature flag.
    pub ocr: OcrConfig,
    /// Logging settings.
    pub logging: LoggingConfig,
}

/// Auth configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuthConfig {
    /// Auth mode.
    pub mode: AuthMode,
    /// Expected token audience.
    pub audience: String,
    /// Trusted token issuers.
    pub trusted_issuers: Vec<Url>,
    /// Issuer advertised in discovery metadata.
    pub discovery_authorization_server: Url,
    /// JWKS cache TTL (seconds).
    pub jwks_cache_ttl_seconds: u64,
    /// Accepted signing algorithms.
    pub jwt_algorithms: Vec<JwtAlgorithm>,
    /// Default scopes granted to dev tokens without an explicit scope claim.
    pub development_default_scopes: Vec<Scope>,
}

/// Index configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct IndexConfig {
    /// SQLite index path (defaults to `{state}/index.sqlite`).
    pub sqlite_path: String,
    /// Use polling instead of native file watching.
    pub watcher_polling: bool,
    /// Globs excluded from indexing.
    pub ignored_globs: Vec<String>,
}

/// Write configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WritesConfig {
    /// Cooldown window before overwriting an existing file (seconds).
    pub cooldown_seconds: u64,
}

/// Audit configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuditConfig {
    /// Rotate when successful-write rows exceed this (0 = disabled).
    pub retention_max_rows: u64,
    /// Optional archive directory (defaults to `{state}/audit-archive`).
    pub archive_path: Option<String>,
}

/// Framework configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FrameworkConfig {
    /// Vault-relative base schema path.
    pub schema_path: String,
}

/// Daily-note configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DailyNoteConfig {
    /// Default capture pattern.
    pub capture_default_pattern: CaptureDefaultPattern,
}

/// OCR configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OcrConfig {
    /// Whether OCR tools are advertised.
    pub enabled: bool,
}

/// Logging configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoggingConfig {
    /// Whether to log raw tool arguments (sensitive; default false).
    pub log_args: bool,
}

/// Errors from loading or validating configuration.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// I/O error reading the config file.
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    /// TOML syntax error.
    #[error("failed to parse config TOML: {0}")]
    Toml(#[from] toml::de::Error),
    /// Semantic validation failure.
    #[error("invalid config: {0}")]
    Invalid(String),
}

/// Load and validate configuration from a TOML file.
///
/// # Errors
/// Returns [`ConfigError`] if the file cannot be read, the TOML is malformed,
/// or a value fails semantic validation.
pub fn load_config(path: &str) -> Result<ServerConfig, ConfigError> {
    let source = std::fs::read_to_string(path)?;
    parse_config(&source)
}

/// Parse and validate configuration from a TOML string.
///
/// # Errors
/// Returns [`ConfigError`] if the TOML is malformed or a value fails semantic
/// validation (bad URL, empty issuers, unknown capture pattern, non-loopback
/// dev auth, etc.).
pub fn parse_config(source: &str) -> Result<ServerConfig, ConfigError> {
    let raw: RawConfig = toml::from_str(source)?;
    raw.validate()
}

// --- raw (serde) shapes -------------------------------------------------------

#[derive(Deserialize)]
struct RawConfig {
    listen: String,
    public_base_url: String,
    vault_path: String,
    state_path: String,
    auth: RawAuth,
    index: RawIndex,
    writes: RawWrites,
    #[serde(default)]
    audit: RawAudit,
    #[serde(default)]
    framework: RawFramework,
    daily_note: RawDailyNote,
    #[serde(default)]
    ocr: RawOcr,
    logging: RawLogging,
}

#[derive(Deserialize)]
struct RawAuth {
    mode: Option<String>,
    audience: String,
    trusted_issuers: Vec<String>,
    discovery_authorization_server: String,
    jwks_cache_ttl_seconds: u64,
    jwt_algorithms: Option<Vec<String>>,
    development_default_scopes: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawIndex {
    sqlite_path: Option<String>,
    watcher_polling: bool,
    ignored_globs: Vec<String>,
}

#[derive(Deserialize)]
struct RawWrites {
    cooldown_seconds: u64,
}

#[derive(Deserialize, Default)]
struct RawAudit {
    retention_max_rows: Option<u64>,
    archive_path: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawFramework {
    schema_path: Option<String>,
}

#[derive(Deserialize)]
struct RawDailyNote {
    capture_default_pattern: String,
}

#[derive(Deserialize, Default)]
struct RawOcr {
    enabled: bool,
}

#[derive(Deserialize)]
struct RawLogging {
    log_args: bool,
}

impl RawConfig {
    fn validate(self) -> Result<ServerConfig, ConfigError> {
        let public_base_url = parse_http_url(&self.public_base_url, "public_base_url")?;
        let vault_path = expand_home(&self.vault_path);
        let state_path = expand_home(&self.state_path);

        if self.auth.trusted_issuers.is_empty() {
            return Err(invalid("auth.trusted_issuers must not be empty"));
        }
        let trusted_issuers = self
            .auth
            .trusted_issuers
            .iter()
            .map(|issuer| parse_http_url(issuer, "auth.trusted_issuers"))
            .collect::<Result<Vec<_>, _>>()?;
        let discovery_authorization_server = parse_http_url(
            &self.auth.discovery_authorization_server,
            "auth.discovery_authorization_server",
        )?;

        let mode = match self.auth.mode.as_deref() {
            None | Some("jwt") => AuthMode::Jwt,
            Some("development") => AuthMode::Development,
            Some(_) => return Err(invalid("auth.mode must be jwt or development")),
        };
        assert_dev_auth_is_local(&self.listen, mode)?;

        let jwt_algorithms = read_jwt_algorithms(self.auth.jwt_algorithms)?;
        let development_default_scopes = self
            .auth
            .development_default_scopes
            .map(|list| list.iter().filter_map(|scope| Scope::from_wire(scope)).collect())
            .unwrap_or_default();

        let capture_default_pattern = match self.daily_note.capture_default_pattern.as_str() {
            "A" => CaptureDefaultPattern::A,
            "B" => CaptureDefaultPattern::B,
            _ => return Err(invalid("daily_note.capture_default_pattern must be A or B")),
        };

        let sqlite_path = self
            .index
            .sqlite_path
            .unwrap_or_else(|| join_path(&state_path, "index.sqlite"));
        let schema_path = self
            .framework
            .schema_path
            .unwrap_or_else(|| "_meta/framework.yaml".to_owned());

        Ok(ServerConfig {
            listen: self.listen,
            public_base_url,
            vault_path,
            state_path,
            auth: AuthConfig {
                mode,
                audience: non_empty(self.auth.audience, "auth.audience")?,
                trusted_issuers,
                discovery_authorization_server,
                jwks_cache_ttl_seconds: self.auth.jwks_cache_ttl_seconds,
                jwt_algorithms,
                development_default_scopes,
            },
            index: IndexConfig {
                sqlite_path,
                watcher_polling: self.index.watcher_polling,
                ignored_globs: self.index.ignored_globs,
            },
            writes: WritesConfig { cooldown_seconds: self.writes.cooldown_seconds },
            audit: AuditConfig {
                retention_max_rows: self.audit.retention_max_rows.unwrap_or(0),
                archive_path: self.audit.archive_path,
            },
            framework: FrameworkConfig { schema_path },
            daily_note: DailyNoteConfig { capture_default_pattern },
            ocr: OcrConfig { enabled: self.ocr.enabled },
            logging: LoggingConfig { log_args: self.logging.log_args },
        })
    }
}

fn read_jwt_algorithms(value: Option<Vec<String>>) -> Result<Vec<JwtAlgorithm>, ConfigError> {
    match value {
        None => Ok(vec![JwtAlgorithm::Rs256]),
        Some(list) if list.is_empty() => Err(invalid("auth.jwt_algorithms must not be empty")),
        Some(list) => list
            .iter()
            .map(|alg| match alg.as_str() {
                "RS256" => Ok(JwtAlgorithm::Rs256),
                "ES256" => Ok(JwtAlgorithm::Es256),
                _ => Err(invalid("auth.jwt_algorithms must be RS256 or ES256")),
            })
            .collect(),
    }
}

fn invalid(message: &str) -> ConfigError {
    ConfigError::Invalid(message.to_owned())
}

fn non_empty(value: String, name: &str) -> Result<String, ConfigError> {
    if value.is_empty() {
        return Err(ConfigError::Invalid(format!("{name} must be a non-empty string")));
    }
    Ok(value)
}

fn parse_http_url(value: &str, name: &str) -> Result<Url, ConfigError> {
    let parsed = Url::parse(value)
        .map_err(|_| ConfigError::Invalid(format!("{name} must be a valid http(s) URL")))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(ConfigError::Invalid(format!("{name} must be a valid http(s) URL")));
    }
    Ok(parsed)
}

fn expand_home(value: &str) -> String {
    if value == "~" {
        return home_dir();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return join_path(&home_dir(), rest);
    }
    value.to_owned()
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn join_path(base: &str, rest: &str) -> String {
    let mut path = PathBuf::from(base);
    path.push(rest);
    path.to_string_lossy().into_owned()
}

fn assert_dev_auth_is_local(listen: &str, mode: AuthMode) -> Result<(), ConfigError> {
    if mode != AuthMode::Development
        || std::env::var("SECOND_BRAIN_ALLOW_DEV_AUTH").as_deref() == Ok("1")
    {
        return Ok(());
    }
    let host = listen.rsplit_once(':').map_or(listen, |(host, _)| host);
    if matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]") {
        Ok(())
    } else {
        Err(invalid(
            "development auth requires a loopback listen address or SECOND_BRAIN_ALLOW_DEV_AUTH=1",
        ))
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    /// Minimal valid config reused across module tests.
    pub(crate) const MINIMAL: &str = r#"
listen = "127.0.0.1:3000"
public_base_url = "http://127.0.0.1:3000"
vault_path = "/tmp/vault"
state_path = "/tmp/state"

[auth]
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
}

#[cfg(test)]
mod tests {
    use super::tests_support::MINIMAL;
    use super::*;

    #[test]
    fn parses_minimal_config_with_defaults() {
        let cfg = parse_config(MINIMAL).unwrap();
        assert_eq!(cfg.listen, "127.0.0.1:3000");
        assert_eq!(cfg.auth.mode, AuthMode::Jwt);
        assert_eq!(cfg.auth.jwt_algorithms, vec![JwtAlgorithm::Rs256]);
        assert_eq!(cfg.index.sqlite_path, "/tmp/state/index.sqlite");
        assert_eq!(cfg.audit.retention_max_rows, 0);
        assert_eq!(cfg.framework.schema_path, "_meta/framework.yaml");
        assert!(!cfg.ocr.enabled);
        assert_eq!(cfg.daily_note.capture_default_pattern, CaptureDefaultPattern::B);
    }

    #[test]
    fn rejects_empty_trusted_issuers() {
        let src = MINIMAL.replace(
            r#"trusted_issuers = ["https://idp.example.com/o/sb/"]"#,
            "trusted_issuers = []",
        );
        let err = parse_config(&src).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(msg) if msg.contains("trusted_issuers")));
    }

    #[test]
    fn rejects_bad_capture_pattern() {
        let src = MINIMAL.replace(
            r#"capture_default_pattern = "B""#,
            r#"capture_default_pattern = "C""#,
        );
        assert!(parse_config(&src).is_err());
    }

    #[test]
    fn dev_mode_requires_loopback() {
        let src = MINIMAL
            .replace(r#"listen = "127.0.0.1:3000""#, r#"listen = "0.0.0.0:3000""#)
            .replace("[auth]", "[auth]\nmode = \"development\"");
        let err = parse_config(&src).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(msg) if msg.contains("loopback")));
    }
}
