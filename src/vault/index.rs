//! Rebuildable SQLite FTS5 index: search, links, source-id lookup, conflicts.
//!
//! The mutex guard is held across each (short, synchronous) query; there is no
//! `.await` inside any critical section, so tightening the guard scope adds
//! churn without benefit.
#![allow(clippy::significant_drop_tightening)]

use std::sync::Mutex;

use rusqlite::{Connection, params};
use serde::Serialize;

use crate::vault::policy::PathPolicy;

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS notes (
  path TEXT PRIMARY KEY,
  title TEXT,
  content TEXT NOT NULL,
  tags_json TEXT NOT NULL,
  aliases_json TEXT NOT NULL,
  source_id TEXT,
  sha256 TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS note_fts USING fts5(path UNINDEXED, title, content);
CREATE TABLE IF NOT EXISTS links (source_path TEXT NOT NULL, target TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS conflicts (canonical TEXT NOT NULL, conflict TEXT NOT NULL);";

/// Errors from index operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IndexError {
    /// SQLite error.
    #[error("index database error")]
    Sqlite(#[from] rusqlite::Error),
    /// JSON (de)serialization error.
    #[error("index serialization error")]
    Json(#[from] serde_json::Error),
}

/// A note to index.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NoteRecord {
    /// Vault-relative path.
    pub path: String,
    /// Title, if any.
    pub title: Option<String>,
    /// Body content (for FTS).
    pub content: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Aliases.
    pub aliases: Vec<String>,
    /// Stable source id, if any.
    pub source_id: Option<String>,
    /// Hex SHA-256.
    pub sha256: String,
    /// Outgoing wikilink targets.
    pub outgoing_links: Vec<String>,
}

/// A canonical/conflict pair from sync-conflict detection.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ConflictPair {
    /// The canonical sibling path.
    pub canonical: String,
    /// The conflict file path.
    pub conflict: String,
}

/// Optional search filters.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SearchFilters {
    /// Restrict to paths under this folder.
    pub folder: Option<String>,
    /// Restrict to notes carrying this tag.
    pub tag: Option<String>,
}

/// A search hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SearchResult {
    /// Vault-relative path.
    pub path: String,
    /// Title, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
    /// Aliases.
    pub aliases: Vec<String>,
    /// Hex SHA-256.
    pub current_sha256: String,
}

/// Active sync-conflict grouping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct VaultConflict {
    /// Canonical path.
    pub canonical: String,
    /// Conflict file paths.
    pub conflicts: Vec<String>,
}

/// A rebuildable SQLite index with blocked-path result filtering.
pub struct VaultIndex {
    connection: Mutex<Connection>,
    blocked_paths: PathPolicy,
}

impl std::fmt::Debug for VaultIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultIndex").finish_non_exhaustive()
    }
}

impl VaultIndex {
    /// Open (or create) the index at `sqlite_path`, applying schema.
    ///
    /// `blocked_paths` (the security denylist) filter all query results.
    ///
    /// # Errors
    /// [`IndexError::Sqlite`] on connection or schema failure.
    pub fn open(sqlite_path: &str, blocked_paths: Vec<String>) -> Result<Self, IndexError> {
        Self::open_with_policy(sqlite_path, PathPolicy::new(blocked_paths))
    }

    /// Open an index sharing the effective privacy policy.
    /// # Errors
    /// Returns an error if SQLite cannot be opened or initialized.
    pub fn open_with_policy(
        sqlite_path: &str,
        blocked_paths: PathPolicy,
    ) -> Result<Self, IndexError> {
        let connection = Connection::open(sqlite_path)?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self {
            connection: Mutex::new(connection),
            blocked_paths,
        })
    }

    fn is_blocked(&self, path: &str) -> bool {
        self.blocked_paths.is_blocked(path)
    }

    /// Clear and repopulate the index from `notes` and `conflicts`.
    ///
    /// # Errors
    /// [`IndexError`] on SQLite or serialization failure.
    pub fn rebuild(
        &self,
        notes: &[NoteRecord],
        conflicts: &[ConflictPair],
    ) -> Result<(), IndexError> {
        let mut guard = self
            .connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tx = guard.transaction()?;
        tx.execute_batch(
            "DELETE FROM notes; DELETE FROM note_fts; DELETE FROM links; DELETE FROM conflicts;",
        )?;
        for note in notes {
            let tags_json = serde_json::to_string(&note.tags)?;
            let aliases_json = serde_json::to_string(&note.aliases)?;
            tx.execute(
                "INSERT INTO notes (path, title, content, tags_json, aliases_json, source_id, sha256)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    note.path,
                    note.title,
                    note.content,
                    tags_json,
                    aliases_json,
                    note.source_id,
                    note.sha256
                ],
            )?;
            tx.execute(
                "INSERT INTO note_fts (path, title, content) VALUES (?1, ?2, ?3)",
                params![
                    note.path,
                    note.title.clone().unwrap_or_default(),
                    note.content
                ],
            )?;
            for target in &note.outgoing_links {
                tx.execute(
                    "INSERT INTO links (source_path, target) VALUES (?1, ?2)",
                    params![note.path, target],
                )?;
            }
        }
        for pair in conflicts {
            tx.execute(
                "INSERT INTO conflicts (canonical, conflict) VALUES (?1, ?2)",
                params![pair.canonical, pair.conflict],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Full-text (or list-all) search with optional folder/tag filters.
    ///
    /// # Errors
    /// [`IndexError`] on SQLite or serialization failure.
    pub fn search(
        &self,
        query: &str,
        filters: &SearchFilters,
    ) -> Result<Vec<SearchResult>, IndexError> {
        let guard = self
            .connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let trimmed = query.trim();
        let mut results = Vec::new();
        let row_map = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        };

        let rows: Vec<(String, Option<String>, String, String, String)> = if trimmed.is_empty() {
            let mut stmt = guard.prepare(
                "SELECT path, title, tags_json, aliases_json, sha256 FROM notes ORDER BY path",
            )?;
            stmt.query_map([], row_map)?.collect::<Result<_, _>>()?
        } else {
            let mut stmt = guard.prepare(
                "SELECT path, title, tags_json, aliases_json, sha256 FROM notes
                 WHERE path IN (SELECT path FROM note_fts WHERE note_fts MATCH ?1)
                 ORDER BY path",
            )?;
            stmt.query_map(params![trimmed], row_map)?
                .collect::<Result<_, _>>()?
        };

        let folder = filters
            .folder
            .as_deref()
            .map(|value| value.trim_end_matches('/').to_owned());
        for (path, title, tags_json, aliases_json, sha256) in rows {
            if self.is_blocked(&path) {
                continue;
            }
            if let Some(folder) = &folder
                && !path.starts_with(&format!("{folder}/"))
            {
                continue;
            }
            let tags: Vec<String> = serde_json::from_str(&tags_json)?;
            if let Some(tag) = &filters.tag
                && !tags.contains(tag)
            {
                continue;
            }
            let aliases: Vec<String> = serde_json::from_str(&aliases_json)?;
            results.push(SearchResult {
                path,
                title,
                tags,
                aliases,
                current_sha256: sha256,
            });
        }
        Ok(results)
    }

    /// Notes linking to `path`.
    ///
    /// # Errors
    /// [`IndexError::Sqlite`] on query failure.
    pub fn backlinks(&self, path: &str) -> Result<Vec<String>, IndexError> {
        let guard = self
            .connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = guard.prepare(
            "SELECT DISTINCT source_path FROM links WHERE target = ?1 ORDER BY source_path",
        )?;
        let rows = stmt.query_map(params![path], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let source = row?;
            if !self.is_blocked(&source) {
                out.push(source);
            }
        }
        Ok(out)
    }

    /// Targets linked from `path` (empty if the source is blocked).
    ///
    /// # Errors
    /// [`IndexError::Sqlite`] on query failure.
    pub fn outgoing_links(&self, path: &str) -> Result<Vec<String>, IndexError> {
        if self.is_blocked(path) {
            return Ok(Vec::new());
        }
        let guard = self
            .connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt =
            guard.prepare("SELECT target FROM links WHERE source_path = ?1 ORDER BY target")?;
        let rows = stmt.query_map(params![path], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The path of the note carrying `source_id`, if indexed and not blocked.
    ///
    /// # Errors
    /// [`IndexError::Sqlite`] on query failure.
    pub fn find_by_source_id(&self, source_id: &str) -> Result<Option<String>, IndexError> {
        let guard = self
            .connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = guard.prepare("SELECT path FROM notes WHERE source_id = ?1 LIMIT 1")?;
        let mut rows = stmt.query_map(params![source_id], |row| row.get::<_, String>(0))?;
        match rows.next() {
            Some(row) => {
                let path = row?;
                Ok((!self.is_blocked(&path)).then_some(path))
            }
            None => Ok(None),
        }
    }

    /// Active conflicts, grouped by canonical path (blocked canonicals excluded).
    ///
    /// # Errors
    /// [`IndexError::Sqlite`] on query failure.
    pub fn list_conflicts(&self) -> Result<Vec<VaultConflict>, IndexError> {
        let guard = self
            .connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = guard
            .prepare("SELECT canonical, conflict FROM conflicts ORDER BY canonical, conflict")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut grouped: Vec<VaultConflict> = Vec::new();
        for row in rows {
            let (canonical, conflict) = row?;
            if self.is_blocked(&canonical) || self.is_blocked(&conflict) {
                continue;
            }
            match grouped.last_mut() {
                Some(last) if last.canonical == canonical => last.conflicts.push(conflict),
                _ => grouped.push(VaultConflict {
                    canonical,
                    conflicts: vec![conflict],
                }),
            }
        }
        Ok(grouped)
    }
}

/// If `path` is a sync-conflict file (`foo.sync-conflict-XXX.md`), return its
/// canonical sibling (`foo.md`).
#[must_use]
pub fn canonical_conflict_path(path: &str) -> Option<String> {
    const MARKER: &str = ".sync-conflict-";
    let stem = path.strip_suffix(".md")?;
    let index = stem.rfind(MARKER)?;
    let suffix = stem.get(index + MARKER.len()..)?;
    if suffix.is_empty() || suffix.contains('.') {
        return None;
    }
    let base = stem.get(..index)?;
    Some(format!("{base}.md"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(path: &str, content: &str, tags: &[&str], links: &[&str]) -> NoteRecord {
        NoteRecord {
            path: path.to_owned(),
            title: Some(path.to_owned()),
            content: content.to_owned(),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            aliases: Vec::new(),
            source_id: None,
            sha256: "0".repeat(64),
            outgoing_links: links.iter().map(|l| (*l).to_owned()).collect(),
        }
    }

    fn index() -> VaultIndex {
        VaultIndex::open(":memory:", vec!["Private/**".to_owned()]).unwrap()
    }

    #[test]
    fn rebuild_search_by_content_folder_and_tag() {
        let idx = index();
        idx.rebuild(
            &[
                note("Notes/apple.md", "red apple fruit", &["fruit"], &[]),
                note("Other/banana.md", "yellow banana fruit", &["fruit"], &[]),
                note("Notes/rock.md", "grey stone", &["mineral"], &[]),
            ],
            &[],
        )
        .unwrap();

        let hits = idx.search("apple", &SearchFilters::default()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits.first().map(|h| h.path.as_str()),
            Some("Notes/apple.md")
        );

        let folder = SearchFilters {
            folder: Some("Notes".to_owned()),
            tag: None,
        };
        let hits = idx.search("fruit", &folder).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
            vec!["Notes/apple.md"]
        );

        let tagged = SearchFilters {
            folder: None,
            tag: Some("mineral".to_owned()),
        };
        let hits = idx.search("", &tagged).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
            vec!["Notes/rock.md"]
        );
    }

    #[test]
    fn backlinks_and_outgoing_links() {
        let idx = index();
        idx.rebuild(
            &[
                note("A.md", "links to B", &[], &["B"]),
                note("C.md", "also links to B", &[], &["B"]),
            ],
            &[],
        )
        .unwrap();
        assert_eq!(idx.backlinks("B").unwrap(), vec!["A.md", "C.md"]);
        assert_eq!(idx.outgoing_links("A.md").unwrap(), vec!["B"]);
    }

    #[test]
    fn blocked_paths_filtered_from_results() {
        let idx = index();
        idx.rebuild(
            &[
                note("Private/secret.md", "classified fruit", &["fruit"], &["X"]),
                note("Public/open.md", "open fruit", &["fruit"], &[]),
            ],
            &[],
        )
        .unwrap();
        let hits = idx.search("fruit", &SearchFilters::default()).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
            vec!["Public/open.md"]
        );
        assert!(idx.backlinks("X").unwrap().is_empty());
        assert!(idx.outgoing_links("Private/secret.md").unwrap().is_empty());
    }

    #[test]
    fn conflicts_grouped_and_blocked_excluded() {
        let idx = index();
        idx.rebuild(
            &[],
            &[
                ConflictPair {
                    canonical: "n.md".to_owned(),
                    conflict: "n.sync-conflict-7.md".to_owned(),
                },
                ConflictPair {
                    canonical: "Private/p.md".to_owned(),
                    conflict: "Private/p.sync-conflict-9.md".to_owned(),
                },
            ],
        )
        .unwrap();
        let conflicts = idx.list_conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(
            conflicts.first().map(|c| c.canonical.as_str()),
            Some("n.md")
        );
    }

    #[test]
    fn canonical_conflict_detection() {
        assert_eq!(
            canonical_conflict_path("notes/foo.sync-conflict-abc123.md").as_deref(),
            Some("notes/foo.md")
        );
        assert_eq!(canonical_conflict_path("notes/foo.md"), None);
        assert_eq!(
            canonical_conflict_path("notes/foo.sync-conflict-a.b.md"),
            None
        );
    }
}
