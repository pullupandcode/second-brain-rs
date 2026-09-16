//! Framework schema composition and vault workflows.

mod daily;
mod metadata;
mod records;
mod schema;

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use serde_json::{Map, Value, json};
use tokio::sync::Mutex;

use crate::{
    runtime::DispatchError,
    vault::{
        index::{SearchFilters, VaultIndex},
        markdown::FrontmatterValue,
        path::{VaultPathError, normalize_vault_path, resolve_existing_vault_path},
        reader::{VaultReader, VaultReaderError},
        writer::{VaultWriteError, VaultWriter},
    },
};

/// Shared framework service. Registry operations serialize read/modify/write.
#[derive(Debug)]
pub(crate) struct Framework {
    root: PathBuf,
    schema_path: String,
    reader: VaultReader,
    writer: Arc<VaultWriter>,
    index: Arc<VaultIndex>,
    registry_lock: Mutex<()>,
}

impl Framework {
    pub(crate) fn new(
        root: PathBuf,
        schema_path: String,
        reader: VaultReader,
        writer: Arc<VaultWriter>,
        index: Arc<VaultIndex>,
    ) -> Self {
        Self {
            root,
            schema_path,
            reader,
            writer,
            index,
            registry_lock: Mutex::new(()),
        }
    }

    // NOT cancel-safe: mutations are delegated to the audited writer; callers must await completion.
    pub(crate) async fn dispatch(
        &self,
        name: &str,
        args: &Map<String, Value>,
    ) -> Result<Value, DispatchError> {
        match name {
            "framework_init" => self.init(args).await,
            "framework_register" => self.register(args).await,
            "framework_unregister" => self.unregister(args).await,
            "framework_list" => Ok(Value::Array(
                self.registrations()
                    .await?
                    .into_iter()
                    .map(|mut value| {
                        value.insert("status".into(), json!("registered"));
                        value.into()
                    })
                    .collect(),
            )),
            "framework_reload" => self.reload().await,
            "framework_compose" => self.compose().await,
            "list_record_types" => Ok(json!({"recordTypes":self.record_types().await?})),
            "find_maps" => self.find_maps(args).await,
            "create_record" => self.create_record(args).await,
            "capture_for_date" | "inbox_capture" => self.capture(name, args).await,
            "daily_note_get" | "daily_note_append" | "daily_note_repair_markers" => {
                self.daily(name, args).await
            }
            _ => Err(DispatchError::UnknownTool(name.into())),
        }
    }

    // cancel-safe: reads only.
    pub(crate) async fn record_types(&self) -> Result<Vec<Value>, DispatchError> {
        let schema = self.compose().await?;
        Ok(schema
            .get("types")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(name, definition)| {
                let mut item = Map::from_iter([
                    ("name".into(), json!(name)),
                    (
                        "folder".into(),
                        definition.get("folder").cloned().unwrap_or(Value::Null),
                    ),
                ]);
                if let Some(description) = definition.get("description") {
                    item.insert("description".into(), description.clone());
                }
                Value::Object(item)
            })
            .collect())
    }

    // cancel-safe: reads only, no in-memory cached schema to partially update.
    async fn compose(&self) -> Result<Value, DispatchError> {
        let base =
            schema::parse_schema(&self.read_text(&self.schema_path).await?).map_err(invalid)?;
        let mut overlays = Vec::new();
        for registration in self.registrations().await? {
            let path = string(&registration, "path")?;
            overlays.push(schema::parse_schema(&self.read_text(path).await?).map_err(invalid)?);
        }
        schema::compose_schema(base, overlays).map_err(invalid)
    }

    // cancel-safe: reads only.
    async fn reload(&self) -> Result<Value, DispatchError> {
        let mut statuses = Vec::new();
        let mut ok = true;
        for mut registration in self.registrations().await? {
            let path = string(&registration, "path")?;
            let result = self
                .read_text(path)
                .await
                .and_then(|s| schema::parse_schema(&s).map_err(invalid));
            match result {
                Ok(_) => {
                    registration.insert("status".into(), json!("loaded"));
                }
                Err(error) => {
                    ok = false;
                    registration.insert("status".into(), json!("error"));
                    registration.insert("error".into(), json!(error.to_string()));
                }
            }
            statuses.push(registration);
        }
        Ok(json!({"ok":ok,"overlays":statuses}))
    }

    // NOT cancel-safe: publishes an atomic metadata file.
    async fn init(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let framework = string(args, "framework")?;
        let source = schema::materialize_preset(framework).map_err(invalid)?;
        let path = optional_string(args, "output_path")?.unwrap_or(&self.schema_path);
        let mode = optional_string(args, "mode")?.unwrap_or("create");
        if !matches!(mode, "create" | "overwrite") {
            return Err(invalid("mode must be create or overwrite"));
        }
        let _guard = self.registry_lock.lock().await;
        let existed = self
            .write_metadata(path, &source, mode == "overwrite")
            .await?;
        Ok(json!({"path":path,"framework":framework,"created":!existed,"overwritten":existed}))
    }

    // NOT cancel-safe: serializes and atomically publishes the registry.
    async fn register(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let name = string(args, "name")?;
        let path = self.check_path(string(args, "path")?)?;
        let (canonical, _) = crate::vault::path::canonical_vault_path_for_write(&self.root, &path)
            .await
            .map_err(|error| invalid(error.to_string()))?;
        self.check_path(&canonical)?;
        let priority = match args.get("priority") {
            None => json!(100),
            Some(value) => read_priority(value, "priority")?,
        };
        let _guard = self.registry_lock.lock().await;
        let mut entries = self.registrations().await?;
        entries.retain(|entry| entry.get("name").and_then(Value::as_str) != Some(name));
        entries.push(Map::from_iter([
            ("name".into(), json!(name)),
            ("path".into(), json!(path)),
            ("priority".into(), json!(priority)),
        ]));
        sort_registrations(&mut entries);
        self.save_registrations(&entries).await?;
        Ok(Value::Array(
            entries
                .into_iter()
                .map(|mut value| {
                    value.insert("status".into(), json!("registered"));
                    value.into()
                })
                .collect(),
        ))
    }

    // NOT cancel-safe: serializes and atomically publishes the registry.
    async fn unregister(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let name = string(args, "name")?;
        let _guard = self.registry_lock.lock().await;
        let mut entries = self.registrations().await?;
        let previous = entries.len();
        entries.retain(|entry| entry.get("name").and_then(Value::as_str) != Some(name));
        let removed = entries.len() != previous;
        if removed {
            self.save_registrations(&entries).await?;
        }
        Ok(json!({"removed":removed}))
    }

    // cancel-safe: reads only.
    async fn registrations(&self) -> Result<Vec<Map<String, Value>>, DispatchError> {
        let Some(source) = self.read_optional_text("_meta/schemas.json").await? else {
            return Ok(vec![]);
        };
        let parsed: Value = serde_json::from_str(&source)
            .map_err(|_| invalid("Invalid framework registry JSON"))?;
        let Some(overlays) = parsed.get("overlays").and_then(Value::as_array) else {
            return Ok(vec![]);
        };
        let mut entries = Vec::new();
        for entry in overlays {
            let entry = entry
                .as_object()
                .ok_or_else(|| invalid("overlay registration must be an object"))?;
            let name = string(entry, "name")?;
            let path = self.check_path(string(entry, "path")?)?;
            let priority = read_priority(
                entry.get("priority").unwrap_or(&Value::Null),
                "overlay.priority",
            )?;
            entries.push(Map::from_iter([
                ("name".into(), json!(name)),
                ("path".into(), json!(path)),
                ("priority".into(), json!(priority)),
            ]));
        }
        sort_registrations(&mut entries);
        Ok(entries)
    }

    // NOT cancel-safe: publishes an atomic registry file.
    async fn save_registrations(
        &self,
        entries: &[Map<String, Value>],
    ) -> Result<(), DispatchError> {
        let path = "_meta/schemas.json";
        let source = format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({"overlays":entries}))
                .map_err(|e| invalid(e.to_string()))?
        );
        self.write_metadata(path, &source, true).await?;
        Ok(())
    }

    fn check_path(&self, path: &str) -> Result<String, DispatchError> {
        let normalized = normalize_vault_path(path).map_err(|e| invalid(e.to_string()))?;
        if self.reader.is_blocked(&normalized) {
            return Err(invalid("Vault path is blocked"));
        }
        if self.reader.is_ignored(&normalized) {
            return Err(invalid("Vault path is ignored"));
        }
        Ok(normalized)
    }

    // cancel-safe: reads only; validates both lexical and resolved path policies.
    async fn read_optional_text(&self, path: &str) -> Result<Option<String>, DispatchError> {
        let normalized = self.check_path(path)?;
        let resolved = match resolve_existing_vault_path(&self.root, &normalized).await {
            Ok(path) => path,
            Err(VaultPathError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(invalid(error.to_string())),
        };
        let real_root = tokio::fs::canonicalize(&self.root)
            .await
            .map_err(|_| invalid("Vault path could not be resolved"))?;
        let relative = resolved
            .strip_prefix(real_root)
            .map_err(|_| invalid("Vault path resolves outside the vault root"))?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        self.check_path(&relative)?;
        let content = tokio::fs::read_to_string(resolved)
            .await
            .map_err(|_| invalid("Vault read failed"))?;
        // Skill reload can replace policy during the filesystem await.
        self.check_path(&normalized)?;
        self.check_path(&relative)?;
        Ok(Some(content))
    }

    // cancel-safe: reads only.
    async fn read_text(&self, path: &str) -> Result<String, DispatchError> {
        self.read_optional_text(path)
            .await?
            .ok_or_else(|| invalid("Vault read failed: file not found"))
    }

    // cancel-safe: read-only index queries continue harmlessly if canceled.
    async fn find_maps(&self, args: &Map<String, Value>) -> Result<Value, DispatchError> {
        let query = optional_string(args, "topic")?.unwrap_or("").to_owned();
        let schema = self.compose().await?;
        let mut folders = Vec::new();
        for (name, definition) in schema
            .get("types")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let folder = definition
                .get("folder")
                .and_then(Value::as_str)
                .unwrap_or("");
            let description = definition
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let searchable = format!("{name} {description} {folder}").to_lowercase();
            if searchable.contains("map") || searchable.contains("index") {
                folders.push(folder.to_owned());
            }
        }
        let index = Arc::clone(&self.index);
        let maps = tokio::task::spawn_blocking(move || {
            let mut maps = BTreeMap::new();
            for folder in folders {
                for result in index.search(
                    &query,
                    &SearchFilters {
                        folder: Some(folder),
                        tag: None,
                    },
                )? {
                    maps.insert(result.path.clone(), result);
                }
            }
            Ok::<_, crate::vault::index::IndexError>(maps.into_values().collect::<Vec<_>>())
        })
        .await
        .map_err(|_| invalid("Index task failed"))?
        .map_err(|_| invalid("Index query failed"))?;
        Ok(json!({"maps":maps}))
    }
}

fn sort_registrations(entries: &mut [Map<String, Value>]) {
    entries.sort_by(|a, b| {
        a.get("priority")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .total_cmp(&b.get("priority").and_then(Value::as_f64).unwrap_or(0.0))
            .then_with(|| {
                a.get("name")
                    .and_then(Value::as_str)
                    .cmp(&b.get("name").and_then(Value::as_str))
            })
    });
}
fn read_priority(value: &Value, name: &str) -> Result<Value, DispatchError> {
    if value
        .as_f64()
        .is_some_and(|number| number.is_finite() && number.fract() == 0.0)
    {
        Ok(value.clone())
    } else {
        Err(invalid(format!("{name} must be an integer")))
    }
}
fn invalid(message: impl Into<String>) -> DispatchError {
    DispatchError::Invalid(message.into())
}
const fn write_error(error: VaultWriteError) -> DispatchError {
    DispatchError::Write(error)
}
#[allow(clippy::needless_pass_by_value)] // Result::map_err consumes the reader error.
fn read_error(error: VaultReaderError) -> DispatchError {
    invalid(error.to_string())
}
fn is_missing(error: &VaultReaderError) -> bool {
    matches!(error,VaultReaderError::Io(e) | VaultReaderError::Path(VaultPathError::Io(e)) if e.kind()==std::io::ErrorKind::NotFound)
}
fn string<'a>(args: &'a Map<String, Value>, name: &str) -> Result<&'a str, DispatchError> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(format!("{name} must be a non-empty string")))
}
fn optional_string<'a>(
    args: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, DispatchError> {
    match args.get(name) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(invalid(format!("{name} must be a string"))),
    }
}
fn frontmatter(value: Option<&Value>) -> Result<BTreeMap<String, FrontmatterValue>, DispatchError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let values = value
        .as_object()
        .ok_or_else(|| invalid("fields must be an object"))?;
    values
        .iter()
        .map(|(key, value)| {
            let field = match value {
                Value::String(s) => FrontmatterValue::String(s.clone()),
                Value::Bool(b) => FrontmatterValue::Bool(*b),
                Value::Number(n) => FrontmatterValue::Number(
                    n.as_f64()
                        .ok_or_else(|| invalid("fields must contain finite numbers"))?,
                ),
                Value::Array(items) => FrontmatterValue::List(
                    items
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| invalid("fields arrays must contain strings"))
                        })
                        .collect::<Result<_, _>>()?,
                ),
                Value::Null | Value::Object(_) => {
                    return Err(invalid(
                        "fields must contain string, number, boolean, or string[]",
                    ));
                }
            };
            Ok((key.clone(), field))
        })
        .collect()
}
