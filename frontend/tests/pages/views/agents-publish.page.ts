import { expect, type Locator, type Page } from "@playwright/test";
import type {
  AgentConversationWorkspace,
  AgentWorkspaceReviewContext,
} from "@/api/chat";

import { setupApp } from "../../fixtures/setup.fixtures";
import { seedMergedWorkspace } from "../../fixtures/terminal-publish.fixtures";
import { BasePage } from "../base.page";

const TERMINAL_CONVERSATION_ID = "conv-agent-terminal-publish-visual";
const PROJECT_ID = "project-mock-1";

export type WorkspaceReviewVisualState = "running" | "blocking" | "passed";

export class AgentsPublishPage extends BasePage {
  readonly terminalHeading: Locator;
  readonly historicalFilter: Locator;
  readonly inlineDiffs: Locator;
  readonly pagedDiffContent: Locator;
  readonly composerContext: Locator;
  readonly publishPane: Locator;
  readonly changesTab: Locator;
  readonly reviewTab: Locator;
  readonly reviewContent: Locator;

  constructor(page: Page) {
    super(page);
    this.terminalHeading = page.getByRole("heading", { name: "Pull Request Merged" });
    this.historicalFilter = page.getByTestId("diff-filter-trigger");
    this.inlineDiffs = page.getByTestId("agents-publish-inline-diffs");
    this.pagedDiffContent = this.inlineDiffs
      .getByText("inlineRowsArePaged = true")
      .first();
    this.composerContext = page.getByTestId("agents-composer-workspace-changes");
    this.publishPane = page.getByTestId("agents-publish-pane");
    this.changesTab = page.getByTestId("agents-publish-tab-changes");
    this.reviewTab = page.getByTestId("agents-publish-tab-review");
    this.reviewContent = page.getByTestId("agents-publish-content-review");
  }

  async openFromHeader() {
    await this.page.getByTestId("agents-publish-workspace").click();
    await expect(this.publishPane).toBeVisible();
    await expect(this.changesTab).toBeVisible();
    await expect(this.reviewTab).toBeVisible();
  }

  async selectChanges() {
    await this.changesTab.click();
    await expect(this.changesTab).toHaveAttribute("data-state", "active");
  }

  async selectReview() {
    await this.reviewTab.click();
    await expect(this.reviewTab).toHaveAttribute("data-state", "active");
    await expect(this.reviewContent).toBeVisible();
  }

  async seedWorkspaceReviewState(
    conversationId: string,
    state: WorkspaceReviewVisualState,
  ) {
    await this.page.evaluate(
      ({ targetConversationId, targetState }) => {
        const queryClient = window.__queryClient;
        if (!queryClient) {
          throw new Error("Expected query client for Workspace Review fixture");
        }
        const workspace = queryClient.getQueryData<AgentConversationWorkspace>([
          "agents",
          "conversation-workspace",
          targetConversationId,
        ]);
        if (!workspace) {
          throw new Error("Expected workspace for Workspace Review fixture");
        }

        const now = "2026-05-13T05:15:00.000Z";
        const isRunning = targetState === "running";
        const isBlocking = targetState === "blocking";
        const artifactId = isRunning
          ? null
          : `workspace-review-${targetState}-visual`;
        const reviewOutcome = isBlocking
          ? "blocking"
          : targetState === "passed"
            ? "passed"
            : "none";
        const reviewGateStatus = isRunning
          ? "reviewing"
          : isBlocking
            ? "blocking"
            : "passed";
        const diffFingerprint = "visual-review-diff-fingerprint";
        const isCurrent = !isRunning;
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
            diffFingerprint,
            sourcePullRequestNumber: workspace.publicationPrNumber,
          },
          monitor: {
            conversationId: targetConversationId,
            projectId: workspace.projectId,
            status: isRunning ? "reviewing" : "ready",
            reviewOutcome,
            reviewGateStatus,
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
            reviewBlockingFingerprint: isBlocking
              ? "visual-blocking-fingerprint"
              : null,
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
          },
          reviewArtifactIsCurrent: isCurrent,
          reviewArtifactIsOutdated: false,
          canMutateReviewState: true,
          reviewRuntimeState: isRunning ? "active_owned" : "terminal",
          isCurrent,
          isOutdated: false,
          shouldShowTab: true,
        };

        if (artifactId) {
          queryClient.setQueryData(["agents", "artifact", artifactId], {
            id: artifactId,
            type: "workspace_review",
            name: "Workspace Review",
            content: {
              type: "inline",
              text: isBlocking
                ? "# Workspace Review\n\n## Blocking findings\n\nTwo release-safety issues must be fixed before publishing."
                : "# Workspace Review\n\nNo blocking findings. The current changes are ready to publish.",
            },
            metadata: {
              createdAt: now,
              createdBy: "ralphx-workspace-reviewer",
              version: 3,
            },
            derivedFrom: [],
            bucketId: "prd-library",
          });
        }
        queryClient.setQueryData(
          ["agents", "workspace-review-context", targetConversationId],
          context,
        );
      },
      { targetConversationId: conversationId, targetState: state },
    );

    const stateLabel =
      state === "running"
        ? "Running"
        : state === "blocking"
          ? "Issues"
          : "Passed";
    await expect(this.reviewTab).toContainText(stateLabel);
  }

  async expectNoPaneOverflow() {
    const horizontalOverflow = await this.publishPane.evaluate(
      (element) => element.scrollWidth - element.clientWidth,
    );
    expect(horizontalOverflow).toBeLessThanOrEqual(2);
  }

  async expectPrimaryActionContained(testId: string) {
    const action = this.page.getByTestId(testId);
    await expect(action).toBeVisible();
    const [actionBox, paneBox] = await Promise.all([
      action.boundingBox(),
      this.publishPane.boundingBox(),
    ]);
    expect(actionBox).not.toBeNull();
    expect(paneBox).not.toBeNull();
    expect(actionBox!.x).toBeGreaterThanOrEqual(paneBox!.x - 1);
    expect(actionBox!.x + actionBox!.width).toBeLessThanOrEqual(
      paneBox!.x + paneBox!.width + 1,
    );
    const viewport = this.page.viewportSize();
    if (viewport) {
      expect(actionBox!.x + actionBox!.width).toBeLessThanOrEqual(
        viewport.width + 1,
      );
    }
  }

  async openMergedPublishScenario() {
    await this.installPagedDiffRoute();
    await setupApp(this.page);
    await seedMergedWorkspace(this.page, TERMINAL_CONVERSATION_ID, PROJECT_ID);
    await this.page.getByTestId("nav-agents").click();
    await expect(this.page.getByTestId("agents-view")).toBeVisible();
    const conversation = this.page.getByTestId(
      `agents-session-${TERMINAL_CONVERSATION_ID}`,
    );
    await expect(conversation).toBeVisible();
    await conversation.getByRole("button").first().click();
    await this.page.evaluate(async (conversationId) => {
      const { mockGetAgentConversationWorkspace } = await import("/src/api-mock/chat");
      const workspace = await mockGetAgentConversationWorkspace(conversationId);
      if (!workspace || !window.__queryClient) {
        throw new Error("Expected terminal workspace query fixture");
      }
      window.__queryClient.setQueryData(
        ["agents", "conversation-workspace", conversationId],
        workspace,
      );
    }, TERMINAL_CONVERSATION_ID);
    await this.page.getByRole("button", { name: "Open artifacts" }).click();
    const publishTab = this.page.getByTestId("agents-artifact-tab-publish");
    await expect(publishTab).toBeVisible();
    await publishTab.click();
    await expect(this.page.getByTestId("agents-publish-pane")).toBeVisible();
  }

  private async installPagedDiffRoute() {
    await this.page.route("**/api/agent-workspaces/**/file-diff-page**", async (route) => {
      const url = new URL(route.request().url());
      const filePath = url.searchParams.get("path") ?? "frontend/src/Published.tsx";
      const rows = [
        {
          kind: "hunk_header", header: "@@ -1,2 +1,3 @@",
          old_start: 1, old_lines: 2, new_start: 1, new_lines: 3,
        },
        {
          kind: "line",
          line: {
            kind: "context",
            content: "export function publishedView() {",
            old_line_num: 1,
            new_line_num: 1,
          },
        },
        {
          kind: "line",
          line: {
            kind: "addition",
            content: "  const inlineRowsArePaged = true;",
            old_line_num: null,
            new_line_num: 2,
          },
        },
      ];
      const offset = Number(url.searchParams.get("offset") ?? "0");
      const limit = Number(url.searchParams.get("limit") ?? "200");
      const pageRows = rows.slice(offset, offset + limit);
      const nextOffset =
        offset + pageRows.length < rows.length
          ? offset + pageRows.length
          : null;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          file_path: filePath,
          language: "tsx",
          rows: pageRows,
          offset,
          limit,
          next_offset: nextOffset,
          total_rows: rows.length,
          old_total_lines: 2,
          new_total_lines: 3,
          is_binary: false,
        }),
      });
    });
  }
}
