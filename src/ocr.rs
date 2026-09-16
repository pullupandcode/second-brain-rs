//! Optional in-memory OCR job queue. Matches the reference contract; no OCR worker.

use std::{collections::HashMap, sync::Mutex};

use serde_json::{Map, Value, json};

use crate::runtime::DispatchError;

/// Queued OCR jobs, lost when the server restarts as in the reference.
#[derive(Debug, Default)]
pub struct OcrQueue {
    jobs: Mutex<HashMap<String, Value>>,
}

impl OcrQueue {
    /// Validate and execute one of the three OCR contract tools.
    /// # Errors
    /// Returns invalid arguments or `job_missing` for unknown jobs.
    pub fn dispatch(&self, name: &str, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if name == "ocr_status" {
            let id = required(args, "job_id")?;
            return jobs.get(id).cloned().ok_or_else(|| DispatchError::Coded {
                code: "job_missing",
                message: format!("Unknown OCR job: {id}"),
            });
        }
        let (kind, input) = if name == "ocr_notebook" {
            let mut input = Map::from_iter([(
                "identifier".to_owned(),
                json!(required(args, "identifier")?),
            )]);
            if let Some(pages) = args.get("pages") {
                if !pages.as_array().is_some_and(|values| {
                    values.iter().all(|value| {
                        value
                            .as_f64()
                            .is_some_and(|number| number.is_finite() && number.fract() == 0.0)
                    })
                }) {
                    return Err(DispatchError::Invalid(
                        "pages must be an array of integers".to_owned(),
                    ));
                }
                input.insert("pages".to_owned(), pages.clone());
            }
            if let Some(force) = args.get("force") {
                if !force.is_boolean() {
                    return Err(DispatchError::Invalid("force must be a boolean".to_owned()));
                }
                input.insert("force".to_owned(), force.clone());
            }
            ("notebook", Value::Object(input))
        } else if name == "ocr_renumber_notebook" {
            (
                "renumber",
                json!({"notebook_id": required(args, "notebook_id")?}),
            )
        } else {
            return Err(DispatchError::UnknownTool(name.to_owned()));
        };
        let id = uuid::Uuid::new_v4().to_string();
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| DispatchError::Internal(e.to_string()))?;
        jobs.insert(id.clone(), json!({"id": id, "type": kind, "state":"queued", "queuedAt":now, "updatedAt":now, "input":input}));
        drop(jobs);
        Ok(json!({"job_id":id,"state":"queued","type":kind,"queued_at":now}))
    }
}

fn required<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, DispatchError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| DispatchError::Invalid(format!("{key} must be a non-empty string")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_accept_equivalent_integer_number_spellings() {
        let queue = OcrQueue::default();
        let args: Map<String, Value> =
            serde_json::from_str(r#"{"identifier":"n","pages":[1.0,2e0,-3.0]}"#).unwrap();
        let job = queue.dispatch("ocr_notebook", &args).unwrap();
        let status = queue
            .dispatch(
                "ocr_status",
                &Map::from_iter([("job_id".to_owned(), job.get("job_id").unwrap().clone())]),
            )
            .unwrap();
        let pages = status.pointer("/input/pages").unwrap().as_array().unwrap();
        assert_eq!(
            pages
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect::<Vec<_>>(),
            vec![1.0, 2.0, -3.0]
        );
        for pages in [json!([1.5]), json!([true]), json!(["1"]), json!([null])] {
            let args = Map::from_iter([
                ("identifier".to_owned(), json!("n")),
                ("pages".to_owned(), pages),
            ]);
            assert!(queue.dispatch("ocr_notebook", &args).is_err());
        }
    }
}
