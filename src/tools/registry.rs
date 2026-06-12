//! Static MCP tool definitions, scope filtering, and JSON input schemas.

use std::{collections::HashSet, hash::BuildHasher};

use serde_json::{Map, Value, json};

use crate::auth::scopes::Scope;

/// An optional feature a tool depends on.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalFeature {
    /// OCR tools, gated by `[ocr].enabled`.
    Ocr,
}

/// A tool's static definition.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ToolDefinition {
    /// Tool name (MCP `tools/list` name).
    pub name: &'static str,
    /// Scope required to call it.
    pub required_scope: Scope,
    /// Human description.
    pub description: &'static str,
    /// Optional feature gate.
    pub optional_feature: Option<OptionalFeature>,
}

const fn tool(
    name: &'static str,
    required_scope: Scope,
    description: &'static str,
) -> ToolDefinition {
    ToolDefinition {
        name,
        required_scope,
        description,
        optional_feature: None,
    }
}

const fn ocr_tool(name: &'static str, description: &'static str) -> ToolDefinition {
    ToolDefinition {
        name,
        required_scope: Scope::Admin,
        description,
        optional_feature: Some(OptionalFeature::Ocr),
    }
}

/// The 27 always-present tools.
pub static BASE_TOOLS: &[ToolDefinition] = &[
    tool(
        "read_note",
        Scope::VaultRead,
        "Read note content plus parsed frontmatter.",
    ),
    tool(
        "create_note",
        Scope::VaultWrite,
        "Create a note; fail if the path exists.",
    ),
    tool(
        "replace_note",
        Scope::VaultWrite,
        "Replace a full note with optimistic concurrency.",
    ),
    tool(
        "list_folder",
        Scope::VaultRead,
        "List notes under a vault path.",
    ),
    tool("search", Scope::VaultRead, "Search indexed notes."),
    tool(
        "get_backlinks",
        Scope::VaultRead,
        "List notes linking to a path.",
    ),
    tool(
        "get_outgoing_links",
        Scope::VaultRead,
        "List links from a note.",
    ),
    tool(
        "update_frontmatter",
        Scope::VaultWrite,
        "Merge frontmatter keys.",
    ),
    tool(
        "replace_section_by_marker",
        Scope::VaultWrite,
        "Replace an MCP-owned marker section.",
    ),
    tool(
        "list_vault_conflicts",
        Scope::Admin,
        "List active conflict quarantine state.",
    ),
    tool(
        "inbox_capture",
        Scope::VaultCapture,
        "Create or update inbox capture content.",
    ),
    tool(
        "capture_for_date",
        Scope::VaultCapture,
        "Create a capture record for a date.",
    ),
    tool("daily_note_get", Scope::VaultRead, "Read a daily note."),
    tool(
        "daily_note_append",
        Scope::DailyAppend,
        "Append inside a writable daily note marker.",
    ),
    tool(
        "daily_note_repair_markers",
        Scope::Admin,
        "Repair missing daily note markers.",
    ),
    tool(
        "create_record",
        Scope::VaultWrite,
        "Create a type-driven framework record.",
    ),
    tool(
        "find_maps",
        Scope::VaultRead,
        "Find framework map or index notes.",
    ),
    tool(
        "list_record_types",
        Scope::VaultRead,
        "List effective framework record types.",
    ),
    tool(
        "get_vault_structure",
        Scope::VaultRead,
        "Return folder map and framework type list.",
    ),
    tool(
        "link_to_page",
        Scope::VaultRead,
        "Return a stable OCR page wikilink.",
    ),
    tool(
        "list_write_recovery_diagnostics",
        Scope::Admin,
        "List write attempts without terminal audit events.",
    ),
    tool(
        "framework_init",
        Scope::Admin,
        "Create a starter framework schema.",
    ),
    tool(
        "framework_reload",
        Scope::Admin,
        "Reload framework schema files.",
    ),
    tool(
        "framework_register",
        Scope::Admin,
        "Register a framework overlay.",
    ),
    tool(
        "framework_unregister",
        Scope::Admin,
        "Unregister a framework overlay.",
    ),
    tool(
        "framework_list",
        Scope::Admin,
        "List registered framework schemas.",
    ),
    tool(
        "framework_compose",
        Scope::Admin,
        "Return the effective framework schema.",
    ),
];

/// The 3 OCR tools, present only when OCR is enabled.
pub static OCR_TOOLS: &[ToolDefinition] = &[
    ocr_tool("ocr_notebook", "Queue OCR for a notebook."),
    ocr_tool("ocr_status", "Poll OCR job state."),
    ocr_tool("ocr_renumber_notebook", "Force notebook page renumbering."),
];

/// Build the active tool registry.
#[must_use]
pub fn create_tool_registry(ocr_enabled: bool) -> Vec<ToolDefinition> {
    let mut tools: Vec<ToolDefinition> = BASE_TOOLS.to_vec();
    if ocr_enabled {
        tools.extend(OCR_TOOLS.iter().cloned());
    }
    tools
}

/// Filter tools to those whose required scope is granted.
#[must_use]
pub fn list_tools_for_scopes<'a, S: BuildHasher>(
    scopes: &HashSet<Scope, S>,
    tools: &'a [ToolDefinition],
) -> Vec<&'a ToolDefinition> {
    tools
        .iter()
        .filter(|tool| scopes.contains(&tool.required_scope))
        .collect()
}

/// The JSON input schema for a tool, transcribed from the documented surface.
///
/// Tools with no parameters return an empty object schema. This is a flat
/// data-mapping match, so the line count is expected.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn input_schema_for_tool(name: &str) -> Value {
    match name {
        "read_note" | "get_backlinks" | "get_outgoing_links" => object(
            &["path"],
            json!({ "path": string_prop("Vault-relative markdown path.") }),
        ),
        "create_note" => object(
            &["path", "content"],
            json!({
                "path": string_prop("Vault-relative markdown path."),
                "content": string_prop("Full note content."),
                "frontmatter": object_prop("Optional frontmatter fields.")
            }),
        ),
        "replace_note" => object(
            &["path", "content", "base_sha256"],
            json!({
                "path": string_prop("Vault-relative markdown path."),
                "content": string_prop("Replacement note content."),
                "base_sha256": string_prop("Current note SHA-256 for optimistic concurrency."),
                "frontmatter": object_prop("Optional replacement frontmatter fields.")
            }),
        ),
        "list_folder" => object(
            &["path"],
            json!({
                "path": string_prop("Vault-relative folder path. Use an empty string for the vault root."),
                "recursive": bool_prop("Whether to list folders recursively.")
            }),
        ),
        "search" => object(
            &["query"],
            json!({
                "query": string_prop("Search query."),
                "filters": object_prop("Optional search filters, such as a folder prefix.")
            }),
        ),
        "update_frontmatter" => object(
            &["path", "patch", "base_sha256"],
            json!({
                "path": string_prop("Vault-relative markdown path."),
                "patch": object_prop("Frontmatter keys and values to merge."),
                "base_sha256": string_prop("Current note SHA-256 for optimistic concurrency.")
            }),
        ),
        "replace_section_by_marker" => object(
            &["path", "marker_name", "content", "base_sha256"],
            json!({
                "path": string_prop("Vault-relative markdown path."),
                "marker_name": string_prop("MCP marker section name."),
                "content": string_prop("Replacement section content."),
                "base_sha256": string_prop("Current note SHA-256 for optimistic concurrency.")
            }),
        ),
        "daily_note_append" => object(
            &["content", "base_sha256"],
            json!({
                "content": string_prop("Markdown content to append to the daily note section."),
                "base_sha256": string_prop("Current daily note SHA-256 for optimistic concurrency."),
                "date": string_prop("Optional ISO date. Defaults to today."),
                "section": string_prop("Optional configured daily note section name.")
            }),
        ),
        "daily_note_get" => object(
            &[],
            json!({ "date": string_prop("Optional ISO date. Defaults to today.") }),
        ),
        "daily_note_repair_markers" => object(
            &["base_sha256"],
            json!({
                "base_sha256": string_prop("Current daily note SHA-256 for optimistic concurrency."),
                "date": string_prop("Optional ISO date. Defaults to today.")
            }),
        ),
        "capture_for_date" => capture_schema(false),
        "inbox_capture" => capture_schema(true),
        "create_record" => object(
            &["type", "title"],
            json!({
                "type": string_prop("Framework record type, such as capture, map, project, or source."),
                "title": string_prop("Record title."),
                "date": string_prop("Optional ISO date or datetime. Defaults to now."),
                "body": string_prop("Optional note body appended after any configured template."),
                "fields": object_prop("Optional frontmatter fields.")
            }),
        ),
        "find_maps" => object(
            &[],
            json!({ "topic": string_prop("Optional topic query.") }),
        ),
        "link_to_page" => object(
            &["notebook", "page_uuid"],
            json!({
                "notebook": string_prop("Notebook UUID or identifier."),
                "page_uuid": string_prop("Page UUID.")
            }),
        ),
        "framework_init" => object(
            &["framework"],
            json!({
                "framework": enum_prop(&["lyt", "para", "zettel"], "Framework preset to initialize."),
                "output_path": string_prop("Optional vault-relative schema output path."),
                "mode": enum_prop(&["create", "overwrite"], "Optional initialization mode.")
            }),
        ),
        "framework_register" => object(
            &["name", "path"],
            json!({
                "name": string_prop("Overlay name."),
                "path": string_prop("Vault-relative overlay schema path."),
                "priority": int_prop("Optional overlay priority. Lower values load first.")
            }),
        ),
        "framework_unregister" => object(
            &["name"],
            json!({ "name": string_prop("Overlay name to unregister.") }),
        ),
        "ocr_notebook" => object(
            &["identifier"],
            json!({
                "identifier": string_prop("Notebook identifier."),
                "pages": int_array_prop("Optional page numbers to OCR."),
                "force": bool_prop("Whether to force a new OCR job.")
            }),
        ),
        "ocr_status" => object(&["job_id"], json!({ "job_id": string_prop("OCR job id.") })),
        "ocr_renumber_notebook" => object(
            &["notebook_id"],
            json!({ "notebook_id": string_prop("Notebook id to renumber.") }),
        ),
        _ => empty_object_schema(),
    }
}

fn object(required: &[&str], properties: Value) -> Value {
    let mut map = Map::new();
    map.insert("type".to_owned(), json!("object"));
    map.insert("properties".to_owned(), properties);
    map.insert("additionalProperties".to_owned(), json!(false));
    if !required.is_empty() {
        map.insert("required".to_owned(), json!(required));
    }
    Value::Object(map)
}

fn empty_object_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn string_prop(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn bool_prop(description: &str) -> Value {
    json!({ "type": "boolean", "description": description })
}

fn int_prop(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn object_prop(description: &str) -> Value {
    json!({ "type": "object", "description": description })
}

fn int_array_prop(description: &str) -> Value {
    json!({ "type": "array", "items": { "type": "integer" }, "description": description })
}

fn enum_prop(values: &[&str], description: &str) -> Value {
    json!({ "type": "string", "enum": values, "description": description })
}

fn capture_schema(include_strategy: bool) -> Value {
    let mut props = json!({
        "content": string_prop("Markdown content to capture."),
        "source_client": string_prop("Client or integration creating the capture."),
        "date": string_prop("Optional ISO date or datetime. Defaults to now."),
        "source_id": string_prop("Optional stable source identifier for deduplication."),
        "capture_type": string_prop("Optional capture category."),
        "title": string_prop("Optional capture title.")
    });
    if include_strategy && let Some(map) = props.as_object_mut() {
        map.insert(
            "strategy".to_owned(),
            json!({
                "type": "string",
                "enum": ["create", "replace_by_source_id"],
                "description": "Optional inbox capture strategy."
            }),
        );
    }
    object(&["content", "source_client"], props)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_registry_has_27_tools() {
        assert_eq!(create_tool_registry(false).len(), 27);
    }

    #[test]
    fn ocr_tools_added_only_when_enabled() {
        assert_eq!(create_tool_registry(true).len(), 30);
        assert!(
            create_tool_registry(false)
                .iter()
                .all(|t| t.optional_feature.is_none())
        );
    }

    #[test]
    fn read_scope_sees_only_read_tools() {
        let tools = create_tool_registry(false);
        let scopes = HashSet::from([Scope::VaultRead]);
        let visible = list_tools_for_scopes(&scopes, &tools);
        assert!(visible.iter().any(|t| t.name == "read_note"));
        assert!(!visible.iter().any(|t| t.name == "create_note"));
        assert!(visible.iter().all(|t| t.required_scope == Scope::VaultRead));
    }

    #[test]
    fn schema_matches_surface_for_create_note() {
        let schema = input_schema_for_tool("create_note");
        assert_eq!(schema.get("required"), Some(&json!(["path", "content"])));
        assert_eq!(schema.get("additionalProperties"), Some(&json!(false)));
        let content_type = schema
            .get("properties")
            .and_then(|props| props.get("content"))
            .and_then(|content| content.get("type"))
            .and_then(Value::as_str);
        assert_eq!(content_type, Some("string"));
    }

    #[test]
    fn unknown_tool_has_empty_schema() {
        assert_eq!(input_schema_for_tool("nope"), empty_object_schema());
    }
}
