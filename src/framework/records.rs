//! Schema-driven records and source-id captures.
use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use time::{Date, Month, OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

use super::{
    Framework, frontmatter, invalid, is_missing, optional_string, read_error, string, write_error,
};
use crate::{runtime::DispatchError, vault::markdown::FrontmatterValue};

impl Framework {
    // NOT cancel-safe: delegates a record creation to the audited writer.
    pub(super) async fn create_record(
        &self,
        args: &Map<String, Value>,
    ) -> Result<Value, DispatchError> {
        let kind = string(args, "type")?;
        let title = string(args, "title")?;
        let date = parse_date(optional_string(args, "date")?)?;
        let body = optional_string(args, "body")?.unwrap_or("");
        let schema = self.compose().await?;
        let definition = schema
            .get("types")
            .and_then(|v| v.get(kind))
            .ok_or_else(|| invalid(format!("Unknown framework record type: {kind}")))?;
        let folder = definition
            .get("folder")
            .and_then(Value::as_str)
            .unwrap_or("");
        let pattern = definition
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or("{title}.md");
        let path = format!(
            "{}/{}",
            folder.strip_suffix('/').unwrap_or(folder),
            expand_pattern(pattern, title, date)
        );
        let template = if let Some(path) = definition.get("template").and_then(Value::as_str) {
            match self.reader.read_note(path).await {
                Ok(note) => note.content,
                Err(e) if is_missing(&e) => String::new(),
                Err(e) => return Err(read_error(e)),
            }
        } else {
            String::new()
        };
        let content = if body.is_empty() {
            template
        } else if template.is_empty() {
            body.to_owned()
        } else {
            format!("{}\n\n{body}", template.trim_end())
        };
        let mut fields = frontmatter(args.get("fields"))?;
        if !fields.contains_key("scheduled")
            && let Some(scheduled) = definition
                .get("frontmatter")
                .and_then(|v| v.get("scheduled"))
        {
            fields.insert(
                "scheduled".into(),
                FrontmatterValue::String(scheduled_date(
                    date,
                    scheduled.get("format").and_then(Value::as_str),
                )),
            );
        }
        if kind == "meeting"
            && let Some(attendees) = fields.get_mut("attendees")
        {
            match attendees {
                FrontmatterValue::String(s) => *s = wiki_link(s),
                FrontmatterValue::List(items) => {
                    for item in items {
                        *item = wiki_link(item);
                    }
                }
                FrontmatterValue::Number(_) | FrontmatterValue::Bool(_) => {}
            }
        }
        let mut metadata = BTreeMap::from_iter([
            ("type".into(), FrontmatterValue::String(kind.into())),
            ("title".into(), FrontmatterValue::String(title.into())),
            ("date".into(), FrontmatterValue::String(date_string(date))),
        ]);
        metadata.extend(fields);
        let result = self
            .writer
            .create_note(&path, &content, Some(&metadata))
            .await
            .map_err(write_error)?;
        serde_json::to_value(result).map_err(|e| invalid(e.to_string()))
    }

    // NOT cancel-safe: creates/replaces a note through the audited writer.
    pub(super) async fn capture(
        &self,
        name: &str,
        args: &Map<String, Value>,
    ) -> Result<Value, DispatchError> {
        let content = string(args, "content")?;
        let client = string(args, "source_client")?;
        let source = optional_string(args, "source_id")?;
        let capture_type = optional_string(args, "capture_type")?;
        let title = optional_string(args, "title")?;
        let raw_date = optional_string(args, "date")?;
        let date = parse_date(raw_date)?;
        let strategy = if name == "inbox_capture" {
            optional_string(args, "strategy")?.unwrap_or("create")
        } else {
            "create"
        };
        if !matches!(strategy, "create" | "replace_by_source_id") {
            return Err(invalid("strategy must be create or replace_by_source_id"));
        }
        let mut fields = Map::from_iter([("source_client".into(), json!(client))]);
        if let Some(source) = source {
            fields.insert("source_id".into(), json!(source));
        }
        if let Some(capture_type) = capture_type {
            fields.insert("capture_type".into(), json!(capture_type));
        }
        // Compose on every invocation, including replacements, matching runtime reference behavior.
        let schema = self.compose().await?;
        if strategy == "replace_by_source_id" {
            let source = source
                .ok_or_else(|| invalid("sourceId is required for replace_by_source_id capture"))?
                .to_owned();
            let index = std::sync::Arc::clone(&self.index);
            let path = tokio::task::spawn_blocking(move || index.find_by_source_id(&source))
                .await
                .map_err(|_| invalid("Index task failed"))?
                .map_err(|_| invalid("Index query failed"))?;
            if let Some(path) = path {
                let current = self.reader.read_note(&path).await.map_err(read_error)?;
                let title = title
                    .or(current.parsed.title.as_deref())
                    .unwrap_or("Capture");
                fields.insert("type".into(), json!("capture"));
                fields.insert("title".into(), json!(title));
                fields.insert("date".into(), json!(date_string(date)));
                let metadata = frontmatter(Some(&Value::Object(fields)))?;
                let result = self
                    .writer
                    .replace_note(&path, content, &current.current_sha256, Some(&metadata))
                    .await
                    .map_err(write_error)?;
                return serde_json::to_value(result).map_err(|e| invalid(e.to_string()));
            }
        }
        if schema.get("types").and_then(|v| v.get("capture")).is_none() {
            return Err(invalid(
                "Effective framework schema does not define a capture type",
            ));
        }
        let input = json!({"type":"capture","title":title.unwrap_or("Capture"),"body":content,"date":date.format(&Rfc3339).map_err(|_|invalid("date must be a valid date"))?,"fields":fields});
        self.create_record(
            input
                .as_object()
                .ok_or_else(|| invalid("Invalid capture"))?,
        )
        .await
    }
}

pub(super) fn parse_date(raw: Option<&str>) -> Result<OffsetDateTime, DispatchError> {
    let Some(raw) = raw else {
        return Ok(OffsetDateTime::now_utc());
    };
    if let Ok(date) = OffsetDateTime::parse(raw, &Rfc3339) {
        return Ok(date.to_offset(UtcOffset::UTC));
    }
    let mut parts = raw.split('-');
    if let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    {
        let date = year
            .parse::<i32>()
            .ok()
            .zip(month.parse::<u8>().ok())
            .zip(day.parse::<u8>().ok())
            .and_then(|((y, m), d)| {
                Month::try_from(m)
                    .ok()
                    .and_then(|m| Date::from_calendar_date(y, m, d).ok())
            });
        if let Some(date) = date {
            return Ok(date.midnight().assume_utc());
        }
    }
    Err(invalid("date must be a valid date"))
}

pub(super) fn date_string(date: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}
fn expand_pattern(pattern: &str, title: &str, date: OffsetDateTime) -> String {
    let title: String = title
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '-' } else { c })
        .collect();
    pattern
        .replace("{title}", title.trim())
        .replace("{date:YYYY}", &format!("{:04}", date.year()))
        .replace(
            "{date:YYYY-MM}",
            &format!("{:04}-{:02}", date.year(), u8::from(date.month())),
        )
        .replace("{date:YYYY-MM-DD}", &date_string(date))
        .replace(
            "{date:YYYY-MM-DD HH-mm}",
            &format!(
                "{} {:02}-{:02}",
                date_string(date),
                date.hour(),
                date.minute()
            ),
        )
        .replace(
            "{quarter}",
            &((u8::from(date.month()) - 1) / 3 + 1).to_string(),
        )
}
fn wiki_link(value: &str) -> String {
    let value = value.trim();
    if value.starts_with("[[") && value.ends_with("]]") {
        value.into()
    } else {
        format!("[[{value}]]")
    }
}
fn scheduled_date(date: OffsetDateTime, format: Option<&str>) -> String {
    let hour = match date.hour() % 12 {
        0 => 12,
        h => h,
    };
    let suffix = if date.hour() >= 12 { "PM" } else { "AM" };
    let hour = if format == Some("YYYY-MM-DD H:mm a") {
        hour.to_string()
    } else {
        format!("{hour:02}")
    };
    format!("{} {hour}:{:02} {suffix}", date_string(date), date.minute())
}
