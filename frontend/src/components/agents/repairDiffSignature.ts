import type {
  AgentWorkspaceChangeBucketSummary,
  AgentWorkspaceChangeSummary,
} from "@/api/diff";

function bucketSignature(bucket: AgentWorkspaceChangeBucketSummary): string {
  return `${bucket.fileCount}:${bucket.additions}:${bucket.deletions}`;
}

export function buildRepairChangeSignature(
  summary: AgentWorkspaceChangeSummary | null | undefined,
): string {
  if (!summary) {
    return "repair:none";
  }

  const conflicted = summary.conflicted;
  const repairState = summary.repairState;
  return [
    `supports:${summary.supportsWorktreeModes === false ? "0" : "1"}`,
    `staged:${bucketSignature(summary.staged)}`,
    `unstaged:${bucketSignature(summary.unstaged)}`,
    `conflicted:${conflicted?.fileCount ?? 0}:${(conflicted?.files ?? []).join("\u001f")}`,
    repairState
      ? [
          "state",
          repairState.expectedBranch,
          repairState.checkedOutBranch,
          repairState.rebaseInProgress ? "rebase" : "no-rebase",
          repairState.mergeInProgress ? "merge" : "no-merge",
        ].join(":")
      : "state:none",
  ].join("|");
}
