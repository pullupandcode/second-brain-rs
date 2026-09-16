//! Asynchronous JWT verification with bounded, per-issuer JWKS caching.

use std::{collections::HashSet, future::Future, pin::Pin, sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{
    Algorithm, DecodingKey, Header, Validation, decode, decode_header,
    jwk::{Jwk, JwkSet, KeyAlgorithm, KeyOperations, PublicKeyUse},
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use tokio::{sync::Mutex, time::Instant};
use url::Url;

use crate::{
    auth::{
        AuthContext, AuthError, AuthFuture, Authenticator,
        scopes::{Scope, parse_scopes},
    },
    config::{AuthConfig, JwtAlgorithm},
};

const MAX_JWKS_BYTES: usize = 1024 * 1024;
const MAX_TOKEN_BYTES: usize = 16 * 1024;
const REFRESH_COOLDOWN: Duration = Duration::from_secs(30);

/// An asynchronous fetch result. Implementations must not return credential details.
pub type JwksFuture<'a> = Pin<Box<dyn Future<Output = Result<String, AuthError>> + Send + 'a>>;

/// Injectable transport for public issuer keys; used by deterministic cache tests.
pub trait JwksFetcher: Send + Sync {
    /// Fetch the JWKS at a configured issuer's resolved endpoint.
    fn fetch<'a>(&'a self, endpoint: &'a Url) -> JwksFuture<'a>;
}

struct HttpFetcher {
    client: reqwest::Client,
}

impl JwksFetcher for HttpFetcher {
    fn fetch<'a>(&'a self, endpoint: &'a Url) -> JwksFuture<'a> {
        Box::pin(async move {
            // cancel-safe: dropping an in-flight GET only cancels the request.
            let mut response = self
                .client
                .get(endpoint.clone())
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|_| invalid())?;
            if response
                .content_length()
                .is_some_and(|size| size > MAX_JWKS_BYTES as u64)
            {
                return Err(invalid());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| invalid())? {
                if bytes.len().saturating_add(chunk.len()) > MAX_JWKS_BYTES {
                    return Err(invalid());
                }
                bytes.extend_from_slice(&chunk);
            }
            String::from_utf8(bytes).map_err(|_| invalid())
        })
    }
}

#[derive(Default)]
struct Cache {
    keys: Option<SecretString>,
    fetched_at: Option<Instant>,
    attempted_at: Option<Instant>,
}

struct Issuer {
    name: String,
    endpoint: Url,
    cache: Mutex<Cache>,
}

/// Production authenticator. No bearer tokens or decoded claims are retained.
pub struct JwtAuthenticator {
    audience: String,
    algorithms: Vec<Algorithm>,
    ttl: Duration,
    issuers: Vec<Issuer>,
    fetcher: Arc<dyn JwksFetcher>,
    clock: fn() -> u64,
}

impl std::fmt::Debug for JwtAuthenticator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtAuthenticator").finish_non_exhaustive()
    }
}

impl JwtAuthenticator {
    /// Build a certificate-validating TLS client with a five-second request limit.
    ///
    /// # Errors
    /// Returns a generic authentication error if the HTTP client cannot initialize.
    pub fn new(config: &AuthConfig) -> Result<Self, AuthError> {
        let client = reqwest::Client::builder()
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| invalid())?;
        Self::with_fetcher(config, Arc::new(HttpFetcher { client }))
    }

    /// Construct with an injected JWKS transport. The configuration is still enforced.
    ///
    /// # Errors
    /// Rejects empty trust configuration and invalid issuer endpoints.
    pub fn with_fetcher(
        config: &AuthConfig,
        fetcher: Arc<dyn JwksFetcher>,
    ) -> Result<Self, AuthError> {
        if config.audience.is_empty()
            || config.trusted_issuers.is_empty()
            || config.jwt_algorithms.is_empty()
        {
            return Err(invalid());
        }
        let issuers = config
            .trusted_issuers
            .iter()
            .map(|url| {
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err(invalid());
                }
                Ok(Issuer {
                    name: url.to_string(),
                    endpoint: url.join(".well-known/jwks.json").map_err(|_| invalid())?,
                    cache: Mutex::new(Cache::default()),
                })
            })
            .collect::<Result<Vec<_>, AuthError>>()?;
        Ok(Self {
            audience: config.audience.clone(),
            algorithms: config
                .jwt_algorithms
                .iter()
                .map(|alg| match alg {
                    JwtAlgorithm::Rs256 => Algorithm::RS256,
                    JwtAlgorithm::Es256 => Algorithm::ES256,
                })
                .collect(),
            ttl: Duration::from_secs(config.jwks_cache_ttl_seconds),
            issuers,
            fetcher,
            clock: jsonwebtoken::get_current_timestamp,
        })
    }

    // cancel-safe: cache only commits validated fetches; dropped fetches retain an
    // attempt timestamp to avoid repeated cancellation becoming a refresh flood.
    async fn keys(&self, issuer: &Issuer, header: &Header) -> Result<JwkSet, AuthError> {
        let mut cache = issuer.cache.lock().await;
        let now = Instant::now();
        let fresh = cache
            .fetched_at
            .is_some_and(|at| now.duration_since(at) < self.ttl);
        let current = cache
            .keys
            .as_ref()
            .and_then(|raw| serde_json::from_str::<JwkSet>(raw.expose_secret()).ok());
        let known = current
            .as_ref()
            .is_some_and(|set| set.keys.iter().any(|key| compatible(key, header)));
        if fresh && known {
            return current.ok_or_else(invalid);
        }
        let guarded = cache
            .attempted_at
            .is_some_and(|at| now.duration_since(at) < REFRESH_COOLDOWN);
        // A normal TTL expiry can refresh once; failed refreshes are throttled.
        let failed_attempt = cache.attempted_at != cache.fetched_at;
        if guarded && (fresh || failed_attempt) {
            return Err(invalid());
        }
        cache.attempted_at = Some(now);
        let raw = self
            .fetcher
            .fetch(&issuer.endpoint)
            .await
            .map_err(|_| invalid())?;
        if raw.len() > MAX_JWKS_BYTES {
            return Err(invalid());
        }
        let keys: JwkSet = serde_json::from_str(&raw).map_err(|_| invalid())?;
        cache.keys = Some(SecretString::from(raw));
        cache.fetched_at = Some(now);
        drop(cache);
        Ok(keys)
    }

    // cancel-safe: verification has no side effects beyond the guarded cache.
    async fn verify(&self, authorization: Option<&str>) -> Result<AuthContext, AuthError> {
        let token = authorization
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(|| AuthError::missing("Missing bearer token"))?;
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(invalid());
        }
        let header = decode_header(token).map_err(|_| invalid())?;
        let protected = token
            .split_once('.')
            .map(|(encoded, _)| encoded)
            .ok_or_else(invalid)?;
        let raw_header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(protected).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        if !self.algorithms.contains(&header.alg) || !valid_critical_header(&raw_header) {
            return Err(invalid());
        }
        for issuer in &self.issuers {
            let Ok(keys) = self.keys(issuer, &header).await else {
                continue;
            };
            let mut candidates = keys.keys.iter().filter(|key| compatible(key, &header));
            let Some(jwk) = candidates.next() else {
                continue;
            };
            if candidates.next().is_some() {
                continue;
            }
            let Ok(key) = DecodingKey::from_jwk(jwk) else {
                continue;
            };
            let mut validation = Validation::new(header.alg);
            // Keep signature/algorithm verification enabled. Registered claims are
            // checked below using raw JSON numbers: the library rounds NumericDate
            // values and accepts issuer arrays, unlike the pinned jose reference.
            validation.required_spec_claims.clear();
            validation.validate_exp = false;
            validation.validate_nbf = false;
            validation.validate_aud = false;
            let Ok(data) = decode::<Value>(token, &key, &validation) else {
                continue;
            };
            if !valid_claims(&data.claims, &issuer.name, &self.audience, (self.clock)()) {
                continue;
            }
            let Some(subject) = data
                .claims
                .get("sub")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            return Ok(AuthContext {
                subject: subject.to_owned(),
                issuer: issuer.name.clone(),
                audience: self.audience.clone(),
                scopes: claim_scopes(data.claims.get("scope")),
                token_id: data
                    .claims
                    .get("jti")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                client_id: data
                    .claims
                    .get("client_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
        Err(invalid())
    }
}

impl Authenticator for JwtAuthenticator {
    fn authenticate<'a>(&'a self, authorization: Option<&'a str>) -> AuthFuture<'a> {
        Box::pin(self.verify(authorization))
    }
}

// The typed Header collapses explicit null into absence. Preserve raw JSON for
// jose's critical-parameter rules, then verify the original signed bytes.
fn valid_critical_header(header: &Value) -> bool {
    match header.get("crit") {
        None => true,
        Some(Value::Array(names)) => {
            !names.is_empty()
                && names.iter().all(|name| name.as_str() == Some("b64"))
                && header.get("b64").and_then(Value::as_bool) == Some(true)
        }
        _ => false,
    }
}

fn compatible(key: &Jwk, header: &Header) -> bool {
    if header
        .kid
        .as_ref()
        .is_some_and(|kid| key.common.key_id.as_ref() != Some(kid))
    {
        return false;
    }
    let expected = match header.alg {
        Algorithm::RS256 => KeyAlgorithm::RS256,
        Algorithm::ES256 => KeyAlgorithm::ES256,
        Algorithm::HS256
        | Algorithm::HS384
        | Algorithm::HS512
        | Algorithm::ES384
        | Algorithm::RS384
        | Algorithm::RS512
        | Algorithm::PS256
        | Algorithm::PS384
        | Algorithm::PS512
        | Algorithm::EdDSA
        | _ => return false,
    };
    if key.common.key_algorithm.is_some_and(|alg| alg != expected) {
        return false;
    }
    if key
        .common
        .public_key_use
        .as_ref()
        .is_some_and(|usage| *usage != PublicKeyUse::Signature)
    {
        return false;
    }
    if key
        .common
        .key_operations
        .as_ref()
        .is_some_and(|ops| !ops.contains(&KeyOperations::Verify))
    {
        return false;
    }
    match (&key.algorithm, header.alg) {
        (jsonwebtoken::jwk::AlgorithmParameters::RSA(_), Algorithm::RS256) => true,
        (jsonwebtoken::jwk::AlgorithmParameters::EllipticCurve(ec), Algorithm::ES256) => {
            ec.curve == jsonwebtoken::jwk::EllipticCurve::P256
        }
        _ => false,
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "NumericDate follows JavaScript number comparison semantics"
)]
fn valid_claims(claims: &Value, issuer: &str, audience: &str, now: u64) -> bool {
    let now = now as f64;
    let audience_matches = match claims.get("aud") {
        Some(Value::String(value)) => value == audience,
        Some(Value::Array(values)) => values.iter().any(|value| value.as_str() == Some(audience)),
        _ => false,
    };
    claims.get("iss").and_then(Value::as_str) == Some(issuer)
        && audience_matches
        && claims
            .get("exp")
            .and_then(Value::as_f64)
            .is_some_and(|exp| exp > now)
        && claims
            .get("nbf")
            .is_none_or(|nbf| nbf.as_f64().is_some_and(|nbf| nbf <= now))
        && claims.get("iat").is_none_or(Value::is_number)
}

fn claim_scopes(value: Option<&Value>) -> HashSet<Scope> {
    match value {
        Some(Value::String(claim)) => parse_scopes(claim),
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => values
            .iter()
            .filter_map(Value::as_str)
            .filter_map(Scope::from_wire)
            .collect(),
        _ => HashSet::new(),
    }
}

fn invalid() -> AuthError {
    AuthError::invalid("Invalid bearer token")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use jsonwebtoken::{EncodingKey, encode};
    use serde_json::json;

    use super::*;

    struct StaticKeys(String);
    impl JwksFetcher for StaticKeys {
        fn fetch<'a>(&'a self, _endpoint: &'a Url) -> JwksFuture<'a> {
            Box::pin(std::future::ready(Ok(self.0.clone())))
        }
    }
    fn setup(curve: Option<&str>) -> JwtAuthenticator {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/jwt.json")).unwrap();
        let mut keys = fixtures["jwks"].clone();
        if let Some(curve) = curve {
            for key in keys["keys"].as_array_mut().unwrap() {
                if key["kty"] == "EC" {
                    key["crv"] = curve.into();
                }
            }
        }
        let config =
            crate::config::parse_config(include_str!("../../tests/fixtures/auth-config.toml"))
                .unwrap();
        JwtAuthenticator::with_fetcher(&config.auth, Arc::new(StaticKeys(keys.to_string())))
            .unwrap()
    }
    fn claims() -> Value {
        json!({"iss":"https://idp.example.com/o/sb/", "aud":"second-brain-rs", "sub":"review-user", "exp":4_102_444_800_u64})
    }
    fn signed(claims: &Value, algorithm: Algorithm) -> String {
        let key = if algorithm == Algorithm::ES256 {
            EncodingKey::from_ec_pem(include_bytes!("../../tests/fixtures/ec-test-only.pem"))
                .unwrap()
        } else {
            EncodingKey::from_rsa_pem(include_bytes!("../../tests/fixtures/rsa-test-only.pem"))
                .unwrap()
        };
        format!(
            "Bearer {}",
            encode(&Header::new(algorithm), claims, &key).unwrap()
        )
    }
    fn signed_raw(header: &Value, payload: &Value) -> String {
        let message = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(payload.to_string())
        );
        let key =
            EncodingKey::from_rsa_pem(include_bytes!("../../tests/fixtures/rsa-test-only.pem"))
                .unwrap();
        let signature =
            jsonwebtoken::crypto::sign(message.as_bytes(), &key, Algorithm::RS256).unwrap();
        format!("Bearer {message}.{signature}")
    }
    #[tokio::test]
    async fn raw_critical_header_matrix_preserves_signature_verification() {
        let auth = setup(None);
        for (extension, accepted) in [
            (json!({}), true),
            (json!({"crit":[]}), false),
            (json!({"crit":null}), false),
            (json!({"crit":"b64"}), false),
            (json!({"crit":12}), false),
            (json!({"crit":[""]}), false),
            (json!({"crit":[null]}), false),
            (json!({"crit":["unknown"],"unknown":true}), false),
            (json!({"crit":["b64"],"b64":true}), true),
            (json!({"crit":["b64","b64"],"b64":true}), true),
            (json!({"crit":["b64"]}), false),
            (json!({"crit":["b64"],"b64":false}), false),
            (json!({"crit":["b64"],"b64":null}), false),
            (json!({"crit":["b64"],"b64":"true"}), false),
            (json!({"b64":true}), true),
            (json!({"b64":false}), true),
            (json!({"b64":null}), true),
            (json!({"b64":"true"}), true),
        ] {
            let mut header = json!({"alg":"RS256"});
            header
                .as_object_mut()
                .unwrap()
                .extend(extension.as_object().unwrap().clone());
            let token = signed_raw(&header, &claims());
            assert_eq!(
                auth.authenticate(Some(&token)).await.is_ok(),
                accepted,
                "{header}"
            );
            if accepted {
                let (message, signature) = token.rsplit_once('.').unwrap();
                let replacement = if signature.starts_with('A') { 'B' } else { 'A' };
                let corrupt = format!("{message}.{replacement}{}", &signature[1..]);
                assert!(
                    auth.authenticate(Some(&corrupt)).await.is_err(),
                    "corrupt signature: {header}"
                );
            }
        }
    }
    #[tokio::test]
    async fn signed_scope_strings_use_ecmascript_separators_and_arrays_stay_exact() {
        let auth = setup(None);
        for (scope, count) in [
            (json!("vault:read\u{85}admin"), 0),
            (json!("vault:read\u{feff}admin"), 2),
            (json!(["vault:read\u{feff}admin"]), 0),
            (json!(["vault:read", "admin"]), 2),
            (json!(["vault:read", null]), 0),
        ] {
            let mut payload = claims();
            payload["scope"] = scope;
            let result = auth
                .authenticate(Some(&signed(&payload, Algorithm::RS256)))
                .await
                .unwrap();
            assert_eq!(result.scopes.len(), count, "{}", payload["scope"]);
        }
    }
    #[tokio::test]
    async fn registered_claim_shapes_match_jose() {
        let auth = setup(None);
        for (field, value) in [
            ("iss", json!(["https://idp.example.com/o/sb/"])),
            (
                "iss",
                json!([
                    "https://untrusted.example/",
                    "https://idp.example.com/o/sb/"
                ]),
            ),
            ("iat", json!("yesterday")),
            ("iat", Value::Null),
            ("iat", json!([])),
            ("iat", json!(true)),
            ("iat", json!({})),
            ("iss", Value::Null),
            ("iss", json!(12)),
            ("exp", Value::Null),
            ("exp", json!(false)),
            ("exp", json!([])),
            ("nbf", Value::Null),
            ("nbf", json!(false)),
            ("nbf", json!([])),
        ] {
            let mut payload = claims();
            payload[field] = value;
            assert!(
                auth.authenticate(Some(&signed(&payload, Algorithm::RS256)))
                    .await
                    .is_err(),
                "{field}: {}",
                payload[field]
            );
        }
        for value in [json!(0), json!(-1.25), json!(4_102_444_800_u64)] {
            let mut payload = claims();
            payload["iat"] = value;
            assert!(
                auth.authenticate(Some(&signed(&payload, Algorithm::RS256)))
                    .await
                    .is_ok()
            );
        }
    }
    #[tokio::test]
    async fn audience_string_and_array_membership_match_jose() {
        let auth = setup(None);
        for (audience, accepted) in [
            (json!("second-brain-rs"), true),
            (json!(["other", "second-brain-rs"]), true),
            (json!([null, "second-brain-rs"]), true),
            (json!(["other"]), false),
            (json!([]), false),
            (Value::Null, false),
            (json!(123), false),
        ] {
            let mut payload = claims();
            payload["aud"] = audience;
            assert_eq!(
                auth.authenticate(Some(&signed(&payload, Algorithm::RS256)))
                    .await
                    .is_ok(),
                accepted,
                "{}",
                payload["aud"]
            );
        }
    }
    #[tokio::test]
    async fn fractional_numeric_dates_use_unrounded_comparison() {
        let mut auth = setup(None);
        auth.clock = || 100;
        for (exp, nbf, accepted) in [
            (100.25, 100.0, true),
            (100.0, 100.0, false),
            (100.75, 100.25, false),
            (101.0, 99.75, true),
            (4_102_444_800.0, 100.25, false),
            (99.75, 0.0, false),
            (101.0, -0.25, true),
        ] {
            let mut payload = claims();
            payload["exp"] = json!(exp);
            payload["nbf"] = json!(nbf);
            assert_eq!(
                auth.authenticate(Some(&signed(&payload, Algorithm::RS256)))
                    .await
                    .is_ok(),
                accepted,
                "exp={exp}, nbf={nbf}"
            );
        }
    }
    #[tokio::test]
    async fn es256_requires_p256_curve_metadata() {
        let token = signed(&claims(), Algorithm::ES256);
        assert!(setup(None).authenticate(Some(&token)).await.is_ok());
        for curve in ["P-384", "P-521", "Ed25519"] {
            assert!(
                setup(Some(curve)).authenticate(Some(&token)).await.is_err(),
                "{curve}"
            );
        }
    }
}
