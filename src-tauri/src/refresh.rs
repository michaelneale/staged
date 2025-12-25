//! Refresh controller that orchestrates file watching and status updates.
//!
//! This module ties together the watcher and status provider, handling:
//! - Throttling (don't refresh too frequently)
//! - Adaptive timing (slow repos get longer intervals)
//! - Slow repo detection and notification
//!
//! All policy decisions live here, making them easy to modify or remove.

use crate::git::provider::StatusProvider;
use crate::git::{AdaptiveProvider, GitStatus};
use crate::watcher::{NotifyWatcher, WatcherManager};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Event name for status updates sent to frontend
pub const EVENT_STATUS_UPDATED: &str = "status-updated";

/// Event name for slow repo detection (one-time notification)
pub const EVENT_SLOW_REPO: &str = "slow-repo-detected";

/// Threshold above which we consider a repo "slow" and notify the user
const SLOW_REPO_THRESHOLD_MS: u64 = 1000;

/// Minimum interval between refreshes (1 second)
const MIN_THROTTLE_INTERVAL_MS: u64 = 1000;

/// State shared between the watcher callback and the controller
struct RefreshState {
    last_refresh: Instant,
    last_duration: Duration,
    slow_notification_sent: bool,
    repo_path: Option<PathBuf>,
}

impl Default for RefreshState {
    fn default() -> Self {
        Self {
            last_refresh: Instant::now() - Duration::from_secs(10), // Allow immediate first refresh
            last_duration: Duration::ZERO,
            slow_notification_sent: false,
            repo_path: None,
        }
    }
}

/// Orchestrates file watching, status fetching, and event emission.
///
/// Owns the watcher and provider, and contains all throttling/policy logic.
/// Easy to modify behavior by changing this struct.
pub struct RefreshController {
    watcher: Mutex<NotifyWatcher>,
    provider: Arc<AdaptiveProvider>,
    state: Arc<Mutex<RefreshState>>,
    app_handle: AppHandle,
}

impl RefreshController {
    /// Create a new refresh controller.
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            watcher: Mutex::new(NotifyWatcher::new()),
            provider: Arc::new(AdaptiveProvider::default()),
            state: Arc::new(Mutex::new(RefreshState::default())),
            app_handle,
        }
    }

    /// Start watching a repository for changes.
    /// Stops any existing watcher first.
    pub fn start(&self, repo_path: PathBuf) -> Result<(), String> {
        // Reset state for new repo
        {
            let mut state = self.state.lock().unwrap();
            *state = RefreshState::default();
            state.repo_path = Some(repo_path.clone());
        }

        // Reset provider (may have switched to CLI for previous repo)
        self.provider.reset();

        // Set up the callback that will be called on FS changes
        let state = Arc::clone(&self.state);
        let provider = Arc::clone(&self.provider);
        let app_handle = self.app_handle.clone();

        let on_change = Box::new(move || {
            Self::handle_change(&state, &provider, &app_handle);
        });

        // Start the watcher
        let mut watcher = self.watcher.lock().unwrap();
        watcher
            .start(&repo_path, on_change)
            .map_err(|e| e.message)?;

        // Do an initial refresh immediately
        Self::handle_change(&self.state, &self.provider, &self.app_handle);

        Ok(())
    }

    /// Stop watching the current repository.
    pub fn stop(&self) {
        let mut watcher = self.watcher.lock().unwrap();
        watcher.stop();

        let mut state = self.state.lock().unwrap();
        state.repo_path = None;
    }

    /// Handle a file system change event.
    /// This is called by the watcher when relevant files change.
    fn handle_change(
        state: &Arc<Mutex<RefreshState>>,
        provider: &Arc<AdaptiveProvider>,
        app_handle: &AppHandle,
    ) {
        let repo_path = {
            let state = state.lock().unwrap();
            match &state.repo_path {
                Some(p) => p.clone(),
                None => return, // No repo to refresh
            }
        };

        // Check throttle
        {
            let state = state.lock().unwrap();
            let throttle_interval = Self::calculate_throttle_interval(state.last_duration);
            if state.last_refresh.elapsed() < throttle_interval {
                log::debug!(
                    "Throttled: {}ms since last refresh, need {}ms",
                    state.last_refresh.elapsed().as_millis(),
                    throttle_interval.as_millis()
                );
                return;
            }
        }

        // Fetch status
        let result = match provider.get_status(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Failed to get git status: {}", e.message);
                return;
            }
        };

        // Update state
        let should_notify_slow = {
            let mut state = state.lock().unwrap();
            state.last_refresh = Instant::now();
            state.last_duration = result.duration;

            // Check if we should send slow repo notification
            let should_notify = result.duration.as_millis() > SLOW_REPO_THRESHOLD_MS as u128
                && !state.slow_notification_sent;

            if should_notify {
                state.slow_notification_sent = true;
            }

            should_notify
        };

        log::debug!(
            "Status refresh took {}ms (used_cli: {})",
            result.duration.as_millis(),
            result.used_cli
        );

        // Emit status update to frontend
        if let Err(e) = app_handle.emit(EVENT_STATUS_UPDATED, &result.status) {
            log::error!("Failed to emit status update: {}", e);
        }

        // Emit slow repo notification (one-time)
        if should_notify_slow {
            log::info!(
                "Slow repository detected ({}ms), notifying user",
                result.duration.as_millis()
            );
            if let Err(e) = app_handle.emit(EVENT_SLOW_REPO, ()) {
                log::error!("Failed to emit slow repo notification: {}", e);
            }
        }
    }

    /// Calculate the throttle interval based on last refresh duration.
    /// Returns at least MIN_THROTTLE_INTERVAL_MS, or 1.5x the last duration if longer.
    fn calculate_throttle_interval(last_duration: Duration) -> Duration {
        let min_interval = Duration::from_millis(MIN_THROTTLE_INTERVAL_MS);
        let adaptive_interval = last_duration.mul_f32(1.5);
        min_interval.max(adaptive_interval)
    }

    /// Force an immediate refresh, bypassing throttle.
    /// Used for manual refresh button.
    pub fn force_refresh(&self) -> Result<GitStatus, String> {
        let repo_path = {
            let state = self.state.lock().unwrap();
            state.repo_path.clone()
        };

        let repo_path = repo_path.ok_or_else(|| "No repository is being watched".to_string())?;

        let result = self
            .provider
            .get_status(&repo_path)
            .map_err(|e| e.message)?;

        // Update timing state
        {
            let mut state = self.state.lock().unwrap();
            state.last_refresh = Instant::now();
            state.last_duration = result.duration;
        }

        // Emit to frontend
        if let Err(e) = self.app_handle.emit(EVENT_STATUS_UPDATED, &result.status) {
            log::error!("Failed to emit status update: {}", e);
        }

        Ok(result.status)
    }
}
