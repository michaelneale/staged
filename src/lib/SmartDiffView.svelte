<!-- SmartDiffView.svelte - Natural language diff view -->
<script lang="ts">
  import { Loader2 } from 'lucide-svelte';
  import type { FileDiff } from './types';
  import { smartDiffState } from './stores/smartDiff.svelte';
  import { onMount } from 'svelte';

  interface Props {
    diff: FileDiff | null;
  }

  let { diff }: Props = $props();

  let description = $state<{ before: string; after: string } | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // Calculate description when diff changes
  $effect(() => {
    if (diff) {
      loadDescription();
    }
  });

  async function loadDescription() {
    if (!diff) return;

    loading = true;
    error = null;

    try {
      const result = await smartDiffState.calculateForFile(diff);
      description = result;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  let filePath = $derived(diff?.after?.path ?? diff?.before?.path ?? 'Unknown file');
</script>

<div class="smart-diff-view">
  <div class="smart-diff-header">
    <span class="file-path">{filePath}</span>
  </div>

  {#if loading}
    <div class="smart-diff-loading">
      <Loader2 size={24} class="spinning" />
      <span>Analyzing changes...</span>
    </div>
  {:else if error}
    <div class="smart-diff-error">
      <span>Failed to analyze: {error}</span>
    </div>
  {:else if description}
    <div class="smart-diff-content">
      <div class="smart-diff-pane before">
        <div class="pane-label">Before</div>
        <div class="pane-description">{description.before}</div>
      </div>
      <div class="smart-diff-divider"></div>
      <div class="smart-diff-pane after">
        <div class="pane-label">After</div>
        <div class="pane-description">{description.after}</div>
      </div>
    </div>
  {:else}
    <div class="smart-diff-empty">
      <span>No description available</span>
    </div>
  {/if}
</div>

<style>
  .smart-diff-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-primary);
    border-radius: 8px;
    overflow: hidden;
  }

  .smart-diff-header {
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-chrome);
  }

  .file-path {
    font-family: 'SF Mono', 'Menlo', monospace;
    font-size: var(--size-sm);
    color: var(--text-primary);
  }

  .smart-diff-loading,
  .smart-diff-error,
  .smart-diff-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    flex: 1;
    color: var(--text-muted);
    font-size: var(--size-sm);
  }

  .smart-diff-loading :global(svg) {
    animation: spin 1s linear infinite;
    color: var(--accent-primary);
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }

  .smart-diff-error {
    color: var(--status-deleted);
  }

  .smart-diff-content {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .smart-diff-pane {
    flex: 1;
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .smart-diff-pane.before {
    background: color-mix(in srgb, var(--status-deleted) 5%, var(--bg-primary));
  }

  .smart-diff-pane.after {
    background: color-mix(in srgb, var(--status-added) 5%, var(--bg-primary));
  }

  .pane-label {
    font-size: var(--size-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .before .pane-label {
    color: var(--status-deleted);
  }

  .after .pane-label {
    color: var(--status-added);
  }

  .pane-description {
    font-size: var(--size-base);
    line-height: 1.6;
    color: var(--text-primary);
  }

  .smart-diff-divider {
    width: 1px;
    background: var(--border-muted);
  }
</style>
