import type { DiffRefKind, FileChange } from "@/api/diff";

export function requiresExplicitDiffHydration(
  file: Pick<FileChange, "additions" | "deletions" | "isGenerated">,
): boolean {
  return file.isGenerated;
}

export function canUsePagedInlineDiff({
  file,
  isConflictMode,
  conversationId,
  diffPageRefKind,
  isShowAnywayOverridden,
}: {
  file: Pick<FileChange, "additions" | "deletions" | "isGenerated">;
  isConflictMode: boolean;
  conversationId?: string | undefined;
  diffPageRefKind?: DiffRefKind | undefined;
  isShowAnywayOverridden: boolean;
}): boolean {
  return (
    !isConflictMode &&
    conversationId !== undefined &&
    diffPageRefKind !== undefined &&
    (!requiresExplicitDiffHydration(file) || isShowAnywayOverridden)
  );
}
