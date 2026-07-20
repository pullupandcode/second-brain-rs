//! rmcp server handler: advertises scope-filtered tools and dispatches calls.

use std::{sync::Arc, time::Instant};

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, Content, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{
    auth::AuthContext,
    config::ServerConfig,
    observability::{OperationalLogEntry, ToolCallResult, log_operational},
    runtime::{DispatchError, Runtime},
    tools::registry::{
        ToolDefinition, create_tool_registry, input_schema_for_tool, list_tools_for_scopes,
    },
};

/// Shared server state behind the rmcp handler.
#[derive(Clone)]
pub struct SecondBrainHandler {
    inner: Arc<HandlerState>,
}

struct HandlerState {
    tools: Vec<ToolDefinition>,
    runtime: Arc<Runtime>,
}

impl std::fmt::Debug for SecondBrainHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecondBrainHandler").finish_non_exhaustive()
    }
}

impl SecondBrainHandler {
    /// Build the handler from config and runtime (tool set depends on OCR flag).
    #[must_use]
    pub fn new(config: &ServerConfig, runtime: Arc<Runtime>) -> Self {
        Self {
            inner: Arc::new(HandlerState {
                tools: create_tool_registry(config.ocr.enabled),
                runtime,
            }),
        }
    }

    /// Tools visible to the given auth context, as `(name, description, schema)`.
    #[must_use]
    pub fn visible_tools(&self, auth: &AuthContext) -> Vec<(&'static str, &'static str, Value)> {
        list_tools_for_scopes(&auth.scopes, &self.inner.tools)
            .into_iter()
            .map(|tool| {
                (
                    tool.name,
                    tool.description,
                    input_schema_for_tool(tool.name),
                )
            })
            .collect()
    }

    /// Whether the named tool may be called with the given scopes.
    #[must_use]
    pub fn tool_allowed(&self, auth: &AuthContext, name: &str) -> ToolAccess {
        match self.inner.tools.iter().find(|tool| tool.name == name) {
            None => ToolAccess::Unknown,
            Some(tool) if auth.scopes.contains(&tool.required_scope) => ToolAccess::Allowed,
            Some(_) => ToolAccess::Forbidden,
        }
    }
}

/// Result of a scope check for a named tool.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAccess {
    /// Caller may invoke the tool.
    Allowed,
    /// Tool exists but the caller lacks the scope.
    Forbidden,
    /// No such tool.
    Unknown,
}

fn tool_to_rmcp(name: &'static str, description: &'static str, schema: Value) -> Tool {
    let object = if let Value::Object(map) = schema {
        map
    } else {
        serde_json::Map::new()
    };
    Tool::new(name, description, Arc::new(object))
}

impl ServerHandler for SecondBrainHandler {
    fn get_info(&self) -> ServerInfo {
        // `ServerInfo::new` populates `server_info` from the crate env
        // (name = "second-brain-rs", version from Cargo.toml).
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = context
            .extensions
            .get::<AuthContext>()
            .map_or_else(Vec::new, |auth| {
                self.visible_tools(auth)
                    .into_iter()
                    .map(|(name, description, schema)| tool_to_rmcp(name, description, schema))
                    .collect()
            });
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let started = Instant::now();
        let name = request.name.to_string();
        let auth = context.extensions.get::<AuthContext>();
        let (subject, client_id) = auth.map_or_else(
            || ("anonymous".to_owned(), None),
            |auth| (auth.subject.clone(), auth.client_id.clone()),
        );
        let access = auth.map_or(ToolAccess::Forbidden, |auth| self.tool_allowed(auth, &name));
        let args: Map<String, Value> = request.arguments.clone().unwrap_or_default();

        let (result, outcome) = match access {
            ToolAccess::Unknown => (
                Err(ErrorData::invalid_params(
                    format!("unknown tool: {name}"),
                    None,
                )),
                ToolCallResult::Error,
            ),
            ToolAccess::Forbidden => (
                Err(ErrorData::invalid_request("forbidden_scope", None)),
                ToolCallResult::ForbiddenScope,
            ),
            ToolAccess::Allowed => match self.inner.runtime.dispatch(&name, &args).await {
                Ok(value) => (Ok(structured_result(&value)), ToolCallResult::Ok),
                Err(DispatchError::NotImplemented) => (
                    Ok(CallToolResult::success(vec![Content::text(
                        "not_implemented",
                    )])),
                    ToolCallResult::Ok,
                ),
                Err(DispatchError::UnknownTool(tool)) => (
                    Err(ErrorData::invalid_params(
                        format!("unknown tool: {tool}"),
                        None,
                    )),
                    ToolCallResult::Error,
                ),
                Err(DispatchError::Invalid(message)) => (
                    Err(ErrorData::invalid_params(message, None)),
                    ToolCallResult::Error,
                ),
                Err(DispatchError::Internal(message)) => (
                    Err(ErrorData::internal_error(message, None)),
                    ToolCallResult::Error,
                ),
            },
        };

        log_operational(&OperationalLogEntry {
            ts: now_rfc3339(),
            sub: subject,
            client_id,
            tool: name,
            args_hash: hash_args(&args),
            result: outcome,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        result
    }
}

fn structured_result(value: &Value) -> CallToolResult {
    let mut result = CallToolResult::success(vec![Content::text(value.to_string())]);
    let structured = if value.is_object() {
        value.clone()
    } else {
        json!({ "result": value })
    };
    result.structured_content = Some(structured);
    result
}

fn hash_args(args: &Map<String, Value>) -> String {
    let serialized = serde_json::to_string(args).unwrap_or_default();
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(serialized.as_bytes()))
    )
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{auth::scopes::Scope, config::parse_config};

    async fn handler() -> SecondBrainHandler {
        let dir = tempfile::tempdir().unwrap();
        let toml = format!(
            "listen = \"127.0.0.1:0\"\npublic_base_url = \"http://127.0.0.1:3000\"\n\
             vault_path = \"{path}\"\nstate_path = \"{path}\"\n\
             [auth]\nmode = \"development\"\naudience = \"a\"\n\
             trusted_issuers = [\"https://i.example.com/\"]\n\
             discovery_authorization_server = \"https://i.example.com/\"\n\
             jwks_cache_ttl_seconds = 60\n\
             [index]\nsqlite_path = \":memory:\"\nwatcher_polling = false\nignored_globs = []\n\
             [writes]\ncooldown_seconds = 0\n[daily_note]\ncapture_default_pattern = \"B\"\n\
             [logging]\nlog_args = false\n",
            path = dir.path().display()
        );
        let config = Arc::new(parse_config(&toml).unwrap());
        let runtime = Runtime::create(Arc::clone(&config)).await.unwrap();
        SecondBrainHandler::new(&config, runtime)
    }

    fn ctx(scopes: &[Scope]) -> AuthContext {
        AuthContext {
            subject: "t".to_owned(),
            scopes: scopes.iter().copied().collect(),
            client_id: None,
        }
    }

    #[tokio::test]
    async fn list_filters_by_scope() {
        let handler = handler().await;
        let names: HashSet<_> = handler
            .visible_tools(&ctx(&[Scope::VaultRead]))
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        assert!(names.contains("read_note"));
        assert!(!names.contains("create_note"));
    }

    #[tokio::test]
    async fn tool_access_enforces_scope() {
        let handler = handler().await;
        assert_eq!(
            handler.tool_allowed(&ctx(&[Scope::VaultRead]), "read_note"),
            ToolAccess::Allowed
        );
        assert_eq!(
            handler.tool_allowed(&ctx(&[Scope::VaultRead]), "create_note"),
            ToolAccess::Forbidden
        );
        assert_eq!(
            handler.tool_allowed(&ctx(&[Scope::VaultRead]), "no_such"),
            ToolAccess::Unknown
        );
    }
}
