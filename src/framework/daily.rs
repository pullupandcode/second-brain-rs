//! Daily notes and protected marker sections.
use serde_json::{Map, Value};

use super::{
    Framework, invalid, is_missing, optional_string, read_error,
    records::{date_string, parse_date},
    string, write_error,
};
use crate::runtime::DispatchError;

const TEMPLATE: &str = "x/Templates/Daily Template.md";
const SECTIONS: [(&str, &str, bool); 3] = [
    ("agenda", "agenda", false),
    ("daily-log", "daily-captures", true),
    ("last-light", "last-light-summary", true),
];

impl Framework {
    // NOT cancel-safe: may create or mutate a daily note through the audited writer.
    pub(super) async fn daily(
        &self,
        name: &str,
        args: &Map<String, Value>,
    ) -> Result<Value, DispatchError> {
        let date = parse_date(optional_string(args, "date")?)?;
        let path = format!("Calendar/Days/{}.md", date_string(date));
        match name {
            "daily_note_get" => {
                match self.reader.read_note(&path).await {
                    Ok(note) => {
                        return serde_json::to_value(note).map_err(|e| invalid(e.to_string()));
                    }
                    Err(error) if is_missing(&error) => {}
                    Err(error) => return Err(read_error(error)),
                }
                let template = self
                    .reader
                    .read_note(TEMPLATE)
                    .await
                    .map_err(read_error)?
                    .content;
                self.writer
                    .create_note(&path, &template, None)
                    .await
                    .map_err(write_error)?;
            }
            "daily_note_append" => {
                let content = string(args, "content")?;
                let base = string(args, "base_sha256")?;
                let section = optional_string(args, "section")?.unwrap_or("daily-log");
                let note = self.reader.read_note(&path).await.map_err(read_error)?;
                let (_, marker, writable) = SECTIONS
                    .iter()
                    .find(|(name, _, _)| *name == section)
                    .ok_or_else(|| {
                    invalid(format!("Daily note section is not configured: {section}"))
                })?;
                if !writable {
                    return Err(invalid(format!(
                        "Daily note section is not writable: {section}"
                    )));
                }
                let (start, end) = marker_range(&note.content, marker)
                    .ok_or_else(|| invalid(format!("Markers missing for section: {marker}")))?;
                let existing = note.content.get(start..end).unwrap_or("").trim();
                let content = if existing.is_empty() {
                    content.to_owned()
                } else {
                    format!("{}\n{content}", existing.trim_end())
                };
                self.writer
                    .replace_section_by_marker(&path, marker, &content, base)
                    .await
                    .map_err(write_error)?;
            }
            "daily_note_repair_markers" => {
                let base = string(args, "base_sha256")?;
                let note = self.reader.read_note(&path).await.map_err(read_error)?;
                let template = self
                    .reader
                    .read_note(TEMPLATE)
                    .await
                    .map_err(read_error)?
                    .content;
                let mut missing = Vec::new();
                for (_, marker, _) in SECTIONS {
                    let start = format!("<!-- mcp:section {marker} start -->");
                    let end = format!("<!-- mcp:section {marker} end -->");
                    if note.content.contains(&start) && note.content.contains(&end) {
                        continue;
                    }
                    if let Some((from, to)) = marker_range(&template, marker)
                        && let Some(block) = template.get(from - start.len()..to + end.len())
                    {
                        missing.push(block);
                    }
                }
                if missing.is_empty() {
                    return serde_json::to_value(note).map_err(|e| invalid(e.to_string()));
                }
                let repaired = format!("{}\n\n{}\n", note.content.trim_end(), missing.join("\n\n"));
                self.writer
                    .replace_note(&path, &repaired, base, None)
                    .await
                    .map_err(write_error)?;
            }
            _ => return Err(DispatchError::UnknownTool(name.into())),
        }
        serde_json::to_value(self.reader.read_note(&path).await.map_err(read_error)?)
            .map_err(|e| invalid(e.to_string()))
    }
}
fn marker_range(content: &str, name: &str) -> Option<(usize, usize)> {
    let start_marker = format!("<!-- mcp:section {name} start -->");
    let end_marker = format!("<!-- mcp:section {name} end -->");
    let start = content.find(&start_marker)?;
    let end = content.find(&end_marker)?;
    (end >= start + start_marker.len()).then_some((start + start_marker.len(), end))
}
