//! Asynchronous JWT verification with bounded, per-issuer JWKS caching.

use std::{collections::HashSet, future::Future, pin::Pin, sync::Arc, time::Duration};

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
        if !self.algorithms.contains(&header.alg)
            || header.crit.as_ref().is_some_and(|crit| !crit.is_empty())
        {
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
            validation.leeway = 0;
            // NumericDate expiry is exclusive: exp == current second is expired.
            validation.reject_tokens_expiring_in_less_than = 1;
            validation.validate_nbf = true;
            validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
            validation.set_issuer(&[&issuer.name]);
            validation.set_audience(&[&self.audience]);
            let Ok(data) = decode::<Value>(token, &key, &validation) else {
                continue;
            };
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
    matches!(
        (&key.algorithm, header.alg),
        (
            jsonwebtoken::jwk::AlgorithmParameters::RSA(_),
            Algorithm::RS256
        ) | (
            jsonwebtoken::jwk::AlgorithmParameters::EllipticCurve(_),
            Algorithm::ES256
        )
    )
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
