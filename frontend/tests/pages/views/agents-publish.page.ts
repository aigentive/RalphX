import { expect, type Locator, type Page } from "@playwright/test";

import { installPagedPublishDiffRoute } from "../../fixtures/agents-publish-diff.fixtures";
import {
  seedWorkspaceReviewState,
  type WorkspaceReviewVisualState,
} from "../../fixtures/agents-workspace-review.fixtures";
import { seedRepairPendingWorkspace } from "../../fixtures/repair-pending-publish.fixtures";
import { setupApp } from "../../fixtures/setup.fixtures";
import { seedMergedWorkspace } from "../../fixtures/terminal-publish.fixtures";
import { revealAgentInboxConversation } from "../../helpers/agents-inbox.helpers";
import {
  expectNoPaneOverflow,
  expectPrimaryActionContained,
} from "../../helpers/agents-publish-layout.helpers";
import { BasePage } from "../base.page";

const TERMINAL_CONVERSATION_ID = "conv-agent-terminal-publish-visual";
const REPAIR_CONVERSATION_ID = "conv-agent-repair-pending-visual";
const REVIEW_WALKTHROUGH_CONVERSATION_ID = "conv-agent-review-walkthrough-visual";
const PROJECT_ID = "project-mock-1";

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
  readonly historyTab: Locator;
  readonly historyContent: Locator;
  readonly automationTab: Locator;
  readonly automationContent: Locator;
  readonly prSupervisionStatus: Locator;
  readonly reviewWalkthroughEnter: Locator;
  readonly reviewWalkthrough: Locator;
  readonly reviewWalkthroughExit: Locator;
  readonly reviewWalkthroughProgress: Locator;
  readonly reviewWalkthroughPosition: Locator;
  readonly reviewWalkthroughCard: Locator;
  readonly reviewWalkthroughHunk: Locator;
  readonly reviewWalkthroughPrevious: Locator;
  readonly reviewWalkthroughNext: Locator;
  readonly reviewWalkthroughMark: Locator;
  readonly reviewWalkthroughDone: Locator;
  readonly reviewWalkthroughRestart: Locator;
  readonly publishConfirm: Locator;
  readonly holdCard: Locator;
  readonly reviewWalkthroughUnreviewedFile: Locator;
  readonly reviewWalkthroughUnreviewedCode: Locator;

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
    this.historyTab = page.getByTestId("agents-publish-tab-history");
    this.historyContent = page.getByTestId("agents-publish-content-history");
    this.automationTab = page.getByTestId("agents-publish-tab-automation");
    this.automationContent = page.getByTestId(
      "agents-publish-content-automation",
    );
    this.prSupervisionStatus = page.getByTestId(
      "agents-pr-supervision-status",
    );
    this.reviewWalkthroughEnter = page.getByTestId("publish-review-walkthrough-enter");
    this.reviewWalkthrough = page.getByTestId("publish-review-walkthrough");
    this.reviewWalkthroughExit = page.getByTestId("publish-review-walkthrough-exit");
    this.reviewWalkthroughProgress = page.getByTestId("publish-review-walkthrough-progress");
    this.reviewWalkthroughPosition = page.getByTestId("publish-review-walkthrough-position");
    this.reviewWalkthroughCard = page.getByTestId("publish-review-walkthrough-card");
    this.reviewWalkthroughHunk = page.getByTestId("publish-review-walkthrough-hunk");
    this.reviewWalkthroughPrevious = page.getByTestId("publish-review-walkthrough-prev");
    this.reviewWalkthroughNext = page.getByTestId("publish-review-walkthrough-next");
    this.reviewWalkthroughMark = page.getByTestId("publish-review-walkthrough-mark");
    this.reviewWalkthroughDone = page.getByTestId("publish-review-walkthrough-done");
    this.reviewWalkthroughRestart = page.getByTestId("publish-review-walkthrough-restart");
    this.publishConfirm = page.getByTestId("agents-publish-confirm");
    this.holdCard = page.getByTestId("agents-publish-hold-card");
    this.reviewWalkthroughUnreviewedFile = page.getByText(
      "frontend/tests/visual/views/agents/no-findings.ts",
      { exact: true },
    );
    this.reviewWalkthroughUnreviewedCode = this.reviewWalkthroughUnreviewedFile
      .locator("xpath=../..")
      .getByText("export function publishedView() {", { exact: true });
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

  /**
   * Publish sub-tabs are lazy-mounted: the panel only renders a tab's content
   * after that tab has been activated once. Always route through these helpers
   * before asserting on anything that lives inside a non-default tab.
   */
  async selectHistory() {
    await this.historyTab.click();
    await expect(this.historyTab).toHaveAttribute("data-state", "active");
    await expect(this.historyContent).toBeVisible();
  }

  async selectAutomation() {
    await this.automationTab.click();
    await expect(this.automationTab).toHaveAttribute("data-state", "active");
    await expect(this.automationContent).toBeVisible();
  }

  async seedWorkspaceReviewState(
    conversationId: string,
    state: WorkspaceReviewVisualState,
  ) {
    await seedWorkspaceReviewState(
      this.page,
      this.reviewTab,
      conversationId,
      state,
    );
  }

  async expectNoPaneOverflow() {
    await expectNoPaneOverflow(this.publishPane);
  }

  async expectPrimaryActionContained(testId: string) {
    await expectPrimaryActionContained(this.page, this.publishPane, testId);
  }

  async expectCompactPrStatus(label: string) {
    await expect(this.page.getByLabel(label, { exact: true })).toBeVisible();
  }

  async expectDiffRowsLoaded() {
    await expect(this.pagedDiffContent).toBeVisible();
    await expect(
      this.inlineDiffs.getByText("Could not load diff rows"),
    ).toHaveCount(0);
  }

  async installPagedDiffRoute() {
    await installPagedPublishDiffRoute(this.page);
  }

  async openRepairPendingScenario() {
    await setupApp(this.page);
    await seedRepairPendingWorkspace(
      this.page,
      REPAIR_CONVERSATION_ID,
      PROJECT_ID,
    );
    await this.page.getByTestId("nav-agents").click();
    await expect(this.page.getByTestId("agents-view")).toBeVisible();
    await this.page.evaluate(() =>
      window.__queryClient?.invalidateQueries({
        queryKey: ["agents", "sidebar-conversations"],
      }),
    );
    const conversation = await revealAgentInboxConversation(
      this.page,
      REPAIR_CONVERSATION_ID,
    );
    await conversation.getByRole("button").first().click();
    await this.page.evaluate(async (conversationId) => {
      const { mockGetAgentConversationWorkspace } = await import(
        "/src/api-mock/chat"
      );
      const workspace = await mockGetAgentConversationWorkspace(conversationId);
      if (!workspace || !window.__queryClient) {
        throw new Error("Expected repair-pending workspace query fixture");
      }
      window.__queryClient.setQueryData(
        ["agents", "conversation-workspace", conversationId],
        workspace,
      );
    }, REPAIR_CONVERSATION_ID);
    await this.page.getByRole("button", { name: "Open artifacts" }).click();
    const publishTab = this.page.getByTestId("agents-artifact-tab-publish");
    await expect(publishTab).toBeVisible();
    await publishTab.click();
    await expect(this.page.getByTestId("agents-publish-pane")).toBeVisible();
  }

  async openMaintenanceRepairScenario() {
    await this.openRepairPendingScenario();
    await this.page.evaluate((conversationId) => {
      const queryKey = ["agents", "conversation-workspace", conversationId];
      const workspace = window.__queryClient?.getQueryData<Record<string, unknown>>(
        queryKey,
      );
      if (!workspace || !window.__queryClient) {
        throw new Error("Expected workspace query fixture");
      }
      window.__queryClient.setQueryData(queryKey, {
        ...workspace,
        maintenanceOperation: {
          operationId: "maintenance-visual-1",
          generation: 1,
          source: "base_update",
          stage: "repairing",
          status: "active",
          summary: "Resolving the base conflict",
          blocker: null,
          automaticContinuation: true,
          startedAt: "2026-07-25T10:00:00Z",
          updatedAt: "2026-07-25T10:01:00Z",
        },
      });
    }, REPAIR_CONVERSATION_ID);
  }

  async openHeldRepairScenario() {
    await this.openRepairPendingScenario();
    await this.page.evaluate(async (conversationId) => {
      const { seedMockAgentConversationWorkspace } = await import("/src/api-mock/chat");
      const queryKey = ["agents", "conversation-workspace", conversationId];
      const workspace = window.__queryClient?.getQueryData<Record<string, unknown>>(
        queryKey,
      );
      if (!workspace || !window.__queryClient) {
        throw new Error("Expected workspace query fixture");
      }
      const heldWorkspace = {
        ...workspace,
        publicationPushStatus: "refreshed",
        prSupervisionStatus: "held",
        maintenanceOperation: {
          operationId: "maintenance-held-visual-1",
          generation: 2,
          source: "pr_autofix",
          stage: "ready",
          status: "ready",
          holdReason: "health_evidence",
          summary: "The fixer made no changes.",
          blocker: null,
          automaticContinuation: false,
          startedAt: "2026-08-02T10:00:00Z",
          updatedAt: "2026-08-02T10:01:00Z",
        },
        prAutofixFingerprintSpend: {
          generations: 2,
          minutes: 18,
          budgetMinutes: 45,
          isExhausted: false,
        },
      };
      seedMockAgentConversationWorkspace(heldWorkspace);
      window.__queryClient.setQueryData(queryKey, heldWorkspace);
    }, REPAIR_CONVERSATION_ID);
    await expect(this.holdCard).toBeVisible();
  }

  async openMergedPublishScenario() {
    await this.installPagedDiffRoute();
    await setupApp(this.page);
    await seedMergedWorkspace(this.page, TERMINAL_CONVERSATION_ID, PROJECT_ID);
    await this.page.getByTestId("nav-agents").click();
    await expect(this.page.getByTestId("agents-view")).toBeVisible();
    await this.page.evaluate(() =>
      window.__queryClient?.invalidateQueries({
        queryKey: ["agents", "sidebar-conversations"],
      }),
    );
    const conversation = await revealAgentInboxConversation(
      this.page,
      TERMINAL_CONVERSATION_ID,
    );
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

  async openReviewWalkthroughScenario() {
    await this.installPagedDiffRoute();
    await setupApp(this.page);
    await this.page.evaluate(async ({ conversationId, projectId }) => {
      const {
        mockGetAgentConversationWorkspace,
        mockStartAgentConversation,
        seedMockConversation,
      } = await import("/src/api-mock/chat");
      const { agentWorkspaceKeys } = await import(
        "/src/components/agents/agentWorkspaceQueries"
      );
      const createdAt = "2026-07-21T10:00:00.000Z";
      seedMockConversation({
        id: conversationId,
        contextType: "project",
        contextId: projectId,
        claudeSessionId: null,
        providerSessionId: `thread-${conversationId}`,
        providerHarness: "codex",
        upstreamProvider: "openai",
        providerProfile: null,
        agentMode: "edit",
        coordinationMode: "solo",
        title: "Review walkthrough visual coverage",
        messageCount: 0,
        lastMessageAt: null,
        createdAt,
        updatedAt: createdAt,
        archivedAt: null,
      }, []);
      await mockStartAgentConversation({
        projectId,
        conversationId,
        content: "Seed review walkthrough visual coverage",
        providerHarness: "codex",
        modelId: "gpt-5.4",
        mode: "edit",
        base: { kind: "project_default", ref: "main", displayName: "Project default (main)" },
      });
      const workspace = await mockGetAgentConversationWorkspace(conversationId);
      if (!workspace || !window.__queryClient) {
        throw new Error("Expected review walkthrough workspace query fixture");
      }
      const files = [
        "frontend/src/components/agents/AgentsPublishPanel.tsx",
        "frontend/src/components/agents/review-data.ts",
        "frontend/tests/visual/views/agents/no-findings.ts",
      ];
      const hunk = (path: string, content: string) => ({
        filePath: path,
        language: path.endsWith(".tsx") ? "tsx" : "ts",
        oldTotalLines: 2,
        newTotalLines: 3,
        isBinary: false,
        hunks: [{
          header: "@@ -1,2 +1,3 @@",
          oldStart: 1,
          oldLines: 2,
          newStart: 1,
          newLines: 3,
          lines: [
            { kind: "context", content: "export const previous = true;", oldLineNum: 1, newLineNum: 1 },
            { kind: "addition", content, oldLineNum: null, newLineNum: 2 },
          ],
        }],
      });
      const annotations = [
        ["review-finding-1", files[0], "Avoid stale publish state", "The publish action must continue to reflect its real gate.", "error"],
        ["review-finding-2", files[0], "Keep keyboard navigation local", "Inputs must retain their native keyboard behavior.", "warning"],
        ["review-finding-3", files[1], "Preserve complete diff context", "Walkthrough findings need their matching hunk.", "notice"],
      ].map(([id, path, title, message, level]) => ({
        id,
        conversationId,
        projectId,
        artifactId: "artifact-review-walkthrough",
        artifactVersion: 1,
        targetScope: "workspace_delta" as const,
        headSha: "review-walkthrough-head",
        diffFingerprint: "review-walkthrough-diff",
        path,
        diffSource: "committed",
        hunkHeader: "@@ -1,2 +1,3 @@",
        oldStart: 1,
        oldLines: 2,
        newStart: 1,
        newLines: 3,
        title,
        message,
        level,
        createdByRunId: "run-review-walkthrough",
        createdAt,
      }));
      window.__queryClient.setQueryData(agentWorkspaceKeys.workspace(conversationId), workspace);
      window.__queryClient.setQueryData(agentWorkspaceKeys.review(conversationId), {
        changes: files.map((path) => ({ path, status: "modified", additions: 1, deletions: 0, isGenerated: false })),
        commits: [],
        baseRef: "main",
        headRef: "HEAD",
        supportsWorktreeModes: true,
      });
      window.__queryClient.setQueryData(agentWorkspaceKeys.workspaceReviewHunkAnnotations(conversationId), {
        artifactId: "artifact-review-walkthrough",
        artifactVersion: 1,
        targetScope: "workspace_delta",
        headSha: "review-walkthrough-head",
        diffFingerprint: "review-walkthrough-diff",
        annotations,
      });
      files.forEach((path, index) => {
        window.__queryClient.setQueryData(
          [...agentWorkspaceKeys.diff(conversationId), "uncommitted", path],
          hunk(path, index === 2 ? "export const walkthroughUnreviewedFile = true;" : "export const reviewWalkthrough = true;"),
        );
      });
      await window.__queryClient.cancelQueries({ queryKey: ["agents", "sidebar-conversations"] });
      window.__queryClient.removeQueries({ queryKey: ["agents", "sidebar-conversations"] });
      window.__queryClient.removeQueries({ queryKey: ["agents", "project-conversations"] });
    }, { conversationId: REVIEW_WALKTHROUGH_CONVERSATION_ID, projectId: PROJECT_ID });
    await this.page.getByTestId("nav-agents").click();
    await expect(this.page.getByTestId("agents-view")).toBeVisible();
    await this.page.evaluate(() => window.__queryClient?.invalidateQueries({ queryKey: ["agents", "sidebar-conversations"] }));
    const conversation = await revealAgentInboxConversation(
      this.page,
      REVIEW_WALKTHROUGH_CONVERSATION_ID,
    );
    await conversation.getByRole("button").first().click();
    await this.page.evaluate(async (conversationId) => {
      const { mockGetAgentConversationWorkspace } = await import("/src/api-mock/chat");
      const workspace = await mockGetAgentConversationWorkspace(conversationId);
      if (!workspace || !window.__queryClient) {
        throw new Error("Expected review walkthrough workspace query fixture");
      }
      window.__queryClient.setQueryData(
        ["agents", "conversation-workspace", conversationId],
        workspace,
      );
    }, REVIEW_WALKTHROUGH_CONVERSATION_ID);
    await this.page.evaluate(async (conversationId) => {
      const queryClient = window.__queryClient;
      if (!queryClient) throw new Error("Expected review walkthrough query client");
      const { agentWorkspaceKeys } = await import(
        "/src/components/agents/agentWorkspaceQueries"
      );
      const keys = [
        agentWorkspaceKeys.workspace(conversationId),
        agentWorkspaceKeys.review(conversationId),
        agentWorkspaceKeys.workspaceReviewHunkAnnotations(conversationId),
      ];
      const files = [
        "frontend/src/components/agents/AgentsPublishPanel.tsx",
        "frontend/src/components/agents/review-data.ts",
        "frontend/tests/visual/views/agents/no-findings.ts",
      ];
      keys.push(...files.map((path) => [
        ...agentWorkspaceKeys.diff(conversationId),
        "uncommitted",
        path,
      ]));
      keys.forEach((key) => {
        queryClient.setQueryData(key, queryClient.getQueryData(key));
      });
    }, REVIEW_WALKTHROUGH_CONVERSATION_ID);
    await this.page.getByRole("button", { name: "Open artifacts" }).click();
    const publishTab = this.page.getByTestId("agents-artifact-tab-publish");
    await expect(publishTab).toBeVisible();
    await publishTab.click();
    await this.page.evaluate(async ({ conversationId, projectId }) => {
      const queryClient = window.__queryClient;
      if (!queryClient) throw new Error("Expected review walkthrough query client");
      const { agentWorkspaceKeys } = await import(
        "/src/components/agents/agentWorkspaceQueries"
      );
      await queryClient.cancelQueries({
        queryKey: agentWorkspaceKeys.review(conversationId),
      });
      await queryClient.cancelQueries({
        queryKey: agentWorkspaceKeys.workspaceReviewHunkAnnotations(conversationId),
      });
      const files = [
        "frontend/src/components/agents/AgentsPublishPanel.tsx",
        "frontend/src/components/agents/review-data.ts",
        "frontend/tests/visual/views/agents/no-findings.ts",
      ];
      const hunk = (path: string, content: string) => ({
        filePath: path,
        language: path.endsWith(".tsx") ? "tsx" : "ts",
        oldTotalLines: 2,
        newTotalLines: 3,
        isBinary: false,
        hunks: [{
          header: "@@ -1,2 +1,3 @@",
          oldStart: 1,
          oldLines: 2,
          newStart: 1,
          newLines: 3,
          lines: [
            { kind: "context", content: "export const previous = true;", oldLineNum: 1, newLineNum: 1 },
            { kind: "addition", content, oldLineNum: null, newLineNum: 2 },
          ],
        }],
      });
      const annotations = [
        ["review-finding-1", files[0], "Avoid stale publish state", "The publish action must continue to reflect its real gate.", "error"],
        ["review-finding-2", files[0], "Keep keyboard navigation local", "Inputs must retain their native keyboard behavior.", "warning"],
        ["review-finding-3", files[1], "Preserve complete diff context", "Walkthrough findings need their matching hunk.", "notice"],
      ].map(([id, path, title, message, level]) => ({
        id,
        conversationId,
        projectId,
        artifactId: "artifact-review-walkthrough",
        artifactVersion: 1,
        targetScope: "workspace_delta" as const,
        headSha: "review-walkthrough-head",
        diffFingerprint: "review-walkthrough-diff",
        path,
        diffSource: "committed",
        hunkHeader: "@@ -1,2 +1,3 @@",
        oldStart: 1,
        oldLines: 2,
        newStart: 1,
        newLines: 3,
        title,
        message,
        level,
        createdByRunId: "run-review-walkthrough",
        createdAt: "2026-07-21T10:00:00.000Z",
      }));
      queryClient.setQueryData(agentWorkspaceKeys.review(conversationId), {
        changes: files.map((path) => ({ path, status: "modified", additions: 1, deletions: 0, isGenerated: false })),
        commits: [],
        baseRef: "main",
        headRef: "HEAD",
        supportsWorktreeModes: true,
      });
      queryClient.setQueryData(agentWorkspaceKeys.workspaceReviewHunkAnnotations(conversationId), {
        artifactId: "artifact-review-walkthrough",
        artifactVersion: 1,
        targetScope: "workspace_delta",
        headSha: "review-walkthrough-head",
        diffFingerprint: "review-walkthrough-diff",
        annotations,
      });
      files.forEach((path, index) => {
        queryClient.setQueryData(
          [...agentWorkspaceKeys.diff(conversationId), "uncommitted", path],
          hunk(path, index === 2 ? "export const walkthroughUnreviewedFile = true;" : "export const reviewWalkthrough = true;"),
        );
      });
    }, { conversationId: REVIEW_WALKTHROUGH_CONVERSATION_ID, projectId: PROJECT_ID });
    await expect(this.reviewWalkthroughEnter).toBeVisible();
  }

  async enterReviewWalkthrough() {
    await this.reviewWalkthroughEnter.click();
    await expect(this.reviewWalkthrough).toBeVisible();
  }

  reviewWalkthroughDot(index: number) {
    return this.page.getByTestId(`publish-review-walkthrough-dot-${index}`);
  }

  async pressWalkthroughKey(key: string) {
    await this.page.keyboard.press(key);
  }

  async focusWalkthroughInput() {
    await this.page.evaluate(() => {
      const input = document.createElement("input");
      input.setAttribute("data-testid", "review-walkthrough-keyboard-guard");
      document.body.append(input);
      input.focus();
    });
  }

  async reloadReviewWalkthroughScenario() {
    await this.page.reload();
    await this.openReviewWalkthroughScenario();
  }
}
