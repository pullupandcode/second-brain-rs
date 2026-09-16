//! Schema-driven records and source-id captures.
#![allow(clippy::literal_string_with_formatting_args)] // Framework filename tokens are literals.
use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use time::OffsetDateTime;

pub(super) use super::dates::parse_date;
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
        let input = json!({"type":"capture","title":title.unwrap_or("Capture"),"body":content,"date":iso_string(date),"fields":fields});
        self.create_record(
            input
                .as_object()
                .ok_or_else(|| invalid("Invalid capture"))?,
        )
        .await
    }
}

pub(super) fn date_string(date: OffsetDateTime) -> String {
    format!(
        "{}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}
fn iso_string(date: OffsetDateTime) -> String {
    let year = if date.year() < 0 {
        format!("{:07}", date.year())
    } else {
        format!("{:04}", date.year())
    };
    format!(
        "{year}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        u8::from(date.month()),
        date.day(),
        date.hour(),
        date.minute(),
        date.second(),
        date.millisecond()
    )
}
fn expand_pattern(pattern: &str, title: &str, date: OffsetDateTime) -> String {
    let title: String = title
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '-' } else { c })
        .collect();
    pattern
        .replace("{title}", title.trim())
        .replace("{date:YYYY}", &date.year().to_string())
        .replace(
            "{date:YYYY-MM}",
            &format!("{}-{:02}", date.year(), u8::from(date.month())),
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

#[cfg(test)]
mod tests {
    use time::format_description::well_known::Rfc3339;

    use super::*;

    #[test]
    fn utc_date_and_scheduled_formats_are_consistent() {
        let date = parse_date(Some("2026-05-07T23:30:00-03:00")).unwrap();
        assert_eq!(date_string(date), "2026-05-08");
        assert_eq!(
            scheduled_date(date, Some("YYYY-MM-DD H:mm a")),
            "2026-05-08 2:30 AM"
        );
        assert_eq!(scheduled_date(date, None), "2026-05-08 02:30 AM");
        assert_eq!(
            scheduled_date(parse_date(Some("2026-01-01")).unwrap(), None),
            "2026-01-01 12:00 AM"
        );
        assert_eq!(
            expand_pattern("{title}", " A/B:C*D?E\"F<G>H|I\\J ", date),
            "A-B-C-D-E-F-G-H-I-J"
        );
    }

    #[test]
    fn iso_date_overflow_matches_javascript_date_normalization() {
        assert_eq!(
            date_string(parse_date(Some("2026-02-30")).unwrap()),
            "2026-03-02"
        );
        assert!(parse_date(Some("2026-02-32")).is_err());
        assert!(parse_date(Some("2026-13-01")).is_err());
    }
    #[test]
    fn iso_forms_and_time_overflow_follow_javascript() {
        for (input, expected) in [
            ("2026", "2026-01-01T00:00:00Z"),
            ("2026-09", "2026-09-01T00:00:00Z"),
            ("2026-02-30T00:00:00Z", "2026-03-02T00:00:00Z"),
            ("2026-02-30T23:30-03:00", "2026-03-03T02:30:00Z"),
            ("2026-01-01T24:00Z", "2026-01-02T00:00:00Z"),
            ("2026-09T14:30Z", "2026-09-01T14:30:00Z"),
            ("2026-01-01T12:30:00.1234Z", "2026-01-01T12:30:00.123Z"),
            ("2026-01-01T12:30+0100", "2026-01-01T11:30:00Z"),
        ] {
            assert_eq!(
                parse_date(Some(input)).unwrap().format(&Rfc3339).unwrap(),
                expected,
                "{input}"
            );
        }
        for input in [
            "2026-02-32T00:00Z",
            "2026-13-01T00:00Z",
            "2026-01-01T24:01Z",
            "2026-01-01T24:00:00.0001Z",
            "9999-12-31T23:00-23:00",
            "2026-01-01T23:59:60Z",
            "2026-01-01T12:00+24:00",
        ] {
            assert!(parse_date(Some(input)).is_err(), "{input}");
        }
    }

    #[test]
    fn early_and_negative_year_fields_match_reference_without_padding() {
        for (raw, expected, iso) in [
            ("0001-01-02", "1-01-02", "0001-01-02T00:00:00.000Z"),
            ("-000001-01-02", "-1-01-02", "-000001-01-02T00:00:00.000Z"),
        ] {
            let date = parse_date(Some(raw)).unwrap();
            assert_eq!(date_string(date), expected);
            assert_eq!(iso_string(date), iso);
            assert_eq!(parse_date(Some(&iso_string(date))).unwrap(), date);
        }
    }

    #[test]
    fn timezone_less_datetime_uses_system_timezone() {
        let expected = jiff::civil::DateTime::new(2026, 9, 1, 14, 30, 0, 0)
            .unwrap()
            .to_zoned(jiff::tz::TimeZone::system())
            .unwrap()
            .timestamp()
            .as_nanosecond();
        assert_eq!(
            parse_date(Some("2026-09-01T14:30:00"))
                .unwrap()
                .unix_timestamp_nanos(),
            expected
        );
    }
}
