import { expect, type Locator, type Page } from "@playwright/test";

import { setupApp } from "../../fixtures/setup.fixtures";
import { BasePage } from "../base.page";

type RecentInboxScenario = "populated" | "empty-needs";

const SETTLE_TIMEOUT_MS = 30_000;

export class AgentsInboxRecentPage extends BasePage {
  readonly sidebar: Locator;
  readonly recentScroller: Locator;
  readonly recentChip: Locator;
  readonly needsGroup: Locator;
  readonly workingGroup: Locator;
  readonly needsHeader: Locator;
  readonly workingHeader: Locator;
  readonly needsPager: Locator;
  readonly workingPager: Locator;
  readonly needsEmpty: Locator;

  constructor(page: Page) {
    super(page);
    this.sidebar = page.getByTestId("agents-sidebar");
    this.recentScroller = page.getByTestId("agents-sidebar-session-list-inbox-recent");
    this.recentChip = page.getByTestId("agents-inbox-lane-chip-recent");
    this.needsGroup = page.getByTestId("agents-inbox-recent-group-needs");
    this.workingGroup = page.getByTestId("agents-inbox-recent-group-working");
    this.needsHeader = page.getByTestId("agents-inbox-recent-group-header-needs");
    this.workingHeader = page.getByTestId("agents-inbox-recent-group-header-working");
    this.needsPager = page.getByTestId("agents-inbox-recent-pager-needs");
    this.workingPager = page.getByTestId("agents-inbox-recent-pager-working");
    this.needsEmpty = page.getByTestId("agents-inbox-lane-empty-needs");
  }

  async open(scenario: RecentInboxScenario): Promise<void> {
    await setupApp(this.page);
    await this.seedScenario(scenario);
    await this.page.getByTestId("nav-agents").click();
    await expect(this.sidebar).toBeVisible();
    await this.page.getByTestId("agents-group-trigger").click();
    await this.page.getByRole("radio", { name: "Inbox" }).click();
    await this.page.keyboard.press("Escape");
    await expect(this.recentChip).toHaveAttribute("aria-selected", "true");
    await expect(this.recentScroller).toBeVisible({ timeout: 30_000 });
    await this.waitForGroupsSettled();
  }

  // A group renders neither rows nor its empty line until its lane query
  // settles, so every assertion and screenshot has to wait for both groups to
  // reach one of those two states or it races the first paint.
  private async waitForGroupsSettled(): Promise<void> {
    for (const group of [this.needsGroup, this.workingGroup]) {
      await expect(
        group.getByTestId(/^agents-(session|inbox-lane-empty)-/),
      ).not.toHaveCount(0, { timeout: SETTLE_TIMEOUT_MS });
    }
  }

  async scrollNeedsHeaderToTop(): Promise<void> {
    await this.recentScroller.evaluate((scroller) => {
      scroller.scrollTop = Math.min(150, scroller.scrollHeight - scroller.clientHeight);
      scroller.dispatchEvent(new Event("scroll"));
    });
    await expect(this.needsHeader).toBeVisible();
  }

  // The screenshot is the record, but sticky has to be falsifiable without a
  // human: the header sits at the scroller's top edge while its own first row
  // has scrolled out from under it.
  async expectNeedsHeaderPinned(): Promise<void> {
    const scroller = await this.recentScroller.boundingBox();
    const header = await this.needsHeader.boundingBox();
    expect(scroller).not.toBeNull();
    expect(header).not.toBeNull();
    expect(Math.abs(header!.y - scroller!.y)).toBeLessThanOrEqual(1);
    expect(scroller!.y).toBeGreaterThan(0);
    await expect(
      this.needsGroup.getByText("Needs you: review 1"),
    ).not.toBeInViewport();
  }

  private async seedScenario(scenario: RecentInboxScenario): Promise<void> {
    await this.page.evaluate(async (seededScenario) => {
      const projectId = "project-mock-1";
      const now = new Date().toISOString();
      const { mockStartAgentConversation, seedMockAgentConversationWorkspace, seedMockConversation } =
        await import("/src/api-mock/chat");

      window.__mockChatApi?.reset();
      const seedConversation = async (id: string, title: string, working: boolean) => {
        seedMockConversation({
          id,
          contextType: "project",
          contextId: projectId,
          claudeSessionId: null,
          providerSessionId: `thread-${id}`,
          providerHarness: "codex",
          upstreamProvider: "openai",
          providerProfile: null,
          agentMode: "edit",
          automationId: null,
          automationRunId: null,
          coordinationMode: "solo",
          title,
          messageCount: 0,
          lastMessageAt: now,
          createdAt: now,
          updatedAt: now,
          archivedAt: null,
        }, []);
        const result = await mockStartAgentConversation({
          projectId,
          content: "Seed Recent inbox visual state",
          conversationId: id,
          providerHarness: "codex",
          modelId: "gpt-5.4",
          mode: "edit",
          base: { kind: "current_branch", ref: "main", displayName: "main" },
        });
        if (working && result.workspace) {
          seedMockAgentConversationWorkspace({
            ...result.workspace,
            prSupervisionStatus: "monitoring",
          });
        }
      };

      const needsCount = seededScenario === "populated" ? 10 : 0;
      await Promise.all([
        ...Array.from({ length: needsCount }, (_, index) =>
          seedConversation(`recent-needs-${index + 1}`, `Needs you: review ${index + 1}`, false),
        ),
        ...Array.from({ length: 3 }, (_, index) =>
          seedConversation(`recent-working-${index + 1}`, `Working: supervised PR ${index + 1}`, true),
        ),
      ]);
    }, scenario);
  }
}
