import { expect, type Locator, type Page } from "@playwright/test";

import { installPagedPublishDiffRoute } from "../../fixtures/agents-publish-diff.fixtures";
import {
  seedWorkspaceReviewState,
  type WorkspaceReviewVisualState,
} from "../../fixtures/agents-workspace-review.fixtures";
import { seedRepairPendingWorkspace } from "../../fixtures/repair-pending-publish.fixtures";
import { setupApp } from "../../fixtures/setup.fixtures";
import { seedMergedWorkspace } from "../../fixtures/terminal-publish.fixtures";
import {
  expectNoPaneOverflow,
  expectPrimaryActionContained,
} from "../../helpers/agents-publish-layout.helpers";
import { BasePage } from "../base.page";

const TERMINAL_CONVERSATION_ID = "conv-agent-terminal-publish-visual";
const REPAIR_CONVERSATION_ID = "conv-agent-repair-pending-visual";
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
    const conversation = this.page.getByTestId(
      `agents-session-${REPAIR_CONVERSATION_ID}`,
    );
    // The sidebar list is repopulated by an async refetch after
    // invalidateQueries, which can exceed the default assertion timeout when
    // the suite runs under load.
    await expect(conversation).toBeVisible({ timeout: 15000 });
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
    const conversation = this.page.getByTestId(
      `agents-session-${TERMINAL_CONVERSATION_ID}`,
    );
    // The sidebar list is repopulated by an async refetch after
    // invalidateQueries, which can exceed the default assertion timeout when
    // the suite runs under load.
    await expect(conversation).toBeVisible({ timeout: 15000 });
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
}
