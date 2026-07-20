//! Markdown parsing: frontmatter, title, tags, aliases, source id, and links.

use std::{collections::BTreeMap, sync::LazyLock};

use regex::Regex;
use serde::Serialize;

/// A frontmatter scalar or list value.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum FrontmatterValue {
    /// A string value.
    String(String),
    /// A numeric value.
    Number(f64),
    /// A boolean value.
    Bool(bool),
    /// A list of strings.
    List(Vec<String>),
}

/// The parsed view of a markdown note.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ParsedMarkdown {
    /// Frontmatter key/value pairs (in key order).
    pub frontmatter: BTreeMap<String, FrontmatterValue>,
    /// The note body (frontmatter removed).
    pub body: String,
    /// Title: first `# ` heading, else frontmatter `title`.
    pub title: Option<String>,
    /// Tags from the body and frontmatter (deduped, in first-seen order).
    pub tags: Vec<String>,
    /// Aliases from frontmatter.
    pub aliases: Vec<String>,
    /// Stable source id from frontmatter, when non-empty.
    pub source_id: Option<String>,
    /// Outgoing wikilink targets (deduped).
    pub outgoing_links: Vec<String>,
}

#[allow(clippy::expect_used)] // compile-time-constant, known-valid patterns
static KEY_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z0-9_-]+):(?:\s*(.*))?$").expect("valid regex"));
#[allow(clippy::expect_used)]
static ARRAY_ITEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*-\s+(.+)$").expect("valid regex"));
#[allow(clippy::expect_used)]
static H1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#\s+(.+)$").expect("valid regex"));
#[allow(clippy::expect_used)]
static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)(?:^|\s)#([A-Za-z0-9_/-]+)").expect("valid regex"));
#[allow(clippy::expect_used)]
static WIKILINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]|#]+)(?:[#|][^\]]*)?\]\]").expect("valid regex"));

/// Parse a markdown note. Total: never panics, never errors.
#[must_use]
pub fn parse_markdown(source: &str) -> ParsedMarkdown {
    let normalized = source.replace("\r\n", "\n");
    let (frontmatter_block, body) = split_frontmatter(&normalized);
    let frontmatter = frontmatter_block.map(parse_frontmatter).unwrap_or_default();

    let title = extract_title(body).or_else(|| match frontmatter.get("title") {
        Some(FrontmatterValue::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    });
    let tags = extract_tags(body, &frontmatter);
    let aliases = string_list(frontmatter.get("aliases"));
    let source_id = match frontmatter.get("source_id") {
        Some(FrontmatterValue::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    };
    let outgoing_links = extract_links(&normalized);

    ParsedMarkdown {
        frontmatter,
        body: body.to_owned(),
        title,
        tags,
        aliases,
        source_id,
        outgoing_links,
    }
}

fn split_frontmatter(source: &str) -> (Option<&str>, &str) {
    let Some(rest) = source.strip_prefix("---\n") else {
        return (None, source);
    };
    let Some(end) = rest.find("\n---") else {
        return (None, source);
    };
    let block = rest.get(..end);
    let after = rest.get(end + 4..).unwrap_or("");
    let body = after.strip_prefix('\n').unwrap_or(after);
    (block, body)
}

fn parse_frontmatter(block: &str) -> BTreeMap<String, FrontmatterValue> {
    let mut map = BTreeMap::new();
    let lines: Vec<&str> = block.split('\n').collect();
    let mut cursor = 0;
    while let Some(line) = lines.get(cursor) {
        cursor += 1;
        let Some(captures) = KEY_LINE.captures(line) else {
            continue;
        };
        let Some(key) = captures.get(1) else { continue };
        let raw = captures.get(2).map_or("", |m| m.as_str()).trim();
        let value = if raw.is_empty() {
            let mut items = Vec::new();
            while let Some(next) = lines.get(cursor) {
                let Some(item) = ARRAY_ITEM.captures(next).and_then(|c| c.get(1)) else {
                    break;
                };
                items.push(strip_quotes(item.as_str().trim()).to_owned());
                cursor += 1;
            }
            if items.is_empty() {
                FrontmatterValue::String(String::new())
            } else {
                FrontmatterValue::List(items)
            }
        } else {
            parse_scalar(raw)
        };
        map.insert(key.as_str().to_owned(), value);
    }
    map
}

fn parse_scalar(raw: &str) -> FrontmatterValue {
    if let Some(inner) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let items = inner
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| strip_quotes(item).to_owned())
            .collect();
        return FrontmatterValue::List(items);
    }
    match raw {
        "true" => return FrontmatterValue::Bool(true),
        "false" => return FrontmatterValue::Bool(false),
        _ => {}
    }
    if let Some(number) = parse_number(raw) {
        return FrontmatterValue::Number(number);
    }
    FrontmatterValue::String(strip_quotes(raw).to_owned())
}

fn parse_number(raw: &str) -> Option<f64> {
    let value: f64 = raw.parse().ok()?;
    (value.is_finite() && value.to_string() == raw).then_some(value)
}

fn strip_quotes(value: &str) -> &str {
    let value = value.strip_prefix(['"', '\'']).unwrap_or(value);
    value.strip_suffix(['"', '\'']).unwrap_or(value)
}

fn extract_title(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| H1.captures(line).and_then(|c| c.get(1)))
        .map(|m| m.as_str().trim().to_owned())
}

fn extract_tags(body: &str, frontmatter: &BTreeMap<String, FrontmatterValue>) -> Vec<String> {
    let mut tags = Vec::new();
    for captures in TAG.captures_iter(body) {
        if let Some(tag) = captures.get(1) {
            push_unique(&mut tags, tag.as_str().to_owned());
        }
    }
    for key in ["tags", "tag"] {
        for tag in string_list(frontmatter.get(key)) {
            push_unique(&mut tags, tag);
        }
    }
    tags
}

fn extract_links(source: &str) -> Vec<String> {
    let mut links = Vec::new();
    for captures in WIKILINK.captures_iter(source) {
        if let Some(target) = captures.get(1) {
            push_unique(&mut links, target.as_str().trim().to_owned());
        }
    }
    links
}

fn string_list(value: Option<&FrontmatterValue>) -> Vec<String> {
    match value {
        Some(FrontmatterValue::List(items)) => items.clone(),
        Some(FrontmatterValue::String(item)) if !item.is_empty() => vec![item.clone()],
        _ => Vec::new(),
    }
}

fn push_unique(vec: &mut Vec<String>, item: String) {
    if !vec.contains(&item) {
        vec.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_scalars_arrays_and_body() {
        let src = "---\ntitle: \"Hello\"\ncount: 3\ndone: true\ntags: [a, \"b\"]\naliases:\n  - one\n  - 'two'\n---\n# Heading\n\nBody text.";
        let parsed = parse_markdown(src);
        assert_eq!(
            parsed.frontmatter.get("title"),
            Some(&FrontmatterValue::String("Hello".to_owned()))
        );
        assert_eq!(
            parsed.frontmatter.get("count"),
            Some(&FrontmatterValue::Number(3.0))
        );
        assert_eq!(
            parsed.frontmatter.get("done"),
            Some(&FrontmatterValue::Bool(true))
        );
        assert_eq!(
            parsed.frontmatter.get("tags"),
            Some(&FrontmatterValue::List(vec![
                "a".to_owned(),
                "b".to_owned()
            ]))
        );
        assert_eq!(parsed.aliases, vec!["one".to_owned(), "two".to_owned()]);
        assert_eq!(parsed.body, "# Heading\n\nBody text.");
        assert_eq!(parsed.title.as_deref(), Some("Heading"));
    }

    #[test]
    fn no_frontmatter_without_leading_delimiter() {
        let parsed = parse_markdown("x\n---\ntitle: y\n---\n");
        assert!(parsed.frontmatter.is_empty());
        assert_eq!(parsed.body, "x\n---\ntitle: y\n---\n");
    }

    #[test]
    fn title_falls_back_to_frontmatter() {
        let parsed = parse_markdown("---\ntitle: FromMeta\n---\nno heading here");
        assert_eq!(parsed.title.as_deref(), Some("FromMeta"));
    }

    #[test]
    fn tags_from_body_and_frontmatter_deduped_ordered() {
        let parsed =
            parse_markdown("---\ntags: [alpha, work/x]\n---\n#alpha and #beta and #work/x");
        assert_eq!(
            parsed.tags,
            vec!["alpha".to_owned(), "beta".to_owned(), "work/x".to_owned()]
        );
    }

    #[test]
    fn source_id_and_outgoing_links() {
        let parsed = parse_markdown(
            "---\nsource_id: abc123\ncollection: [\"[[Meetings]]\"]\n---\nSee [[Target#anchor]] and [[Other|label]] and [[Target]].",
        );
        assert_eq!(parsed.source_id.as_deref(), Some("abc123"));
        assert_eq!(
            parsed.outgoing_links,
            vec![
                "Meetings".to_owned(),
                "Target".to_owned(),
                "Other".to_owned()
            ]
        );
    }

    #[test]
    fn crlf_is_normalized() {
        let parsed = parse_markdown("---\r\ntitle: Y\r\n---\r\nbody");
        assert_eq!(parsed.title.as_deref(), Some("Y"));
        assert_eq!(parsed.body, "body");
    }

    #[test]
    fn non_number_strings_stay_strings() {
        let parsed = parse_markdown("---\nv: 007\nw: 1.0\n---\n");
        assert_eq!(
            parsed.frontmatter.get("v"),
            Some(&FrontmatterValue::String("007".to_owned()))
        );
        assert_eq!(
            parsed.frontmatter.get("w"),
            Some(&FrontmatterValue::String("1.0".to_owned()))
        );
    }

    proptest::proptest! {
        #[test]
        fn never_panics(input in ".{0,2048}") {
            let _ = parse_markdown(&input);
        }
    }
}
