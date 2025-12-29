//! Git actions for modifying the working tree.
//!
//! These are imperative operations that mutate git state.

use std::path::Path;

use git2::{Repository, Signature};

use super::git::GitError;
use super::types::Connection;

type Result<T> = std::result::Result<T, GitError>;

/// Discard all changes to a file, reverting it to the base ref.
pub fn discard_file(repo: &Repository, path: &str, base_ref: &str) -> Result<()> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError("Bare repository".into()))?;
    let file_path = workdir.join(path);

    // Get the content from the base ref
    let obj = repo.revparse_single(base_ref)?;
    let commit = obj.peel_to_commit()?;
    let tree = commit.tree()?;

    match tree.get_path(Path::new(path)) {
        Ok(entry) => {
            // File exists in base - restore it
            let obj = entry.to_object(repo)?;
            let blob = obj.as_blob().ok_or_else(|| GitError("Not a file".into()))?;
            std::fs::write(&file_path, blob.content())
                .map_err(|e| GitError(format!("Cannot write file: {}", e)))?;
        }
        Err(_) => {
            // File doesn't exist in base - delete it
            if file_path.exists() {
                std::fs::remove_file(&file_path)
                    .map_err(|e| GitError(format!("Cannot delete file: {}", e)))?;
            }
        }
    }

    Ok(())
}

/// Discard a specific region of changes in a file.
///
/// This is more complex: we need to reconstruct the file with the
/// specified region reverted to its base state.
pub fn discard_region(
    repo: &Repository,
    path: &str,
    base_ref: &str,
    connection: &Connection,
) -> Result<()> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError("Bare repository".into()))?;
    let file_path = workdir.join(path);

    // Read current file content
    let current_content = std::fs::read_to_string(&file_path)
        .map_err(|e| GitError(format!("Cannot read file: {}", e)))?;
    let current_lines: Vec<&str> = current_content.lines().collect();

    // Get base content
    let obj = repo.revparse_single(base_ref)?;
    let commit = obj.peel_to_commit()?;
    let tree = commit.tree()?;
    let entry = tree.get_path(Path::new(path))?;
    let obj = entry.to_object(repo)?;
    let blob = obj.as_blob().ok_or_else(|| GitError("Not a file".into()))?;
    let base_content = String::from_utf8_lossy(blob.content());
    let base_lines: Vec<&str> = base_content.lines().collect();

    // Reconstruct the file:
    // - Lines before the region: keep from current
    // - The region itself: take from base
    // - Lines after the region: keep from current
    let mut result = Vec::new();

    // Lines before the changed region in current
    let after_start = connection.after.start as usize;
    let after_end = connection.after.end as usize;
    result.extend_from_slice(&current_lines[..after_start]);

    // The region from base
    let before_start = connection.before.start as usize;
    let before_end = connection.before.end as usize;
    if before_end <= base_lines.len() {
        result.extend_from_slice(&base_lines[before_start..before_end]);
    }

    // Lines after the changed region in current
    if after_end < current_lines.len() {
        result.extend_from_slice(&current_lines[after_end..]);
    }

    // Write back
    let new_content = result.join("\n");
    // Preserve trailing newline if original had one
    let new_content = if current_content.ends_with('\n') {
        new_content + "\n"
    } else {
        new_content
    };

    std::fs::write(&file_path, new_content)
        .map_err(|e| GitError(format!("Cannot write file: {}", e)))?;

    Ok(())
}

/// Commit the specified files with the given message.
///
/// If `files` is empty, commits all staged changes.
/// Otherwise, stages only the specified files before committing.
pub fn commit(repo: &Repository, message: &str, files: &[String]) -> Result<String> {
    let mut index = repo.index()?;

    if files.is_empty() {
        // Commit whatever is currently staged
    } else {
        // Stage only the specified files
        let workdir = repo
            .workdir()
            .ok_or_else(|| GitError("Bare repository".into()))?;

        for file in files {
            let path = Path::new(file);
            let full_path = workdir.join(path);

            if full_path.exists() {
                index.add_path(path)?;
            } else {
                index.remove_path(path)?;
            }
        }
        index.write()?;
    }

    // Create the commit
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let signature = repo.signature().or_else(|_| {
        // Fallback signature if not configured
        Signature::now("Staged User", "staged@local")
    })?;

    let parent = repo.head()?.peel_to_commit()?;

    let commit_id = repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &[&parent],
    )?;

    Ok(commit_id.to_string())
}

/// Amend the last commit with the specified files and message.
pub fn amend(repo: &Repository, message: &str, files: &[String]) -> Result<String> {
    let mut index = repo.index()?;

    if !files.is_empty() {
        let workdir = repo
            .workdir()
            .ok_or_else(|| GitError("Bare repository".into()))?;

        for file in files {
            let path = Path::new(file);
            let full_path = workdir.join(path);

            if full_path.exists() {
                index.add_path(path)?;
            } else {
                index.remove_path(path)?;
            }
        }
        index.write()?;
    }

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let head = repo.head()?.peel_to_commit()?;
    let signature = repo
        .signature()
        .or_else(|_| Signature::now("Staged User", "staged@local"))?;

    let commit_id = head.amend(
        Some("HEAD"),
        Some(&signature),
        Some(&signature),
        None, // encoding
        Some(message),
        Some(&tree),
    )?;

    Ok(commit_id.to_string())
}

/// Stage a file (add to index).
pub fn stage_file(repo: &Repository, path: &str) -> Result<()> {
    let mut index = repo.index()?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError("Bare repository".into()))?;

    let file_path = Path::new(path);
    let full_path = workdir.join(file_path);

    if full_path.exists() {
        index.add_path(file_path)?;
    } else {
        index.remove_path(file_path)?;
    }

    index.write()?;
    Ok(())
}

/// Unstage a file (remove from index, keep in working tree).
pub fn unstage_file(repo: &Repository, path: &str) -> Result<()> {
    let head = repo.head()?.peel_to_commit()?;
    repo.reset_default(Some(&head.into_object()), [Path::new(path)])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn init_test_repo() -> (tempfile::TempDir, Repository) {
        let dir = tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        // Create initial commit
        {
            let mut index = repo.index().unwrap();
            let file_path = dir.path().join("test.txt");
            fs::write(&file_path, "line 1\nline 2\nline 3\n").unwrap();
            index.add_path(Path::new("test.txt")).unwrap();
            index.write().unwrap();

            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let sig = Signature::now("Test", "test@test.com").unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        (dir, repo)
    }

    #[test]
    fn test_discard_file() {
        let (dir, repo) = init_test_repo();
        let file_path = dir.path().join("test.txt");

        // Modify the file
        fs::write(&file_path, "modified content\n").unwrap();

        // Discard changes
        discard_file(&repo, "test.txt", "HEAD").unwrap();

        // Should be back to original
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line 1\nline 2\nline 3\n");
    }

    #[test]
    fn test_commit() {
        let (dir, repo) = init_test_repo();
        let file_path = dir.path().join("test.txt");

        // Modify and commit
        fs::write(&file_path, "new content\n").unwrap();
        let sha = commit(&repo, "Second commit", &["test.txt".into()]).unwrap();

        assert!(!sha.is_empty());

        // Verify the commit
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().unwrap(), "Second commit");
    }
}
