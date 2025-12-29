//! File system watcher for detecting repository changes.
//!
//! Watches the repository and emits events when files change.
//! The frontend can then re-fetch the diff as needed.
//!
//! Design:
//! - Uses `notify` with debouncing to coalesce rapid changes
//! - Uses `ignore` crate to respect .gitignore
//! - Emits simple "files changed" events (no git status computation)
//! - Throttles to prevent overwhelming the frontend

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ignore::WalkBuilder;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebouncedEvent, Debouncer, RecommendedCache};
use tauri::{AppHandle, Emitter};

/// Event emitted when files change in the repository.
pub const EVENT_FILES_CHANGED: &str = "files-changed";

/// Minimum interval between emitting change events (prevents flooding).
const MIN_EMIT_INTERVAL: Duration = Duration::from_millis(500);

/// Debounce timeout for file system events.
const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(300);

/// Watches a repository for file changes and emits events to the frontend.
pub struct RepoWatcher {
    inner: Mutex<WatcherInner>,
    app_handle: AppHandle,
}

struct WatcherInner {
    debouncer: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
    watched_paths: HashSet<PathBuf>,
    repo_path: Option<PathBuf>,
    last_emit: Instant,
}

impl RepoWatcher {
    /// Create a new repository watcher.
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            inner: Mutex::new(WatcherInner {
                debouncer: None,
                watched_paths: HashSet::new(),
                repo_path: None,
                last_emit: Instant::now() - Duration::from_secs(10), // Allow immediate first emit
            }),
            app_handle,
        }
    }

    /// Start watching a repository.
    ///
    /// Stops any existing watcher first. Emits `files-changed` events
    /// when relevant files are modified.
    pub fn start(&self, repo_path: PathBuf) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();

        // Stop existing watcher
        Self::stop_inner(&mut inner);

        let repo_path_for_filter = repo_path.clone();
        let app_handle = self.app_handle.clone();
        let last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10)));
        let last_emit_clone = Arc::clone(&last_emit);

        // Create debouncer
        let mut debouncer = new_debouncer(
            DEBOUNCE_TIMEOUT,
            None,
            move |result: Result<Vec<DebouncedEvent>, Vec<notify::Error>>| {
                match result {
                    Ok(events) => {
                        // Collect changed paths
                        let changed: Vec<String> = events
                            .iter()
                            .flat_map(|e| e.paths.iter())
                            .filter(|p| should_trigger_refresh(p, &repo_path_for_filter))
                            .filter_map(|p| {
                                p.strip_prefix(&repo_path_for_filter)
                                    .ok()
                                    .map(|rel| rel.to_string_lossy().to_string())
                            })
                            .collect::<HashSet<_>>() // Dedupe
                            .into_iter()
                            .collect();

                        if changed.is_empty() {
                            return;
                        }

                        // Throttle emissions
                        {
                            let mut last = last_emit_clone.lock().unwrap();
                            if last.elapsed() < MIN_EMIT_INTERVAL {
                                log::debug!("Throttled file change event");
                                return;
                            }
                            *last = Instant::now();
                        }

                        log::debug!("Files changed: {:?}", changed);

                        // Emit event to frontend
                        if let Err(e) = app_handle.emit(EVENT_FILES_CHANGED, &changed) {
                            log::error!("Failed to emit files-changed event: {}", e);
                        }
                    }
                    Err(errors) => {
                        for e in errors {
                            log::warn!("Watcher error: {}", e);
                        }
                    }
                }
            },
        )
        .map_err(|e| format!("Failed to create watcher: {}", e))?;

        // Collect directories to watch (respecting .gitignore)
        let mut dirs_to_watch: HashSet<PathBuf> = HashSet::new();
        dirs_to_watch.insert(repo_path.clone());

        let walker = WalkBuilder::new(&repo_path)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .ignore(true)
            .parents(true)
            .build();

        for entry in walker.flatten() {
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                dirs_to_watch.insert(entry.path().to_path_buf());
            }
        }

        // Watch each directory
        for dir in &dirs_to_watch {
            if let Err(e) = debouncer.watch(dir, RecursiveMode::NonRecursive) {
                log::warn!("Failed to watch {}: {}", dir.display(), e);
            }
        }

        // Also watch .git directory for index/HEAD changes
        let git_dir = repo_path.join(".git");
        if git_dir.exists() {
            if let Err(e) = debouncer.watch(&git_dir, RecursiveMode::Recursive) {
                log::warn!("Failed to watch .git: {}", e);
            }
            dirs_to_watch.insert(git_dir);
        }

        inner.debouncer = Some(debouncer);
        inner.watched_paths = dirs_to_watch;
        inner.repo_path = Some(repo_path.clone());
        inner.last_emit = *last_emit.lock().unwrap();

        log::info!("Started watching repository: {}", repo_path.display());
        Ok(())
    }

    /// Stop watching the current repository.
    pub fn stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        Self::stop_inner(&mut inner);
    }

    fn stop_inner(inner: &mut WatcherInner) {
        if let Some(mut debouncer) = inner.debouncer.take() {
            for path in &inner.watched_paths {
                let _ = debouncer.unwatch(path);
            }
            log::info!("Stopped watching repository");
        }
        inner.watched_paths.clear();
        inner.repo_path = None;
    }

    /// Get the currently watched repository path.
    pub fn repo_path(&self) -> Option<PathBuf> {
        self.inner.lock().unwrap().repo_path.clone()
    }
}

/// Determine if a file change should trigger a refresh event.
fn should_trigger_refresh(path: &Path, repo_root: &Path) -> bool {
    let relative = match path.strip_prefix(repo_root) {
        Ok(rel) => rel,
        Err(_) => return false,
    };

    let path_str = relative.to_string_lossy();

    // Key .git files that indicate state changes
    if path_str == ".git/index" || path_str == ".git/HEAD" || path_str.starts_with(".git/refs/") {
        return true;
    }

    // Ignore internal git files
    if path_str.starts_with(".git/") {
        return false;
    }

    // Ignore common build/dependency directories
    // (These should also be in .gitignore, but belt and suspenders)
    let ignored_prefixes = [
        "node_modules/",
        "target/",
        ".build/",
        "build/",
        "dist/",
        ".next/",
        "__pycache__/",
        ".pytest_cache/",
        "venv/",
        ".venv/",
    ];

    for prefix in ignored_prefixes {
        if path_str.starts_with(prefix) {
            return false;
        }
    }

    // Ignore common temporary files
    let ignored_suffixes = [
        ".pyc", ".pyo", ".class", ".o", ".a", ".so", ".dylib", "~", ".swp", ".swo", ".lock",
    ];

    for suffix in ignored_suffixes {
        if path_str.ends_with(suffix) {
            return false;
        }
    }

    // Ignore .DS_Store
    if path_str.contains(".DS_Store") {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_trigger_refresh() {
        let repo = Path::new("/repo");

        // Should trigger
        assert!(should_trigger_refresh(Path::new("/repo/src/main.rs"), repo));
        assert!(should_trigger_refresh(Path::new("/repo/.git/index"), repo));
        assert!(should_trigger_refresh(Path::new("/repo/.git/HEAD"), repo));
        assert!(should_trigger_refresh(
            Path::new("/repo/.git/refs/heads/main"),
            repo
        ));

        // Should NOT trigger
        assert!(!should_trigger_refresh(
            Path::new("/repo/.git/objects/ab/cdef"),
            repo
        ));
        assert!(!should_trigger_refresh(
            Path::new("/repo/node_modules/foo.js"),
            repo
        ));
        assert!(!should_trigger_refresh(
            Path::new("/repo/target/debug/foo"),
            repo
        ));
        assert!(!should_trigger_refresh(Path::new("/repo/foo.pyc"), repo));
        assert!(!should_trigger_refresh(
            Path::new("/repo/.git/index.lock"),
            repo
        ));
    }
}
