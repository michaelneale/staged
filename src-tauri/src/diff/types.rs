//! Core data types for the diff viewer.
//!
//! These types represent the minimal information needed to display diffs
//! and track reviews. The design prioritizes simplicity and statelessness.

use serde::{Deserialize, Serialize};

/// Identifies a diff between two repository states.
///
/// - `before`: A ref (branch name, tag), SHA, or "HEAD"
/// - `after`: A ref, SHA, or "@" for the working tree
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DiffId {
    pub before: String,
    pub after: String,
}

impl DiffId {
    pub fn new(before: impl Into<String>, after: impl Into<String>) -> Self {
        Self {
            before: before.into(),
            after: after.into(),
        }
    }

    /// Returns true if this diff includes the working tree.
    pub fn is_working_tree(&self) -> bool {
        self.after == "@"
    }
}

/// The diff for a single file between two states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// The file path (uses `after` path, or `before` path if deleted)
    pub path: String,
    /// Content before the change (None if file was added)
    pub before: Option<FileContent>,
    /// Content after the change (None if file was deleted)
    pub after: Option<FileContent>,
    /// Mappings between regions in before/after for scroll sync
    pub connections: Vec<Connection>,
}

impl FileDiff {
    /// Returns the kind of change this file underwent.
    pub fn change_kind(&self) -> ChangeKind {
        match (&self.before, &self.after) {
            (None, Some(_)) => ChangeKind::Added,
            (Some(_), None) => ChangeKind::Deleted,
            (Some(_), Some(_)) => ChangeKind::Modified,
            (None, None) => ChangeKind::Modified, // shouldn't happen
        }
    }
}

/// The type of change a file underwent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

/// Content of a file at a specific state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FileContent {
    Text { lines: Vec<String> },
    Binary,
}

impl FileContent {
    /// Create text content from a string, splitting into lines.
    pub fn from_text(content: &str) -> Self {
        let lines: Vec<String> = content.lines().map(String::from).collect();
        Self::Text { lines }
    }

    /// Check if content appears to be binary.
    pub fn is_binary(bytes: &[u8]) -> bool {
        // Check for null bytes in first 8KB (common heuristic)
        let check_len = bytes.len().min(8192);
        bytes[..check_len].contains(&0)
    }
}

/// A mapping between a region in the before file and a region in the after file.
///
/// Used for scroll synchronization and drawing connectors between panes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub before: Span,
    pub after: Span,
}

/// A contiguous range of lines (0-indexed, exclusive end).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_id_working_tree() {
        let working = DiffId::new("HEAD", "@");
        assert!(working.is_working_tree());

        let historical = DiffId::new("main", "feature");
        assert!(!historical.is_working_tree());
    }

    #[test]
    fn test_change_kind() {
        let added = FileDiff {
            path: "new.txt".into(),
            before: None,
            after: Some(FileContent::Text { lines: vec![] }),
            connections: vec![],
        };
        assert_eq!(added.change_kind(), ChangeKind::Added);

        let deleted = FileDiff {
            path: "old.txt".into(),
            before: Some(FileContent::Text { lines: vec![] }),
            after: None,
            connections: vec![],
        };
        assert_eq!(deleted.change_kind(), ChangeKind::Deleted);

        let modified = FileDiff {
            path: "changed.txt".into(),
            before: Some(FileContent::Text { lines: vec![] }),
            after: Some(FileContent::Text { lines: vec![] }),
            connections: vec![],
        };
        assert_eq!(modified.change_kind(), ChangeKind::Modified);
    }

    #[test]
    fn test_binary_detection() {
        assert!(FileContent::is_binary(&[0x00, 0x01, 0x02]));
        assert!(!FileContent::is_binary(b"hello world"));
    }
}
