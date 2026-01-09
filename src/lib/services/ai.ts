// AI description service - calls goose to describe code changes

import { invoke } from '@tauri-apps/api/core';

/** Structured description of a code change */
export interface HunkDescription {
  before: string;
  after: string;
}

/**
 * Describe a code change using goose AI.
 *
 * @param filePath - Path to the file being changed
 * @param beforeLines - Lines before the change
 * @param afterLines - Lines after the change
 * @returns Before/after descriptions of the change
 */
export async function describeHunk(
  filePath: string,
  beforeLines: string[],
  afterLines: string[]
): Promise<HunkDescription> {
  return invoke<HunkDescription>('describe_hunk', {
    filePath,
    beforeLines,
    afterLines,
  });
}
