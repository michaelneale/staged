//! Review storage using SQLite.
//!
//! Reviews are stored separately from git, keyed by DiffId.

use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::types::{DiffId, Span};

/// A review attached to a specific diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: DiffId,
    /// Paths that have been marked as reviewed
    pub reviewed: Vec<String>,
    /// Comments attached to specific locations
    pub comments: Vec<Comment>,
    /// Edits made during review (stored as diffs)
    pub edits: Vec<Edit>,
}

impl Review {
    pub fn new(id: DiffId) -> Self {
        Self {
            id,
            reviewed: Vec::new(),
            comments: Vec::new(),
            edits: Vec::new(),
        }
    }
}

/// A comment attached to a specific location in a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub path: String,
    pub selection: Selection,
    pub content: String,
}

/// Where a comment applies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Selection {
    /// Applies to the whole file
    Global,
    /// Applies to a specific line (0-indexed)
    Line { line: u32 },
    /// Applies to a range of lines
    Range { span: Span },
}

/// An edit made during review, stored as a unified diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edit {
    pub id: String,
    pub path: String,
    /// Unified diff format
    pub diff: String,
}

/// Error type for review storage operations.
#[derive(Debug)]
pub struct ReviewError(pub String);

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ReviewError {}

impl From<rusqlite::Error> for ReviewError {
    fn from(e: rusqlite::Error) -> Self {
        ReviewError(e.to_string())
    }
}

type Result<T> = std::result::Result<T, ReviewError>;

/// Review storage backed by SQLite.
pub struct ReviewStore {
    conn: Connection,
}

impl ReviewStore {
    /// Open or create the review database at the given path.
    pub fn open(db_path: PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ReviewError(format!("Cannot create directory: {}", e)))?;
        }

        let conn = Connection::open(&db_path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize the database schema.
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS reviews (
                before_ref TEXT NOT NULL,
                after_ref TEXT NOT NULL,
                PRIMARY KEY (before_ref, after_ref)
            );

            CREATE TABLE IF NOT EXISTS reviewed_files (
                before_ref TEXT NOT NULL,
                after_ref TEXT NOT NULL,
                path TEXT NOT NULL,
                PRIMARY KEY (before_ref, after_ref, path),
                FOREIGN KEY (before_ref, after_ref) REFERENCES reviews(before_ref, after_ref)
            );

            CREATE TABLE IF NOT EXISTS comments (
                id TEXT PRIMARY KEY,
                before_ref TEXT NOT NULL,
                after_ref TEXT NOT NULL,
                path TEXT NOT NULL,
                selection_type TEXT NOT NULL,
                selection_line INTEGER,
                selection_start INTEGER,
                selection_end INTEGER,
                content TEXT NOT NULL,
                FOREIGN KEY (before_ref, after_ref) REFERENCES reviews(before_ref, after_ref)
            );

            CREATE TABLE IF NOT EXISTS edits (
                id TEXT PRIMARY KEY,
                before_ref TEXT NOT NULL,
                after_ref TEXT NOT NULL,
                path TEXT NOT NULL,
                diff TEXT NOT NULL,
                FOREIGN KEY (before_ref, after_ref) REFERENCES reviews(before_ref, after_ref)
            );
            "#,
        )?;
        Ok(())
    }

    /// Get or create a review for the given diff.
    pub fn get_or_create(&self, id: &DiffId) -> Result<Review> {
        // Ensure review exists
        self.conn.execute(
            "INSERT OR IGNORE INTO reviews (before_ref, after_ref) VALUES (?1, ?2)",
            params![&id.before, &id.after],
        )?;

        self.get(id)
    }

    /// Get a review by its DiffId.
    pub fn get(&self, id: &DiffId) -> Result<Review> {
        // Check if review exists
        let exists: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM reviews WHERE before_ref = ?1 AND after_ref = ?2",
                params![&id.before, &id.after],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);

        if !exists {
            return Ok(Review::new(id.clone()));
        }

        // Load reviewed files
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM reviewed_files WHERE before_ref = ?1 AND after_ref = ?2")?;
        let reviewed: Vec<String> = stmt
            .query_map(params![&id.before, &id.after], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Load comments
        let mut stmt = self.conn.prepare(
            "SELECT id, path, selection_type, selection_line, selection_start, selection_end, content 
             FROM comments WHERE before_ref = ?1 AND after_ref = ?2",
        )?;
        let comments: Vec<Comment> = stmt
            .query_map(params![&id.before, &id.after], |row| {
                let id: String = row.get(0)?;
                let path: String = row.get(1)?;
                let selection_type: String = row.get(2)?;
                let selection_line: Option<u32> = row.get(3)?;
                let selection_start: Option<u32> = row.get(4)?;
                let selection_end: Option<u32> = row.get(5)?;
                let content: String = row.get(6)?;

                let selection = match selection_type.as_str() {
                    "global" => Selection::Global,
                    "line" => Selection::Line {
                        line: selection_line.unwrap_or(0),
                    },
                    "range" => Selection::Range {
                        span: Span::new(selection_start.unwrap_or(0), selection_end.unwrap_or(0)),
                    },
                    _ => Selection::Global,
                };

                Ok(Comment {
                    id,
                    path,
                    selection,
                    content,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Load edits
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, diff FROM edits WHERE before_ref = ?1 AND after_ref = ?2")?;
        let edits: Vec<Edit> = stmt
            .query_map(params![&id.before, &id.after], |row| {
                Ok(Edit {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    diff: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(Review {
            id: id.clone(),
            reviewed,
            comments,
            edits,
        })
    }

    /// Mark a file as reviewed.
    pub fn mark_reviewed(&self, id: &DiffId, path: &str) -> Result<()> {
        self.get_or_create(id)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO reviewed_files (before_ref, after_ref, path) VALUES (?1, ?2, ?3)",
            params![&id.before, &id.after, path],
        )?;
        Ok(())
    }

    /// Unmark a file as reviewed.
    pub fn unmark_reviewed(&self, id: &DiffId, path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM reviewed_files WHERE before_ref = ?1 AND after_ref = ?2 AND path = ?3",
            params![&id.before, &id.after, path],
        )?;
        Ok(())
    }

    /// Add a comment.
    pub fn add_comment(&self, id: &DiffId, comment: Comment) -> Result<()> {
        self.get_or_create(id)?;

        let (selection_type, selection_line, selection_start, selection_end) =
            match &comment.selection {
                Selection::Global => ("global", None, None, None),
                Selection::Line { line } => ("line", Some(*line), None, None),
                Selection::Range { span } => ("range", None, Some(span.start), Some(span.end)),
            };

        self.conn.execute(
            "INSERT INTO comments (id, before_ref, after_ref, path, selection_type, selection_line, selection_start, selection_end, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &comment.id,
                &id.before,
                &id.after,
                &comment.path,
                selection_type,
                selection_line,
                selection_start,
                selection_end,
                &comment.content
            ],
        )?;
        Ok(())
    }

    /// Update a comment's content.
    pub fn update_comment(&self, comment_id: &str, content: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE comments SET content = ?1 WHERE id = ?2",
            params![content, comment_id],
        )?;
        Ok(())
    }

    /// Delete a comment.
    pub fn delete_comment(&self, comment_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM comments WHERE id = ?1", params![comment_id])?;
        Ok(())
    }

    /// Add an edit.
    pub fn add_edit(&self, id: &DiffId, edit: Edit) -> Result<()> {
        self.get_or_create(id)?;
        self.conn.execute(
            "INSERT INTO edits (id, before_ref, after_ref, path, diff) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![&edit.id, &id.before, &id.after, &edit.path, &edit.diff],
        )?;
        Ok(())
    }

    /// Delete an edit.
    pub fn delete_edit(&self, edit_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM edits WHERE id = ?1", params![edit_id])?;
        Ok(())
    }

    /// Delete an entire review and all associated data.
    pub fn delete_review(&self, id: &DiffId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM reviewed_files WHERE before_ref = ?1 AND after_ref = ?2",
            params![&id.before, &id.after],
        )?;
        self.conn.execute(
            "DELETE FROM comments WHERE before_ref = ?1 AND after_ref = ?2",
            params![&id.before, &id.after],
        )?;
        self.conn.execute(
            "DELETE FROM edits WHERE before_ref = ?1 AND after_ref = ?2",
            params![&id.before, &id.after],
        )?;
        self.conn.execute(
            "DELETE FROM reviews WHERE before_ref = ?1 AND after_ref = ?2",
            params![&id.before, &id.after],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_mark_reviewed() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ReviewStore::open(db_path).unwrap();
        let id = DiffId::new("main", "feature");

        store.mark_reviewed(&id, "src/main.rs").unwrap();
        let review = store.get(&id).unwrap();
        assert_eq!(review.reviewed, vec!["src/main.rs"]);

        store.unmark_reviewed(&id, "src/main.rs").unwrap();
        let review = store.get(&id).unwrap();
        assert!(review.reviewed.is_empty());
    }

    #[test]
    fn test_comments() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ReviewStore::open(db_path).unwrap();
        let id = DiffId::new("main", "feature");

        let comment = Comment {
            id: "c1".into(),
            path: "src/lib.rs".into(),
            selection: Selection::Line { line: 42 },
            content: "This looks wrong".into(),
        };

        store.add_comment(&id, comment).unwrap();
        let review = store.get(&id).unwrap();
        assert_eq!(review.comments.len(), 1);
        assert_eq!(review.comments[0].content, "This looks wrong");

        store.update_comment("c1", "Actually it's fine").unwrap();
        let review = store.get(&id).unwrap();
        assert_eq!(review.comments[0].content, "Actually it's fine");

        store.delete_comment("c1").unwrap();
        let review = store.get(&id).unwrap();
        assert!(review.comments.is_empty());
    }
}
