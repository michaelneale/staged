<!--
  FileViewer.svelte - Single file display
  
  A simplified viewer for displaying a single file without diff comparison.
  Used for:
  - Created files (no "before" to compare against)
  - Deleted files (no "after" to compare against)
  - Future: browsing files without changes
  
  Shows a status badge indicating why we're in single-file mode.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { GitBranch } from 'lucide-svelte';
  import type { FileDiff } from './types';
  import {
    initHighlighter,
    highlightLines,
    detectLanguage,
    prepareLanguage,
    type Token,
  } from './services/highlighter';
  import { getLanguageFromDiff, getFilePath, isBinaryDiff, getTextLines } from './diffUtils';
  import { setupKeyboardNav } from './diffKeyboard';
  import { WORKDIR } from './stores/diffSelection.svelte';

  type FileStatus = 'created' | 'deleted' | 'unchanged';

  interface Props {
    diff: FileDiff;
    /** Base ref for the diff */
    diffBase?: string;
    /** Head ref for the diff */
    diffHead?: string;
    /** Bumped when syntax theme changes to trigger re-highlight */
    syntaxThemeVersion?: number;
  }

  let { diff, diffBase = 'HEAD', diffHead = WORKDIR, syntaxThemeVersion = 0 }: Props = $props();

  let container: HTMLDivElement | null = $state(null);
  let highlighterReady = $state(false);
  let languageReady = $state(false);

  // Pre-computed tokens for all lines
  let tokens: Token[][] = $state([]);

  // Determine file status
  let status: FileStatus = $derived.by(() => {
    if (diff.before === null) return 'created';
    if (diff.after === null) return 'deleted';
    return 'unchanged';
  });

  // Get the appropriate ref label and content based on status
  let refLabel = $derived(
    status === 'deleted' ? diffBase : diffHead === WORKDIR ? 'Working Tree' : diffHead
  );
  let filePath = $derived(getFilePath(diff));
  let lines = $derived(
    status === 'deleted' ? getTextLines(diff, 'before') : getTextLines(diff, 'after')
  );
  let isBinary = $derived(isBinaryDiff(diff));

  // Detect language for syntax highlighting
  let language = $derived(getLanguageFromDiff(diff, detectLanguage));

  // Label for indicator strip
  let statusLabel = $derived(
    status === 'created' ? 'Created' : status === 'deleted' ? 'Deleted' : 'No Changes'
  );

  // Pre-compute all tokens when diff, language, or syntax theme changes
  $effect(() => {
    const _version = syntaxThemeVersion;

    if (highlighterReady && languageReady) {
      const code = lines.join('\n');
      tokens = code ? highlightLines(code, language) : [];
    } else {
      // Fallback: plain text tokens
      tokens = lines.map((line) => [{ content: line, color: 'inherit' }]);
    }
  });

  // Prepare language when highlighter is ready
  $effect(() => {
    if (highlighterReady && diff) {
      languageReady = false;
      const path = getFilePath(diff);
      if (path) {
        prepareLanguage(path).then((ready) => {
          languageReady = ready;
        });
      }
    }
  });

  /**
   * Handle copy event to properly include newlines between lines.
   */
  function handleCopy(event: ClipboardEvent) {
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed) return;

    // Check if selection is within our code container
    const range = selection.getRangeAt(0);
    const ancestor = range.commonAncestorContainer;
    const codeContainer = (
      ancestor instanceof Element ? ancestor : ancestor.parentElement
    )?.closest('.code-container');

    if (!codeContainer || !container?.contains(codeContainer)) return;

    // Get all selected line elements
    const lines: string[] = [];
    const lineElements = codeContainer.querySelectorAll('.line');

    for (const lineEl of lineElements) {
      if (selection.containsNode(lineEl, true)) {
        const contentEl = lineEl.querySelector('.line-content');
        if (contentEl) {
          lines.push(contentEl.textContent || '');
        }
      }
    }

    if (lines.length > 0) {
      event.preventDefault();
      const text = lines.join('\n');
      event.clipboardData?.setData('text/plain', text);
    }
  }

  onMount(() => {
    initHighlighter().then(() => {
      highlighterReady = true;
    });

    // Arrow keys and Ctrl+N/P for scrolling
    const cleanupKeyboardNav = setupKeyboardNav({
      getScrollTarget: () => container,
    });

    // Copy handler for proper newline handling
    document.addEventListener('copy', handleCopy);

    return () => {
      cleanupKeyboardNav?.();
      document.removeEventListener('copy', handleCopy);
    };
  });
</script>

<div class="file-viewer">
  {#if isBinary}
    <div class="binary-notice">
      <p>Binary file - cannot display content</p>
    </div>
  {:else}
    <!-- Created: empty space on left (no "before") -->
    {#if status === 'created'}
      <div class="empty-side left">
        <span class="empty-side-label">{statusLabel}</span>
      </div>
    {/if}

    <div class="file-panel">
      <div class="panel-header">
        <span class="panel-ref">
          <GitBranch size={12} />
          {refLabel}
        </span>
        <span class="panel-path" title={filePath}>{filePath ?? 'No file'}</span>
      </div>

      <div class="code-container" bind:this={container}>
        <div class="lines-wrapper">
          {#each tokens as lineTokens}
            <div class="line">
              <span class="line-content">
                {#each lineTokens as token}
                  <span style="color: {token.color}">{token.content}</span>
                {/each}
              </span>
            </div>
          {/each}
          {#if tokens.length === 0}
            <div class="empty-notice">
              <span class="empty-label">Empty file</span>
            </div>
          {/if}
        </div>
      </div>
    </div>

    <!-- Deleted: empty space on right (no "after") -->
    {#if status === 'deleted'}
      <div class="empty-side right">
        <span class="empty-side-label">{statusLabel}</span>
      </div>
    {/if}

    <!-- Unchanged: empty space on left (viewing current state) -->
    {#if status === 'unchanged'}
      <div class="empty-side left">
        <span class="empty-side-label">{statusLabel}</span>
      </div>
    {/if}
  {/if}
</div>

<style>
  .file-viewer {
    display: flex;
    flex-direction: row;
    height: 100%;
    overflow: hidden;
    /* Match diff-content padding */
    padding-left: 8px;
  }

  /* Empty side - represents the missing before/after */
  .empty-side {
    display: flex;
    align-items: center;
    width: 120px;
    flex-shrink: 0;
  }

  /* Left side: align label to right edge (next to panel) */
  .empty-side.left {
    justify-content: flex-end;
    padding-right: 12px;
  }

  /* Right side: align label to left edge (next to panel) */
  .empty-side.right {
    justify-content: flex-start;
    padding-left: 12px;
  }

  .empty-side-label {
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Courier New', monospace;
    font-size: var(--size-lg);
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    writing-mode: vertical-rl;
    text-orientation: mixed;
  }

  /* Left side (created/unchanged): first letter at bottom, rotate so text reads bottom-to-top */
  .empty-side.left .empty-side-label {
    transform: rotate(180deg);
    color: var(--status-added);
  }

  /* Right side (deleted): first letter at top */
  .empty-side.right .empty-side-label {
    color: var(--status-deleted);
  }

  .file-panel {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
    border-radius: 12px;
    background-color: var(--bg-primary);
    min-width: 0;
  }

  /* Header - ref and path */
  .panel-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-subtle);
  }

  .panel-ref {
    display: flex;
    align-items: center;
    gap: 4px;
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Courier New', monospace;
    font-size: var(--size-xs);
    color: var(--text-faint);
    flex-shrink: 0;
  }

  .panel-path {
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Courier New', monospace;
    font-size: var(--size-sm);
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .code-container {
    flex: 1;
    overflow: auto;
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Courier New', monospace;
    font-size: var(--size-md);
    line-height: 1.5;
    min-width: 0;
    scrollbar-width: thin;
    scrollbar-color: var(--scrollbar-thumb) transparent;
  }

  .code-container::-webkit-scrollbar {
    width: 8px;
    height: 8px;
  }

  .code-container::-webkit-scrollbar-track {
    background: transparent;
  }

  .code-container::-webkit-scrollbar-thumb {
    background: var(--scrollbar-thumb);
    border-radius: 4px;
  }

  .code-container::-webkit-scrollbar-thumb:hover {
    background: var(--scrollbar-thumb-hover);
  }

  /* Wrapper sizes to longest line */
  .lines-wrapper {
    display: inline-block;
    min-width: 100%;
  }

  .line {
    display: flex;
    min-height: calc(var(--size-md) * 1.5);
  }

  .line-content {
    flex: 1;
    padding: 0 12px;
    white-space: pre;
  }

  /* Empty file notice */
  .empty-notice {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    min-height: 200px;
  }

  .empty-label {
    color: var(--text-faint);
    font-size: var(--size-sm);
    font-style: italic;
  }

  .binary-notice {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    font-size: var(--size-lg);
    background-color: var(--bg-primary);
    border-radius: 12px;
  }
</style>
