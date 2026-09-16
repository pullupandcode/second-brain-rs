//! HTTP surface: aux routes, auth middleware, OWASP headers, rmcp mount.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderName, HeaderValue, StatusCode, header},
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
    let mut transport = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true);
    let public = &state.config.public_base_url;
    if let Some(port) = public.port_or_known_default() {
        let host = &public[url::Position::BeforeHost..url::Position::AfterHost];
        transport.allowed_hosts.push(format!("{host}:{port}"));
    }
    let mcp = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        transport,
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
    let original_numeric_id = if request.method() == axum::http::Method::POST {
        let checked = match preflight(request).await {
            Ok(checked) => checked,
            Err(response) => return *response.0,
        };
        request = checked.request;
        checked.original_numeric_id
    } else {
        None
    };
    normalize_public_authority(&mut request, &state.config.public_base_url);
    let authorization = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    let authenticated = state.authenticator.authenticate(authorization).await;
    crate::observability::log_authentication(&authenticated, peer_ip(&request));
    match authenticated {
        Ok(auth) => {
            request.extensions_mut().insert(auth);
            let response = next.run(request).await;
            if let Some(id) = original_numeric_id {
                restore_numeric_id(response, id).await
            } else {
                response
            }
        }
        Err(error) => auth_error_response(&error),
    }
}

struct PreflightRequest {
    request: axum::extract::Request,
    original_numeric_id: Option<serde_json::Value>,
}

// rmcp compares explicit ports literally. Normalize only the configured public
// authority's omitted standard port; other hosts and ports retain its protection.
fn normalize_public_authority(request: &mut axum::extract::Request, public: &url::Url) {
    if public.port().is_some() {
        return;
    }
    let host = &public[url::Position::BeforeHost..url::Position::AfterHost];
    let authority = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            request
                .uri()
                .authority()
                .map(axum::http::uri::Authority::as_str)
        });
    if authority.is_some_and(|authority| authority.eq_ignore_ascii_case(host))
        && let Some(port) = public.port_or_known_default()
        && let Ok(value) = HeaderValue::from_str(&format!("{host}:{port}"))
    {
        request.headers_mut().insert(header::HOST, value);
    }
}

struct PreflightResponse(Box<Response>);
impl PreflightResponse {
    fn new(response: Response) -> Self {
        Self(Box::new(response))
    }
}

// cancel-safe: reads a bounded body; no application state is mutated.
async fn preflight(request: axum::extract::Request) -> Result<PreflightRequest, PreflightResponse> {
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
    let mut message: Value = serde_json::from_slice(&bytes)
        .map_err(|_| rpc_error(StatusCode::BAD_REQUEST, None, -32700, "Parse error"))?;
    normalize_json_numbers(&mut message)
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
            || (method == "tools/call"
                && message
                    .pointer("/params/arguments")
                    .is_some_and(|value| !value.is_object())))
    {
        return Err(rpc_error(StatusCode::OK, id, -32602, "Invalid params"));
    }
    let result = match method {
        "initialize" => Some(initialize_result(&message)),
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
    // rmcp only accepts i64/string IDs. The reference accepts every finite JSON
    // number. This mapping is local to one stateless HTTP request, never a session.
    let original_numeric_id = id
        .filter(|id| id.is_number() && id.as_i64().is_none())
        .cloned();
    if let Some(id) = &original_numeric_id
        && let Some(object) = message.as_object_mut()
    {
        object.insert("id".to_owned(), Value::String(format!("number:{id}")));
    }
    normalize_method_params(&mut message);
    normalize_transport_headers(&mut parts.headers);
    let body = Body::from(message.to_string());
    Ok(PreflightRequest {
        request: axum::extract::Request::from_parts(parts, body),
        original_numeric_id,
    })
}

// The reference accepts JSON regardless of Content-Type/Accept; normalize for rmcp.
fn normalize_transport_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.remove(header::CONTENT_LENGTH);
}

// The reference reads only protocolVersion; initialization needs no auth or
// client capability payload and accepts unknown nonempty protocol versions.
fn initialize_result(message: &serde_json::Value) -> serde_json::Value {
    let protocol = message
        .pointer("/params/protocolVersion")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("2025-03-26");
    serde_json::json!({
        "protocolVersion": protocol,
        "capabilities": {"tools":{},"prompts":{}},
        "serverInfo": {"name":env!("CARGO_PKG_NAME"),"version":env!("CARGO_PKG_VERSION")}
    })
}

// Strip fields the reference ignores before rmcp's stricter typed decoding.
// Header binding and method-specific validation have already inspected the
// original request. Tool argument objects remain intact for runtime validation.
fn normalize_method_params(message: &mut serde_json::Value) {
    use serde_json::{Value, json};
    let params = match message.get("method").and_then(Value::as_str) {
        Some("tools/list" | "prompts/list") => None,
        Some("prompts/get") => Some(json!({"name":message.pointer("/params/name")})),
        Some("tools/call") => Some(json!({
            "name":message.pointer("/params/name"),
            "arguments":message.pointer("/params/arguments").cloned().unwrap_or_else(||json!({}))
        })),
        _ => return,
    };
    if let Some(object) = message.as_object_mut() {
        if let Some(params) = params {
            object.insert("params".to_owned(), params);
        } else {
            object.remove("params");
        }
    }
}

// JSON.parse represents every number as binary64. Preserve its rounded value,
// while retaining integer variants for rmcp and integer argument validators.
fn normalize_json_numbers(value: &mut serde_json::Value) -> Result<(), serde_json::Error> {
    use serde_json::Value;
    match value {
        Value::Number(number) => {
            if let Some(float) = number.as_f64() {
                let mut buffer = ryu_js::Buffer::new();
                *number = serde_json::from_str(buffer.format(float))?;
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_json_numbers(value)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                normalize_json_numbers(value)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::String(_) => {}
    }
    Ok(())
}

// cancel-safe: only transforms the completed stateless JSON response body.
async fn restore_numeric_id(response: Response, id: serde_json::Value) -> Response {
    if response.status() != StatusCode::OK {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    // Responses are already materialized JSON in this stateless transport. No new
    // output-size limit is imposed on read_note results.
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return *rpc_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(&id),
            -32603,
            "Internal server error",
        )
        .0;
    };
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return *rpc_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(&id),
            -32603,
            "Internal server error",
        )
        .0;
    };
    if let Some(object) = value.as_object_mut() {
        object.insert("id".to_owned(), id);
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, axum::body::Body::from(value.to_string()))
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

async fn list_tools(State(state): State<AppState>, request: axum::extract::Request) -> Response {
    let authorization = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let authenticated = state.authenticator.authenticate(authorization).await;
    crate::observability::log_authentication(&authenticated, peer_ip(&request));
    match authenticated {
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

fn peer_ip(request: &axum::extract::Request) -> Option<std::net::IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|peer| peer.0.ip())
}

fn auth_error_response(error: &AuthError) -> Response {
    let www = format!(
        "Bearer error=\"{}\", error_description=\"{}\"",
        error.code.as_str(),
        match error.code {
            crate::auth::AuthErrorCode::MissingToken => "Missing bearer token",
            crate::auth::AuthErrorCode::InvalidToken => "Invalid bearer token",
        }
    );
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": error.code.as_str(), "message": match error.code { crate::auth::AuthErrorCode::MissingToken => "Missing bearer token", crate::auth::AuthErrorCode::InvalidToken => "Invalid bearer token" } })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&www) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
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
