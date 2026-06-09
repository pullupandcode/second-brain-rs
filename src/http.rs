//! HTTP surface: aux routes, auth middleware, OWASP headers, rmcp mount.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};

use crate::{
    auth::{AuthError, Authenticator, discovery::build_protected_resource_metadata},
    config::ServerConfig,
    mcp::SecondBrainHandler,
};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Validated config.
    pub config: Arc<ServerConfig>,
    /// Authenticator seam.
    pub authenticator: Arc<dyn Authenticator>,
    /// rmcp handler (tool listing + dispatch).
    pub handler: SecondBrainHandler,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

/// Build the full axum router (aux routes + `/mcp` + middleware layers).
pub fn build_router(state: AppState) -> Router {
    let handler = state.handler.clone();
    let mcp = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    Router::new()
        .route("/healthz", get(healthz))
        .route("/.well-known/oauth-protected-resource", get(discovery))
        .route("/tools", get(list_tools))
        .nest_service("/mcp", mcp)
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(serde_json::json!({ "ok": true }))
}

async fn discovery(State(state): State<AppState>) -> impl IntoResponse {
    Json(build_protected_resource_metadata(&state.config))
}

async fn list_tools(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    match state.authenticator.authenticate(authorization) {
        Ok(auth) => {
            let tools: Vec<_> = state
                .handler
                .visible_tools(&auth)
                .into_iter()
                .map(|(name, description, schema)| {
                    serde_json::json!({
                        "name": name,
                        "description": description,
                        "inputSchema": schema,
                    })
                })
                .collect();
            Json(serde_json::json!({ "tools": tools })).into_response()
        }
        Err(error) => auth_error_response(&error),
    }
}

fn auth_error_response(error: &AuthError) -> Response {
    let www = format!(
        "Bearer error=\"{}\", error_description=\"{}\"",
        error.code.as_str(),
        sanitize_header_value(&error.message)
    );
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": error.code.as_str(), "message": error.message })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&www) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

fn sanitize_header_value(value: &str) -> String {
    value.replace('"', "'").replace(['\r', '\n'], " ")
}

/// Tower middleware: add OWASP security headers to every response.
async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in OWASP_HEADERS {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    // Never advertise the server stack.
    headers.remove(header::SERVER);
    headers.remove(HeaderName::from_static("x-powered-by"));
    response
}

const OWASP_HEADERS: [(&str, &str); 10] = [
    (
        "strict-transport-security",
        "max-age=63072000; includeSubDomains",
    ),
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "deny"),
    (
        "content-security-policy",
        "default-src 'self'; form-action 'self'; object-src 'none'; frame-ancestors 'none'; upgrade-insecure-requests",
    ),
    ("referrer-policy", "no-referrer"),
    ("cache-control", "no-store, max-age=0"),
    ("x-dns-prefetch-control", "off"),
    ("cross-origin-opener-policy", "same-origin"),
    ("cross-origin-embedder-policy", "require-corp"),
    ("cross-origin-resource-policy", "same-origin"),
];
