export interface FileStatus {
  path: string;
  status: 'modified' | 'added' | 'deleted' | 'renamed' | 'typechange' | 'untracked' | 'unknown';
}

// =============================================================================
// Diff types
// =============================================================================

/** Content of a file - either text lines or binary marker */
export type FileContent = { type: 'text'; lines: string[] } | { type: 'binary' };

/** A file with its path and content */
export interface File {
  path: string;
  content: FileContent;
}

/** A contiguous range of lines (0-indexed, exclusive end) */
export interface Span {
  start: number;
  end: number;
}

/** Maps a region in the before file to a region in the after file */
export interface Alignment {
  before: Span;
  after: Span;
  /** True if this region contains changes */
  changed: boolean;
}

/** The diff for a single file between two states */
export interface FileDiff {
  /** File before the change (null if added) */
  before: File | null;
  /** File after the change (null if deleted) */
  after: File | null;
  /** Alignments mapping regions between before/after */
  alignments: Alignment[];
}

// =============================================================================
// Legacy types (to be removed)
// =============================================================================

export interface GitStatus {
  staged: FileStatus[];
  unstaged: FileStatus[];
  untracked: FileStatus[];
  branch: string | null;
  repo_path: string;
}

export interface DiffLine {
  line_type: 'context' | 'added' | 'removed';
  lineno: number;
  content: string;
}

export interface DiffHunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  header: string;
  lines: DiffLine[];
}

/** Half-open interval [start, end) of row indices */
export interface Span {
  start: number;
  end: number;
}

/** Source file line numbers for a changed region (1-indexed, inclusive) */
export interface SourceLines {
  /** Lines removed from the "before" file. None if pure addition */
  old_start: number | null;
  old_end: number | null;
  /** Lines added in the "after" file. None if pure deletion */
  new_start: number | null;
  new_end: number | null;
}

/** Maps corresponding regions between before/after panes */
export interface Range {
  before: Span;
  after: Span;
  /** true = region contains changes, false = identical lines */
  changed: boolean;
  /** Source file line numbers (only present for changed ranges) */
  source_lines?: SourceLines;
}

/** Content for one side of the diff */
export interface LegacyDiffSide {
  path: string | null;
  lines: DiffLine[];
}

/** @deprecated Use FileDiff instead */
export interface LegacyFileDiff {
  status: string;
  is_binary: boolean;
  hunks: DiffHunk[];
  before: LegacyDiffSide;
  after: LegacyDiffSide;
  ranges: Range[];
}

export interface CommitResult {
  oid: string;
  message: string;
}

/** A file that changed between two refs */
export interface ChangedFile {
  path: string;
  status: string;
}

/** A git reference for autocomplete */
export interface GitRef {
  name: string;
  ref_type: 'branch' | 'tag' | 'special';
}

// =============================================================================
// Review types
// =============================================================================

/** Identifies a diff by its two endpoints */
export interface DiffId {
  before: string;
  after: string;
}

/** A diff specification with display label (for UI) */
export interface DiffSpec {
  base: string;
  head: string;
  label: string;
}

/** Where a comment applies */
export type Selection =
  | { type: 'global' }
  | { type: 'line'; line: number }
  | { type: 'range'; span: Span };

/** A comment attached to a specific location in a file */
export interface Comment {
  id: string;
  path: string;
  selection: Selection;
  content: string;
}

/** An edit made during review, stored as a unified diff */
export interface Edit {
  id: string;
  path: string;
  diff: string;
}

/** A review attached to a specific diff */
export interface Review {
  id: DiffId;
  reviewed: string[];
  comments: Comment[];
  edits: Edit[];
}

/** Input for creating a new comment */
export interface NewComment {
  path: string;
  selection: Selection;
  content: string;
}

/** Input for recording a new edit */
export interface NewEdit {
  path: string;
  diff: string;
}
