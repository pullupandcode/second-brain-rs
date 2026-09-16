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
        StreamableHttpServerConfig::default()
            .with_stateful_mode(false)
            .with_json_response(true),
    );

    let protected =
        Router::new()
            .nest_service("/mcp", mcp)
            .route_layer(axum::middleware::from_fn_with_state(
                state.clone(),
                authenticate_mcp,
            ));
    Router::new()
        .route("/healthz", get(healthz))
        .route("/.well-known/oauth-protected-resource", get(discovery))
        .route("/tools", get(list_tools))
        .merge(protected)
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

// cancel-safe: authentication and extension insertion have no external mutations.
async fn authenticate_mcp(
    State(state): State<AppState>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::POST {
        request = match preflight(request).await {
            Ok(request) => request,
            Err(response) => return *response.0,
        };
    }
    let authorization = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    match state.authenticator.authenticate(authorization) {
        Ok(auth) => {
            request.extensions_mut().insert(auth);
            next.run(request).await
        }
        Err(error) => auth_error_response(&error),
    }
}

struct PreflightResponse(Box<Response>);
impl PreflightResponse {
    fn new(response: Response) -> Self {
        Self(Box::new(response))
    }
}

// cancel-safe: reads a bounded body; no application state is mutated.
async fn preflight(
    request: axum::extract::Request,
) -> Result<axum::extract::Request, PreflightResponse> {
    use axum::body::{Body, to_bytes};
    use serde_json::{Value, json};
    const LIMIT: usize = 1_000_000;
    let (mut parts, body) = request.into_parts();
    let bytes = to_bytes(body, LIMIT).await.map_err(|_| {
        rpc_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            None,
            -32700,
            "Request body too large",
        )
    })?;
    let message: Value = serde_json::from_slice(&bytes)
        .map_err(|_| rpc_error(StatusCode::BAD_REQUEST, None, -32700, "Parse error"))?;
    let id = message.get("id");
    let method = message.get("method").and_then(Value::as_str);
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || method.is_none()
        || id.is_some_and(|id| !id.is_string() && !id.is_number())
    {
        return Err(rpc_error(
            StatusCode::BAD_REQUEST,
            None,
            -32700,
            "Parse error",
        ));
    }
    let method = method.unwrap_or_default();
    let name = match method {
        "tools/call" | "prompts/get" => message.pointer("/params/name").and_then(Value::as_str),
        "resources/read" => message.pointer("/params/uri").and_then(Value::as_str),
        _ => None,
    };
    for (header, expected) in [("mcp-method", Some(method)), ("mcp-name", name)] {
        if let Some(actual) = parts.headers.get(header)
            && actual.to_str().ok() != expected
        {
            return Err(rpc_error(
                StatusCode::BAD_REQUEST,
                id,
                -32600,
                "MCP headers do not match JSON-RPC body",
            ));
        }
    }
    if id.is_none() {
        return Err(PreflightResponse::new(StatusCode::ACCEPTED.into_response()));
    }
    if matches!(method, "tools/call" | "prompts/get")
        && (name.is_none_or(str::is_empty)
            || message
                .pointer("/params/arguments")
                .is_some_and(|value| !value.is_object()))
    {
        return Err(rpc_error(StatusCode::OK, id, -32602, "Invalid params"));
    }
    let result = match method {
        "resources/list" => Some(json!({"resources":[]})),
        "resources/templates/list" => Some(json!({"resourceTemplates":[]})),
        "ping" => Some(json!({})),
        _ => None,
    };
    if let Some(result) = result {
        return Err(PreflightResponse::new(
            Json(json!({"jsonrpc":"2.0", "id": id, "result": result})).into_response(),
        ));
    }
    if !matches!(
        method,
        "initialize" | "tools/list" | "tools/call" | "prompts/list" | "prompts/get"
    ) {
        return Err(rpc_error(StatusCode::OK, id, -32601, "Method not found"));
    }
    // The reference accepts JSON regardless of Content-Type/Accept; normalize for rmcp.
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    parts.headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    Ok(axum::extract::Request::from_parts(parts, Body::from(bytes)))
}

fn rpc_error(
    status: StatusCode,
    id: Option<&serde_json::Value>,
    code: i32,
    message: &str,
) -> PreflightResponse {
    let mut body = serde_json::json!({"jsonrpc":"2.0", "error":{"code": code, "message": message}});
    if let Some(id) = id
        && let Some(object) = body.as_object_mut()
    {
        object.insert("id".to_owned(), id.clone());
    }
    PreflightResponse::new((status, Json(body)).into_response())
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
