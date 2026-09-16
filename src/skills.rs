//! Vault skill discovery and validation for MCP prompts.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::Serialize;

use crate::vault::{
    markdown::{FrontmatterValue, parse_markdown},
    path::{is_markdown_path, normalize_vault_path},
    reader::VaultReader,
};

/// A validated skill exposed only through the separately scoped prompt surface.
#[derive(Debug, Clone, Serialize)]
pub struct LoadedSkill {
    /// Slug used as prompt name.
    pub name: String,
    /// Human description.
    pub description: String,
    /// Vault-relative source path.
    pub path: String,
    /// Trimmed markdown body.
    pub content: String,
}

/// A load diagnostic, including invalid and missing candidates for privacy.
#[derive(Debug, Clone, Serialize)]
pub struct SkillStatus {
    /// Vault-relative source path.
    pub path: String,
    /// `loaded` or `error`.
    pub status: &'static str,
    /// Name when successfully loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Description when successfully loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Sanitized diagnostic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Complete skill snapshot, swapped atomically on reload.
#[derive(Debug, Clone, Default)]
pub struct SkillLoad {
    /// Valid skills, sorted and deduplicated by slug (last path wins).
    pub skills: Vec<LoadedSkill>,
    /// Diagnostics for candidates and unreadable maps.
    pub statuses: Vec<SkillStatus>,
}

/// Load configured maps, then their directly linked skill notes.
// cancel-safe: only reads files and builds an unpublished local snapshot.
pub async fn load_skills(reader: &VaultReader, maps: &[String]) -> SkillLoad {
    let mut load = SkillLoad::default();
    let mut paths = BTreeSet::new();
    for map in maps {
        match reader.read_note(map).await {
            Ok(note) => {
                for candidate in extract_links(&note.content, map) {
                    match candidate {
                        Ok(path) => {
                            paths.insert(path);
                        }
                        Err(path) => load
                            .statuses
                            .push(error_status(path, "Vault path is invalid")),
                    }
                }
            }
            Err(_) => load
                .statuses
                .push(error_status(map.clone(), "Vault path could not be read")),
        }
    }
    let mut by_name = BTreeMap::new();
    for path in paths {
        if maps.contains(&path) {
            continue;
        }
        let skill = match reader.read_note(&path).await {
            Ok(note) => parse_skill(&path, &note.content),
            Err(_) => Err("Vault path could not be read"),
        };
        match skill {
            Ok(skill) => {
                load.statuses.push(SkillStatus {
                    path,
                    status: "loaded",
                    name: Some(skill.name.clone()),
                    description: Some(skill.description.clone()),
                    error: None,
                });
                by_name.insert(skill.name.clone(), skill);
            }
            Err(message) => load.statuses.push(error_status(path, message)),
        }
    }
    load.skills = by_name.into_values().collect();
    load
}

fn error_status(path: String, error: &str) -> SkillStatus {
    SkillStatus {
        path,
        status: "error",
        name: None,
        description: None,
        error: Some(error.to_owned()),
    }
}

fn parse_skill(path: &str, source: &str) -> Result<LoadedSkill, &'static str> {
    let parsed = parse_markdown(source);
    let field = |key| match parsed.frontmatter.get(key) {
        Some(FrontmatterValue::String(value))
            if !value.trim().is_empty() && !matches!(value.trim(), "|" | ">") =>
        {
            Some(value.trim().to_owned())
        }
        _ => None,
    };
    let name = field("name").ok_or("skill frontmatter name must be a slug")?;
    if name.len() > 64
        || !name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err("skill frontmatter name must be a slug");
    }
    let description =
        field("description").ok_or("skill frontmatter description must be a non-empty string")?;
    let content = parsed.body.trim().to_owned();
    if content.is_empty() {
        return Err("skill body must not be empty");
    }
    Ok(LoadedSkill {
        name,
        description,
        path: path.to_owned(),
        content,
    })
}

fn extract_links(source: &str, map: &str) -> Vec<Result<String, String>> {
    let mut paths = Vec::new();
    // These constant patterns cannot fail, but avoid a panicking initializer.
    if let Ok(wiki) = regex::Regex::new(r"\[\[([^\]|#]+)(?:[#|][^\]]*)?\]\]") {
        for capture in wiki.captures_iter(source) {
            if let Some(target) = capture.get(1) {
                paths.push(normalize_candidate(target.as_str().trim()));
            }
        }
    }
    if let Ok(hint) = regex::Regex::new(r"(?im)(?:^|\s)(?:\*\*)?Path(?:\*\*)?\s*:\s*(.+)$") {
        for capture in hint.captures_iter(source) {
            if let Some(value) = capture
                .get(1)
                .and_then(|v| v.as_str().split_whitespace().next())
            {
                let value = value
                    .trim_matches(['\'', '"', '`'])
                    .trim_start_matches('/')
                    .trim_start_matches("[[")
                    .trim_end_matches("]]");
                if !value.is_empty() {
                    paths.push(normalize_candidate(value));
                }
            }
        }
    }
    if let Ok(markdown) = regex::Regex::new(r"\[[^\]]+\]\(([^)]+)\)") {
        let parent = Path::new(map).parent().and_then(Path::to_str).unwrap_or("");
        for capture in markdown.captures_iter(source) {
            let Some(target) = capture.get(1).map(|v| v.as_str().trim()) else {
                continue;
            };
            let target = target.split('#').next().unwrap_or("");
            if target.starts_with('/') || target.contains(':') || !is_markdown_path(target) {
                continue;
            }
            let mut components: Vec<&str> = parent.split('/').filter(|v| !v.is_empty()).collect();
            let mut escaped = false;
            for part in target.split('/') {
                match part {
                    ".." => {
                        if components.pop().is_none() {
                            escaped = true;
                        }
                    }
                    "" | "." => {}
                    other => components.push(other),
                }
            }
            if escaped {
                paths.push(Err(target.to_owned()));
            } else {
                paths.push(normalize_candidate(&components.join("/")));
            }
        }
    }
    paths
}

fn normalize_candidate(path: &str) -> Result<String, String> {
    let path = if is_markdown_path(path) {
        path.to_owned()
    } else {
        format!("{path}.md")
    };
    normalize_vault_path(&path).map_err(|_| "<invalid path>".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::reader::VaultReaderOptions;

    #[tokio::test]
    async fn links_validation_duplicates_and_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::create_dir_all(dir.path().join("Maps"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(dir.path().join("Skills"))
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("Maps/map.md"), "[[Skills/a#part|label]]\n[duplicate](../Skills/a.md#part)\n**Path**: `/Skills/b`\n[[Skills/invalid]]\n[[missing]]\n[[Maps/map]]\n[external](https://evil.example/skill.md)\n[escape](../../outside.md)").await.unwrap();
        tokio::fs::write(
            dir.path().join("Skills/a.md"),
            "---\nname: same\ndescription: First\n---\nFirst.",
        )
        .await
        .unwrap();
        tokio::fs::write(
            dir.path().join("Skills/b.md"),
            "---\nname: same\ndescription: Second\n---\nSecond.",
        )
        .await
        .unwrap();
        tokio::fs::write(
            dir.path().join("Skills/invalid.md"),
            "---\nname: BAD\ndescription: Bad\n---\nSecret.",
        )
        .await
        .unwrap();
        let reader = VaultReader::new(VaultReaderOptions {
            vault_root: dir.path().to_path_buf(),
            ignored_globs: Vec::new(),
            blocked_paths: Vec::new(),
        });
        let load = load_skills(
            &reader,
            &["Maps/map.md".to_owned(), "MissingMap.md".to_owned()],
        )
        .await;
        assert_eq!(load.skills.len(), 1);
        assert_eq!(load.skills.first().unwrap().description, "Second");
        assert_eq!(
            load.statuses
                .iter()
                .filter(|s| s.status == "loaded")
                .count(),
            2
        );
        assert!(
            load.statuses
                .iter()
                .any(|s| s.path == "Skills/invalid.md" && s.status == "error")
        );
        assert!(!load.statuses.iter().any(|s| s.path == "Maps/map.md"));
        assert!(
            !serde_json::to_string(&load.statuses)
                .unwrap()
                .contains(&dir.path().display().to_string())
        );
    }

    #[test]
    fn rejects_missing_fields_bad_slug_and_empty_body() {
        for source in [
            "body",
            "---\nname: -bad\ndescription: yes\n---\nbody",
            "---\nname: valid\ndescription: |\n---\nbody",
            "---\nname: valid\ndescription: yes\n---\n   ",
        ] {
            assert!(parse_skill("s.md", source).is_err());
        }
    }
}
