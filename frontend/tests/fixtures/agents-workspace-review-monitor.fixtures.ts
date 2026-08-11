import type {
  AgentConversationWorkspace,
  AgentWorkspaceReviewContext,
} from "@/api/chat";

export type WorkspaceReviewVisualState = "running" | "blocking" | "passed";

export function buildWorkspaceReviewMonitorFixture(
  workspace: AgentConversationWorkspace,
  conversationId: string,
  state: WorkspaceReviewVisualState,
) {
  const now = "2026-05-13T05:15:00.000Z";
  const isRunning = state === "running";
  const isBlocking = state === "blocking";
  const artifactId = isRunning ? null : `workspace-review-${state}-visual`;
  const isCurrent = !isRunning;
  const diffFingerprint = "visual-review-diff-fingerprint";
  const monitor: AgentWorkspaceReviewContext["monitor"] = {
    conversationId,
    projectId: workspace.projectId,
    status: isRunning ? "reviewing" : "ready",
    reviewOutcome: isBlocking ? "blocking" : state === "passed" ? "passed" : "none",
    reviewGateStatus: isRunning ? "reviewing" : isBlocking ? "blocking" : "passed",
    currentTargetScope: "workspace_delta",
    reviewedTargetScope: isCurrent ? "workspace_delta" : null,
    reviewConversationId: null,
    reviewArtifactId: artifactId,
    reviewArtifactVersion: artifactId ? 3 : null,
    reviewArtifactUpdatedAt: artifactId ? now : null,
    reviewGateBypassedAt: null,
    reviewGateBypassedTargetScope: null,
    reviewGateBypassedDiffFingerprint: null,
    reviewGateBypassedArtifactId: null,
    reviewGateBypassedArtifactVersion: null,
    reviewedHeadSha: isCurrent ? "visual-head-sha" : null,
    reviewedDiffFingerprint: isCurrent ? diffFingerprint : null,
    selectedSourceBaseRef: null,
    selectedSourceBaseSha: null,
    selectedSourceHeadRef: null,
    selectedSourceHeadSha: null,
    selectedSourcePullRequestNumber: null,
    workspaceBaseRef: workspace.baseBranch ?? "main",
    workspaceBaseSha: "visual-base-sha",
    workspaceHeadRef: workspace.branchName,
    workspaceHeadSha: "visual-head-sha",
    currentDiffFingerprint: diffFingerprint,
    previousVersionId: null,
    reviewBlockingSummary: isBlocking
      ? "Two release-safety issues must be fixed before publishing."
      : null,
    reviewBlockingFingerprint: isBlocking ? "visual-blocking-fingerprint" : null,
    reviewFixerRunId: null,
    reviewFixerConversationId: null,
    reviewFixerStatus: null,
    lastRunId: "visual-review-run",
    lastError: null,
    autoMergeGuardStatus: null,
    autoMergeGuardPrNumber: null,
    autoMergeGuardMethod: null,
    autoMergeGuardTargetScope: null,
    autoMergeGuardDiffFingerprint: null,
    autoMergeGuardHeadSha: null,
    autoMergeGuardLastError: null,
    createdAt: now,
    updatedAt: now,
  };
  return { artifactId, diffFingerprint, isBlocking, isCurrent, monitor, now };
}
