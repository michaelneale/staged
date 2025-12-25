/**
 * Status event subscription service.
 *
 * Listens for backend events and forwards them to callbacks.
 * The frontend is intentionally dumb - all throttling/timing logic
 * lives in the backend.
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { GitStatus } from '../types';

/** Callback for status updates */
export type StatusUpdateCallback = (status: GitStatus) => void;

/** Callback for slow repo detection (one-time) */
export type SlowRepoCallback = () => void;

/** Cleanup function returned by subscribe */
export type Unsubscribe = () => void;

// Active listeners
let statusUnlisten: UnlistenFn | null = null;
let slowRepoUnlisten: UnlistenFn | null = null;

/**
 * Subscribe to status update events from the backend.
 *
 * @param onStatus - Called whenever the git status changes
 * @param onSlowRepo - Optional, called once if the repo is detected as slow
 * @returns Cleanup function to unsubscribe
 */
export async function subscribeToStatusEvents(
  onStatus: StatusUpdateCallback,
  onSlowRepo?: SlowRepoCallback
): Promise<Unsubscribe> {
  // Clean up any existing listeners first
  await unsubscribeAll();

  // Listen for status updates
  statusUnlisten = await listen<GitStatus>('status-updated', (event) => {
    onStatus(event.payload);
  });

  // Optionally listen for slow repo notification
  if (onSlowRepo) {
    slowRepoUnlisten = await listen('slow-repo-detected', () => {
      onSlowRepo();
    });
  }

  return unsubscribeAll;
}

/**
 * Unsubscribe from all status events.
 */
async function unsubscribeAll(): Promise<void> {
  if (statusUnlisten) {
    statusUnlisten();
    statusUnlisten = null;
  }
  if (slowRepoUnlisten) {
    slowRepoUnlisten();
    slowRepoUnlisten = null;
  }
}

/**
 * Start watching a repository for changes.
 * The backend will emit 'status-updated' events when files change.
 */
export async function startWatching(repoPath: string): Promise<void> {
  await invoke('start_watching', { repoPath });
}

/**
 * Stop watching the current repository.
 */
export async function stopWatching(): Promise<void> {
  await invoke('stop_watching');
}

/**
 * Force an immediate status refresh, bypassing throttle.
 * Returns the new status directly.
 */
export async function forceRefresh(): Promise<GitStatus> {
  return invoke<GitStatus>('force_refresh');
}
