//! Reference-compatible framework schema parser and composer.

use std::fmt::Write;

use serde_json::{Map, Value, json};

pub(super) fn parse_schema(source: &str) -> Result<Value, String> {
    if source
        .lines()
        .any(|line| line.len() - line.trim_start_matches(' ').len() > 256)
    {
        return Err("schema nesting is too deep".into());
    }
    let lines: Vec<_> = source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, raw) = trimmed.split_once(':')?;
            if key.is_empty()
                || !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return None;
            }
            Some((
                line.len() - line.trim_start_matches(' ').len(),
                key,
                raw.trim(),
            ))
        })
        .collect();
    let raw = parse_mapping(&lines, &mut 0, None);
    if raw.get("version") != Some(&json!(1)) {
        return Err("version must be 1".into());
    }
    let kind = raw.get("schema_kind").and_then(Value::as_str).unwrap_or("");
    if !matches!(kind, "base" | "overlay") {
        return Err("schema_kind must be base or overlay".into());
    }
    if let Some(framework) = raw.get("framework")
        && !matches!(
            framework.as_str(),
            Some("lyt" | "para" | "zettel" | "custom")
        )
    {
        return Err("framework must be one of: lyt, para, zettel, custom".into());
    }
    let mut result = Map::from_iter([
        ("version".into(), json!(1)),
        ("schemaKind".into(), json!(kind)),
        (
            "override".into(),
            json!(raw.get("override") == Some(&Value::Bool(true))),
        ),
    ]);
    copy_strings(
        &raw,
        &mut result,
        &["framework", "name", "description", "extends"],
        "",
    )?;
    if let Some(inbox) = raw.get("inbox") {
        let inbox = object(inbox, "inbox")?;
        let mut normalized = Map::new();
        copy_strings(inbox, &mut normalized, &["folder"], "inbox.")?;
        result.insert("inbox".into(), normalized.into());
    }
    result.insert("types".into(), parse_types(&raw)?.into());
    Ok(result.into())
}

fn parse_types(raw: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let raw_types = raw
        .get("types")
        .and_then(Value::as_object)
        .ok_or("types must be an object")?;
    let mut types = Map::new();
    for (name, raw_type) in raw_types {
        let prefix = format!("types.{name}");
        let raw_type = object(raw_type, &prefix)?;
        let folder = raw_type
            .get("folder")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("{prefix}.folder must be a non-empty string"))?;
        let mut definition = Map::from_iter([("folder".into(), json!(folder))]);
        copy_strings(
            raw_type,
            &mut definition,
            &["description", "filename", "template"],
            &format!("{prefix}."),
        )?;
        if let Some(fields) = raw_type.get("frontmatter") {
            let mut normalized = Map::new();
            for (field, declaration) in object(fields, &format!("{prefix}.frontmatter"))? {
                let field_prefix = format!("{prefix}.frontmatter.{field}.");
                let value = if let Some(declaration) = declaration.as_object() {
                    let mut decl = Map::new();
                    copy_strings(declaration, &mut decl, &["type", "format"], &field_prefix)?;
                    if let Some(required) = declaration.get("required") {
                        if !required.is_boolean() {
                            return Err(format!("{field_prefix}required must be a boolean"));
                        }
                        decl.insert("required".into(), required.clone());
                    }
                    Value::Object(decl)
                } else {
                    json!({"defaultValue":declaration})
                };
                normalized.insert(field.clone(), value);
            }
            definition.insert("frontmatter".into(), normalized.into());
        }
        types.insert(name.clone(), definition.into());
    }
    Ok(types)
}

fn copy_strings(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    keys: &[&str],
    prefix: &str,
) -> Result<(), String> {
    for key in keys {
        if let Some(value) = source.get(*key) {
            if !value.is_string() {
                return Err(format!("{prefix}{key} must be a string"));
            }
            target.insert((*key).into(), value.clone());
        }
    }
    Ok(())
}

fn object<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{name} must be an object"))
}

fn parse_mapping(
    lines: &[(usize, &str, &str)],
    cursor: &mut usize,
    parent: Option<usize>,
) -> Map<String, Value> {
    let mut result = Map::new();
    while let Some(&(indent, key, raw)) = lines.get(*cursor) {
        if parent.is_some_and(|p| indent <= p) {
            break;
        }
        *cursor += 1;
        let value = if raw.is_empty() {
            parse_mapping(lines, cursor, Some(indent)).into()
        } else {
            let raw = raw
                .char_indices()
                .find(|(i, character)| {
                    *character == '#'
                        && raw
                            .get(..*i)
                            .is_some_and(|s| s.ends_with(char::is_whitespace))
                })
                .map_or(raw, |(i, _)| raw.get(..i).unwrap_or(raw));
            parse_scalar(raw.trim())
        };
        result.insert(key.into(), value);
    }
    result
}

fn unquote(raw: &str) -> &str {
    raw.strip_prefix(['\'', '"'])
        .unwrap_or(raw)
        .strip_suffix(['\'', '"'])
        .unwrap_or_else(|| raw.strip_prefix(['\'', '"']).unwrap_or(raw))
}

fn parse_scalar(raw: &str) -> Value {
    match raw {
        "true" => return json!(true),
        "false" => return json!(false),
        _ => {}
    }
    if let Some(body) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return Value::Array(if body.trim().is_empty() {
            vec![]
        } else {
            body.split(',').map(|s| json!(unquote(s.trim()))).collect()
        });
    }
    if let Some(body) = raw.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        return Value::Object(
            body.split(',')
                .filter_map(|s| s.split_once(':'))
                .map(|(k, v)| (k.trim().into(), parse_scalar(v.trim())))
                .collect(),
        );
    }
    if let Ok(number) = raw.parse::<f64>()
        && number.is_finite()
        && number.to_string() == raw
    {
        return serde_json::from_str(raw).unwrap_or_else(|_| json!(raw));
    }
    json!(unquote(raw))
}

pub(super) fn compose_schema(base: Value, overlays: Vec<Value>) -> Result<Value, String> {
    let Value::Object(mut base) = base else {
        return Err("base schema must be an object".into());
    };
    if base.get("schemaKind").and_then(Value::as_str) != Some("base") {
        return Err("base schema must have schema_kind: base".into());
    }
    let framework = base
        .get("framework")
        .and_then(Value::as_str)
        .unwrap_or("custom")
        .to_owned();
    base.insert("framework".into(), json!(framework));
    if let Some(preset) = preset(&framework) {
        base.insert("preset".into(), preset);
    }
    let inbox = base.get("inbox").and_then(|v| v.get("folder")).cloned();
    let types = base
        .get_mut("types")
        .and_then(Value::as_object_mut)
        .ok_or("types must be an object")?;
    for overlay in overlays {
        if overlay.get("schemaKind").and_then(Value::as_str) != Some("overlay") {
            return Err("overlay schema must have schema_kind: overlay".into());
        }
        if let Some(folder) = overlay.get("inbox").and_then(|v| v.get("folder"))
            && Some(folder) != inbox.as_ref()
        {
            return Err("overlay cannot change inbox.folder".into());
        }
        for (name, definition) in overlay
            .get("types")
            .and_then(Value::as_object)
            .ok_or("types must be an object")?
        {
            if types.contains_key(name) && overlay.get("override") != Some(&Value::Bool(true)) {
                return Err(format!("framework type already exists: {name}"));
            }
            types.insert(name.clone(), definition.clone());
        }
    }
    Ok(base.into())
}

pub(super) fn preset(id: &str) -> Option<Value> {
    let (name, description, types) = match id {
        "lyt" => (
            "Linking Your Thinking",
            "Ideaverse-style maps, sources, dots, efforts, records, and daily notes.",
            vec![
                (
                    "meeting",
                    "A time-based meeting record.",
                    "Calendar/Records/Meetings",
                ),
                (
                    "capture",
                    "An agent or inbox capture record.",
                    "Calendar/Records/Captures",
                ),
                ("person", "A person or contact note.", "Atlas/Dots/People"),
                (
                    "project",
                    "An active effort or project.",
                    "Efforts/Projects/Active",
                ),
                ("map", "A map of content or collection hub.", "Atlas/Maps"),
            ],
        ),
        "para" => (
            "PARA",
            "Projects, Areas, Resources, and Archives.",
            vec![
                ("project", "A finite outcome with active work.", "Projects"),
                (
                    "area",
                    "A long-running responsibility or standard.",
                    "Areas",
                ),
                (
                    "resource",
                    "Reference material grouped by topic.",
                    "Resources",
                ),
                (
                    "archive",
                    "Inactive material retained for reference.",
                    "Archives",
                ),
            ],
        ),
        "zettel" => (
            "Zettelkasten",
            "Fleeting, literature, and permanent notes.",
            vec![
                (
                    "fleeting_note",
                    "A quick temporary thought or capture.",
                    "Fleeting",
                ),
                (
                    "literature_note",
                    "A source-grounded note from reading or research.",
                    "Literature",
                ),
                (
                    "permanent_note",
                    "An atomic durable knowledge note.",
                    "Permanent",
                ),
            ],
        ),
        _ => return None,
    };
    Some(
        json!({"id":id,"name":name,"description":description,"types":types.into_iter().map(|(name,description,folder)|json!({"name":name,"description":description,"defaultFolder":folder})).collect::<Vec<_>>() }),
    )
}

pub(super) fn materialize_preset(id: &str) -> Result<String, String> {
    let preset = preset(id).ok_or_else(|| format!("Unknown framework preset: {id}"))?;
    let name = preset.get("name").and_then(Value::as_str).unwrap_or("");
    let mut source = format!(
        "version: 1\nschema_kind: base\nframework: {id}\ndescription: \"{name} starter schema\"\n\ntypes:\n"
    );
    if let Some(types) = preset.get("types").and_then(Value::as_array) {
        for definition in types {
            let text = |k| definition.get(k).and_then(Value::as_str).unwrap_or("");
            write!(
                source,
                "  {}:\n    description: \"{}\"\n    folder: {}\n    filename: \"{{title}}.md\"\n",
                text("name"),
                text("description"),
                text("defaultFolder")
            )
            .map_err(|error| error.to_string())?;
        }
    }
    Ok(source)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn inline_comment_after_literal_hash_matches_reference_subset() {
        let schema = parse_schema(
            "version: 1\nschema_kind: base\ndescription: literal#hash # comment\ntypes: {}\n",
        )
        .unwrap();
        assert_eq!(schema["description"], "literal#hash");
    }

    #[test]
    fn schema_fields_defaults_and_declarations() {
        let schema = parse_schema("version: 1\nschema_kind: base\nframework: custom\ntypes:\n  meeting:\n    folder: Meetings\n    frontmatter:\n      collection: [\"[[Meetings]]\"]\n      scheduled: { required: true, format: \"YYYY-MM-DD hh:mm a\" }\n      attendees: { required: false, type: linklist }\n").unwrap();
        assert_eq!(
            schema["types"]["meeting"]["frontmatter"]["collection"],
            json!({"defaultValue":["[[Meetings]]"]})
        );
        assert_eq!(
            schema["types"]["meeting"]["frontmatter"]["scheduled"],
            json!({"required":true,"format":"YYYY-MM-DD hh:mm a"})
        );
    }

    #[test]
    fn composition_preserves_identity_and_requires_explicit_override() {
        let base = parse_schema("version: 1\nschema_kind: base\nframework: para\ninbox:\n  folder: +\ntypes:\n  project:\n    folder: Projects\n").unwrap();
        let mut overlay = parse_schema(
            "version: 1\nschema_kind: overlay\ntypes:\n  project:\n    folder: Work\n",
        )
        .unwrap();
        assert!(compose_schema(base.clone(), vec![overlay.clone()]).is_err());
        overlay["override"] = json!(true);
        let effective = compose_schema(base.clone(), vec![overlay.clone()]).unwrap();
        assert_eq!(effective["types"]["project"]["folder"], "Work");
        assert_eq!(effective["preset"]["name"], "PARA");
        overlay["inbox"] = json!({"folder":"Inbox"});
        assert!(compose_schema(base, vec![overlay]).is_err());
    }

    #[test]
    fn schema_validation_rejects_invalid_shapes() {
        for source in [
            "version: 2\nschema_kind: base\ntypes: {}",
            "version: 1\nschema_kind: wrong\ntypes: {}",
            "version: 1\nschema_kind: base\nframework: wrong\ntypes: {}",
            "version: 1\nschema_kind: base\ntypes:\n  note:\n    folder: 5",
        ] {
            assert!(parse_schema(source).is_err(), "{source}");
        }
    }

    #[test]
    fn presets_materialize_all_types() {
        for (id, count) in [("lyt", 5), ("para", 4), ("zettel", 3)] {
            let schema = parse_schema(&materialize_preset(id).unwrap()).unwrap();
            assert_eq!(schema["types"].as_object().unwrap().len(), count);
            assert_eq!(schema["framework"], id);
        }
        assert!(materialize_preset("unknown").is_err());
    }
}
