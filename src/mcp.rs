//! rmcp server handler: advertises scope-filtered tools and dispatches calls.

use std::sync::Arc;

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, Content, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
};
use serde_json::Value;

use crate::{
    auth::AuthContext,
    config::ServerConfig,
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
}

impl std::fmt::Debug for SecondBrainHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecondBrainHandler").finish_non_exhaustive()
    }
}

impl SecondBrainHandler {
    /// Build the handler from config (tool set depends on the OCR flag).
    #[must_use]
    pub fn new(config: &ServerConfig) -> Self {
        Self {
            inner: Arc::new(HandlerState {
                tools: create_tool_registry(config.ocr.enabled),
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
        let name = request.name.as_ref();
        let access = context
            .extensions
            .get::<AuthContext>()
            .map_or(ToolAccess::Forbidden, |auth| self.tool_allowed(auth, name));

        match access {
            ToolAccess::Unknown => Err(ErrorData::invalid_params(
                format!("unknown tool: {name}"),
                None,
            )),
            ToolAccess::Forbidden => Err(ErrorData::invalid_request("forbidden_scope", None)),
            // Real dispatch arrives in Phases 2-4; Phase 1 confirms routing + scope.
            ToolAccess::Allowed => Ok(CallToolResult::success(vec![Content::text(
                "not_implemented",
            )])),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{
        auth::scopes::Scope,
        config::{parse_config, tests_support::MINIMAL},
    };

    fn handler() -> SecondBrainHandler {
        SecondBrainHandler::new(&parse_config(MINIMAL).unwrap())
    }

    fn ctx(scopes: &[Scope]) -> AuthContext {
        AuthContext {
            subject: "t".to_owned(),
            scopes: scopes.iter().copied().collect(),
            client_id: None,
        }
    }

    #[test]
    fn list_filters_by_scope() {
        let handler = handler();
        let names: HashSet<_> = handler
            .visible_tools(&ctx(&[Scope::VaultRead]))
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        assert!(names.contains("read_note"));
        assert!(!names.contains("create_note"));
    }

    #[test]
    fn tool_access_enforces_scope() {
        let handler = handler();
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
