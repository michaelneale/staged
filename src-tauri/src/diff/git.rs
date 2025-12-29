//! Git operations for computing diffs.
//!
//! All functions are stateless - they discover the repo fresh each call.

use std::collections::HashMap;
use std::path::Path;

use git2::{DiffOptions, Repository, Tree};

use super::types::{Connection, DiffId, FileContent, FileDiff, Span};

/// Error type for git operations.
#[derive(Debug)]
pub struct GitError(pub String);

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for GitError {}

impl From<git2::Error> for GitError {
    fn from(e: git2::Error) -> Self {
        GitError(e.message().to_string())
    }
}

type Result<T> = std::result::Result<T, GitError>;

/// Open the repository containing the given path.
pub fn open_repo(path: &Path) -> Result<Repository> {
    Repository::discover(path).map_err(Into::into)
}

/// List all refs (branches, tags) for autocomplete.
pub fn list_refs(repo: &Repository) -> Result<Vec<String>> {
    let mut refs = Vec::new();

    // Local branches
    for branch in repo.branches(Some(git2::BranchType::Local))? {
        let (branch, _) = branch?;
        if let Some(name) = branch.name()? {
            refs.push(name.to_string());
        }
    }

    // Remote branches
    for branch in repo.branches(Some(git2::BranchType::Remote))? {
        let (branch, _) = branch?;
        if let Some(name) = branch.name()? {
            refs.push(name.to_string());
        }
    }

    // Tags
    repo.tag_foreach(|_oid, name| {
        if let Ok(name) = std::str::from_utf8(name) {
            // Strip "refs/tags/" prefix
            let name = name.strip_prefix("refs/tags/").unwrap_or(name);
            refs.push(name.to_string());
        }
        true
    })?;

    refs.sort();
    refs.dedup();
    Ok(refs)
}

/// Get the current branch name.
pub fn current_branch(repo: &Repository) -> Result<Option<String>> {
    let head = repo.head()?;
    if head.is_branch() {
        Ok(head.shorthand().map(String::from))
    } else {
        // Detached HEAD
        Ok(None)
    }
}

/// Get the last commit message (for amend).
pub fn last_commit_message(repo: &Repository) -> Result<Option<String>> {
    let head = repo.head()?;
    let commit = head.peel_to_commit()?;
    Ok(commit.message().map(String::from))
}

/// Resolve a ref string to a tree.
///
/// Special values:
/// - "@" means the working tree (returns None, caller handles specially)
/// - "HEAD" resolves to the current HEAD commit
fn resolve_to_tree<'a>(repo: &'a Repository, refspec: &str) -> Result<Option<Tree<'a>>> {
    if refspec == "@" {
        return Ok(None); // Working tree - no tree object
    }

    let obj = repo
        .revparse_single(refspec)
        .map_err(|e| GitError(format!("Cannot resolve '{}': {}", refspec, e)))?;

    let commit = obj
        .peel_to_commit()
        .map_err(|e| GitError(format!("'{}' is not a commit: {}", refspec, e)))?;

    Ok(Some(commit.tree()?))
}

/// Compute the diff between two refs.
///
/// Returns a list of FileDiff objects with full content and connections.
pub fn compute_diff(repo: &Repository, diff_id: &DiffId) -> Result<Vec<FileDiff>> {
    let before_tree = resolve_to_tree(repo, &diff_id.before)?;
    let after_tree = resolve_to_tree(repo, &diff_id.after)?;

    let mut opts = DiffOptions::new();
    opts.ignore_submodules(true);

    let diff = if diff_id.is_working_tree() {
        // Diff from before_tree to working directory
        repo.diff_tree_to_workdir_with_index(before_tree.as_ref(), Some(&mut opts))?
    } else {
        // Diff between two trees
        repo.diff_tree_to_tree(before_tree.as_ref(), after_tree.as_ref(), Some(&mut opts))?
    };

    // Collect changed file paths
    let mut file_diffs: HashMap<String, FileDiff> = HashMap::new();

    // First pass: identify files and their status
    for delta in diff.deltas() {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        if path.is_empty() {
            continue;
        }

        file_diffs.insert(
            path.clone(),
            FileDiff {
                path,
                before: None,
                after: None,
                connections: Vec::new(),
            },
        );
    }

    // Second pass: load content for each file
    for file_diff in file_diffs.values_mut() {
        let path = Path::new(&file_diff.path);

        // Load "before" content from before_tree
        if let Some(ref tree) = before_tree {
            file_diff.before = load_file_content_from_tree(repo, tree, path)?;
        }

        // Load "after" content
        if diff_id.is_working_tree() {
            // Read from working directory
            let workdir = repo
                .workdir()
                .ok_or_else(|| GitError("Bare repository".into()))?;
            let full_path = workdir.join(path);
            file_diff.after = load_file_content_from_disk(&full_path)?;
        } else if let Some(ref tree) = after_tree {
            file_diff.after = load_file_content_from_tree(repo, tree, path)?;
        }

        // Compute connections between before and after
        file_diff.connections = compute_connections(&file_diff.before, &file_diff.after);
    }

    let mut result: Vec<_> = file_diffs.into_values().collect();
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

/// Load file content from a git tree.
fn load_file_content_from_tree(
    repo: &Repository,
    tree: &Tree,
    path: &Path,
) -> Result<Option<FileContent>> {
    let entry = match tree.get_path(path) {
        Ok(e) => e,
        Err(_) => return Ok(None), // File doesn't exist in this tree
    };

    let obj = entry
        .to_object(repo)
        .map_err(|e| GitError(format!("Cannot load object: {}", e)))?;

    let blob = match obj.as_blob() {
        Some(b) => b,
        None => return Ok(None), // Not a file (maybe a submodule)
    };

    let content = blob.content();
    if FileContent::is_binary(content) {
        Ok(Some(FileContent::Binary))
    } else {
        let text = String::from_utf8_lossy(content);
        Ok(Some(FileContent::from_text(&text)))
    }
}

/// Load file content from disk.
fn load_file_content_from_disk(path: &Path) -> Result<Option<FileContent>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read(path).map_err(|e| GitError(format!("Cannot read file: {}", e)))?;

    if FileContent::is_binary(&content) {
        Ok(Some(FileContent::Binary))
    } else {
        let text = String::from_utf8_lossy(&content);
        Ok(Some(FileContent::from_text(&text)))
    }
}

/// Compute connections between before and after content.
///
/// Uses a simple diff algorithm to find matching regions.
fn compute_connections(
    before: &Option<FileContent>,
    after: &Option<FileContent>,
) -> Vec<Connection> {
    let before_lines = match before {
        Some(FileContent::Text { lines }) => lines.as_slice(),
        _ => &[],
    };
    let after_lines = match after {
        Some(FileContent::Text { lines }) => lines.as_slice(),
        _ => &[],
    };

    if before_lines.is_empty() && after_lines.is_empty() {
        return vec![];
    }

    // Use a simple LCS-based approach to find matching blocks
    let matches = find_matching_blocks(before_lines, after_lines);

    // Convert matching blocks to connections
    // Each match represents an unchanged region; the gaps are changes
    let mut connections = Vec::new();
    let mut before_pos = 0u32;
    let mut after_pos = 0u32;

    for (before_start, after_start, len) in matches {
        let before_start = before_start as u32;
        let after_start = after_start as u32;
        let len = len as u32;

        // If there's a gap before this match, that's a changed region
        if before_pos < before_start || after_pos < after_start {
            connections.push(Connection {
                before: Span::new(before_pos, before_start),
                after: Span::new(after_pos, after_start),
            });
        }

        // The matching region itself
        if len > 0 {
            connections.push(Connection {
                before: Span::new(before_start, before_start + len),
                after: Span::new(after_start, after_start + len),
            });
        }

        before_pos = before_start + len;
        after_pos = after_start + len;
    }

    // Handle any remaining content after the last match
    let before_len = before_lines.len() as u32;
    let after_len = after_lines.len() as u32;
    if before_pos < before_len || after_pos < after_len {
        connections.push(Connection {
            before: Span::new(before_pos, before_len),
            after: Span::new(after_pos, after_len),
        });
    }

    connections
}

/// Find matching blocks between two sequences of lines.
///
/// Returns a list of (before_start, after_start, length) tuples.
fn find_matching_blocks(before: &[String], after: &[String]) -> Vec<(usize, usize, usize)> {
    if before.is_empty() || after.is_empty() {
        return vec![];
    }

    // Build a map of line -> positions in "after"
    let mut after_positions: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, line) in after.iter().enumerate() {
        after_positions.entry(line.as_str()).or_default().push(i);
    }

    // Find longest common subsequence using patience diff approach
    let mut matches = Vec::new();
    let mut after_used = vec![false; after.len()];

    let mut before_idx = 0;
    while before_idx < before.len() {
        let line = &before[before_idx];

        // Find the first unused occurrence in after
        if let Some(positions) = after_positions.get(line.as_str()) {
            if let Some(&after_idx) = positions.iter().find(|&&i| !after_used[i]) {
                // Found a match - extend it as far as possible
                let mut len = 1;
                after_used[after_idx] = true;

                while before_idx + len < before.len()
                    && after_idx + len < after.len()
                    && !after_used[after_idx + len]
                    && before[before_idx + len] == after[after_idx + len]
                {
                    after_used[after_idx + len] = true;
                    len += 1;
                }

                matches.push((before_idx, after_idx, len));
                before_idx += len;
                continue;
            }
        }

        before_idx += 1;
    }

    // Sort by position in before
    matches.sort_by_key(|m| m.0);
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_matching_blocks() {
        let before: Vec<String> = vec!["a", "b", "c", "d"]
            .into_iter()
            .map(String::from)
            .collect();
        let after: Vec<String> = vec!["a", "x", "c", "d"]
            .into_iter()
            .map(String::from)
            .collect();

        let matches = find_matching_blocks(&before, &after);
        // Should find "a" at (0,0,1) and "c","d" at (2,2,2)
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0], (0, 0, 1));
        assert_eq!(matches[1], (2, 2, 2));
    }

    #[test]
    fn test_compute_connections() {
        let before = Some(FileContent::Text {
            lines: vec!["a".into(), "b".into(), "c".into()],
        });
        let after = Some(FileContent::Text {
            lines: vec!["a".into(), "x".into(), "c".into()],
        });

        let connections = compute_connections(&before, &after);
        // Should have connections for: "a" (unchanged), "b"->"x" (changed), "c" (unchanged)
        assert!(!connections.is_empty());
    }
}
