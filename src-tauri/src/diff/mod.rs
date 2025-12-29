//! Diff module - core data types and operations for the diff viewer.
//!
//! This module provides:
//! - `types`: Core data structures (DiffId, FileDiff, etc.)
//! - `git`: Git operations for computing diffs
//! - `review`: SQLite-backed review storage
//! - `actions`: Git actions (commit, discard, etc.)
//! - `watcher`: File system watcher for detecting changes

pub mod actions;
pub mod git;
pub mod review;
pub mod types;
pub mod watcher;

// Re-export commonly used types
pub use actions::{amend, commit, discard_file, discard_region, stage_file, unstage_file};
pub use git::{compute_diff, current_branch, last_commit_message, list_refs, open_repo, GitError};
pub use review::{Comment, Edit, Review, ReviewStore, Selection};
pub use types::{Alignment, ChangeKind, DiffId, File, FileContent, FileDiff, Span};
pub use watcher::RepoWatcher;
