<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Sidebar, { type FileCategory } from './lib/Sidebar.svelte';
  import DiffViewer from './lib/DiffViewer.svelte';
  import CommitPanel from './lib/CommitPanel.svelte';
  import { getFileDiff, getUntrackedFileDiff } from './lib/services/git';
  import {
    subscribeToStatusEvents,
    startWatching,
    stopWatching,
    type Unsubscribe,
  } from './lib/services/statusEvents';
  import type { FileDiff, GitStatus } from './lib/types';

  let selectedFile: string | null = $state(null);
  let selectedCategory: FileCategory | null = $state(null);
  let currentDiff: FileDiff | null = $state(null);
  let diffLoading = $state(false);
  let diffError: string | null = $state(null);
  let sidebarRef: Sidebar | null = $state(null);
  let commitPanelRef: CommitPanel | null = $state(null);

  // Watcher cleanup function
  let unsubscribe: Unsubscribe | null = null;

  // Current repo path (for watcher)
  let currentRepoPath: string | null = $state(null);

  onMount(async () => {
    // Subscribe to status events from the backend
    unsubscribe = await subscribeToStatusEvents(
      // On status update - forward to sidebar
      (status: GitStatus) => {
        sidebarRef?.setStatus(status);
      },
      // On slow repo detected (optional one-time notification)
      () => {
        console.log(
          'Slow repository detected. Consider enabling git fsmonitor: git config core.fsmonitor true'
        );
        // Could show a toast/notification here in the future
      }
    );
  });

  onDestroy(() => {
    // Clean up watcher and event listeners
    unsubscribe?.();
    stopWatching().catch(() => {
      // Ignore errors on cleanup
    });
  });

  // Called by Sidebar when it loads a repo
  async function handleRepoLoaded(repoPath: string) {
    if (repoPath && repoPath !== currentRepoPath) {
      currentRepoPath = repoPath;
      try {
        await startWatching(repoPath);
        console.log('Started watching:', repoPath);
      } catch (e) {
        console.error('Failed to start watcher:', e);
      }
    }
  }

  async function handleFileSelect(path: string, category: FileCategory) {
    selectedFile = path;
    selectedCategory = category;
    await loadDiff(path, category);
  }

  async function loadDiff(path: string, category: FileCategory) {
    diffLoading = true;
    diffError = null;
    currentDiff = null;

    try {
      if (category === 'untracked') {
        currentDiff = await getUntrackedFileDiff(path);
      } else {
        currentDiff = await getFileDiff(path, category === 'staged');
      }
    } catch (e) {
      diffError = e instanceof Error ? e.message : String(e);
      console.error('Failed to load diff:', e);
    } finally {
      diffLoading = false;
    }
  }

  async function handleStatusChange() {
    // Sidebar staged/unstaged/discarded a file - refresh commit panel
    commitPanelRef?.refresh();

    // If the selected file was discarded, clear the diff
    // The sidebar will handle re-selecting if needed
    if (selectedFile && selectedCategory) {
      try {
        await loadDiff(selectedFile, selectedCategory);
      } catch {
        // File may have been discarded
        currentDiff = null;
        selectedFile = null;
        selectedCategory = null;
      }
    }
  }

  async function handleCommitComplete() {
    // Refresh sidebar and commit panel after successful commit
    await sidebarRef?.loadStatus();
    commitPanelRef?.refresh();
    // Clear the diff view since staged files are now committed
    currentDiff = null;
    selectedFile = null;
    selectedCategory = null;
  }
</script>

<main>
  <div class="app-container">
    <aside class="sidebar">
      <Sidebar
        bind:this={sidebarRef}
        onFileSelect={handleFileSelect}
        onStatusChange={handleStatusChange}
        onRepoLoaded={handleRepoLoaded}
        {selectedFile}
      />
    </aside>
    <section class="main-content">
      {#if diffLoading}
        <div class="loading-state">Loading diff...</div>
      {:else if diffError}
        <div class="error-state">
          <p>Error loading diff:</p>
          <p class="error-message">{diffError}</p>
        </div>
      {:else}
        <DiffViewer diff={currentDiff} />
      {/if}
    </section>
  </div>
  <footer class="commit-panel">
    <CommitPanel bind:this={commitPanelRef} onCommitComplete={handleCommitComplete} />
  </footer>
</main>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
    background-color: var(--bg-primary);
    color: var(--text-primary);
  }

  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }

  .app-container {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .sidebar {
    width: 280px;
    min-width: 200px;
    background-color: var(--bg-secondary);
    border-right: 1px solid var(--border-primary);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .main-content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .loading-state,
  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    font-size: 14px;
  }

  .error-state {
    color: var(--status-deleted);
  }

  .error-message {
    font-family: monospace;
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 8px;
  }

  .commit-panel {
    height: 120px;
    min-height: 80px;
    background-color: var(--bg-secondary);
    border-top: 1px solid var(--border-primary);
  }
</style>
