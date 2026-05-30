import type { FileChange } from "@/api/diff";

export const INLINE_DIFF_SHOW_ANYWAY_CHANGE_THRESHOLD = 1_000;

export function isLargeInlineDiff(
  file: Pick<FileChange, "additions" | "deletions">,
): boolean {
  return file.additions + file.deletions >= INLINE_DIFF_SHOW_ANYWAY_CHANGE_THRESHOLD;
}

export function requiresExplicitDiffHydration(
  file: Pick<FileChange, "additions" | "deletions" | "isGenerated">,
): boolean {
  return file.isGenerated;
}
