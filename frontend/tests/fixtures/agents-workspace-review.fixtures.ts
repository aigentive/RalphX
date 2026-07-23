import { expect, type Locator, type Page } from "@playwright/test";
import type {
  AgentConversationWorkspace,
  AgentWorkspaceReviewContext,
} from "@/api/chat";

import {
  buildWorkspaceReviewMonitorFixture,
  type WorkspaceReviewVisualState,
} from "./agents-workspace-review-monitor.fixtures";

export { type WorkspaceReviewVisualState };

export async function seedWorkspaceReviewState(
  page: Page,
  reviewTab: Locator,
  conversationId: string,
  state: WorkspaceReviewVisualState,
) {
  const workspace = await page.evaluate((targetConversationId) => {
    const queryClient = window.__queryClient;
    const value = queryClient?.getQueryData<AgentConversationWorkspace>([
      "agents",
      "conversation-workspace",
      targetConversationId,
    ]);
    if (!value) throw new Error("Expected workspace for Workspace Review fixture");
    return value;
  }, conversationId);
  const fixture = buildWorkspaceReviewMonitorFixture(
    workspace,
    conversationId,
    state,
  );
  const context: AgentWorkspaceReviewContext = {
    success: true,
    workspace,
    events: [],
    target: {
      scope: "workspace_delta",
      baseRef: workspace.baseBranch ?? "main",
      baseSha: "visual-base-sha",
      headRef: workspace.branchName,
      headSha: "visual-head-sha",
      diffFingerprint: fixture.diffFingerprint,
      sourcePullRequestNumber: workspace.publicationPrNumber,
    },
    monitor: fixture.monitor,
    reviewArtifactIsCurrent: fixture.isCurrent,
    reviewArtifactIsOutdated: false,
    canMutateReviewState: true,
    reviewRuntimeState: state === "running" ? "active_owned" : "terminal",
    isCurrent: fixture.isCurrent,
    isOutdated: false,
    shouldShowTab: true,
  };
  const artifact = fixture.artifactId
    ? {
        id: fixture.artifactId,
        type: "workspace_review",
        name: "Workspace Review",
        content: {
          type: "inline",
          text: fixture.isBlocking
            ? "# Workspace Review\n\n## Blocking findings\n\nTwo release-safety issues must be fixed before publishing."
            : "# Workspace Review\n\nNo blocking findings. The current changes are ready to publish.",
        },
        metadata: { createdAt: fixture.now, createdBy: "ralphx-workspace-reviewer", version: 3 },
        derivedFrom: [],
        bucketId: "prd-library",
      }
    : null;
  await page.evaluate(({ artifact, context, targetConversationId }) => {
    const queryClient = window.__queryClient;
    if (!queryClient) throw new Error("Expected query client for Review fixture");
    if (artifact) queryClient.setQueryData(["agents", "artifact", artifact.id], artifact);
    queryClient.setQueryData(
      ["agents", "workspace-review-context", targetConversationId],
      context,
    );
  }, { artifact, context, targetConversationId: conversationId });
  const label = state === "running" ? "Running" : state === "blocking" ? "Issues" : "Passed";
  await expect(reviewTab).toContainText(label);
}
