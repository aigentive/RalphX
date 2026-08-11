import { expect, type Locator, type Page } from "@playwright/test";

import {
  seedRecentInboxScenario,
  type RecentInboxScenario,
} from "../../fixtures/agents-inbox-recent.fixtures";
import { setupApp } from "../../fixtures/setup.fixtures";
import { BasePage } from "../base.page";

const SETTLE_TIMEOUT_MS = 30_000;

export class AgentsInboxRecentPage extends BasePage {
  readonly sidebar: Locator;
  readonly recentScroller: Locator;
  readonly recentChip: Locator;
  readonly doneChip: Locator;
  readonly needsGroup: Locator;
  readonly workingGroup: Locator;
  readonly needsHeader: Locator;
  readonly workingHeader: Locator;
  readonly needsPager: Locator;
  readonly workingPager: Locator;
  readonly needsEmptyStrip: Locator;
  readonly zeroCard: Locator;
  readonly staleEmpty: Locator;
  readonly doneEmpty: Locator;
  readonly zeroPrimary: Locator;
  readonly zeroSecondary: Locator;
  readonly backToRecent: Locator;
  readonly clearSearch: Locator;
  readonly stalePrimary: Locator;

  constructor(page: Page) {
    super(page);
    this.sidebar = page.getByTestId("agents-sidebar");
    this.recentScroller = page.getByTestId("agents-sidebar-session-list-inbox-recent");
    this.recentChip = page.getByTestId("agents-inbox-lane-chip-recent");
    this.doneChip = page.getByTestId("agents-inbox-lane-chip-done");
    this.needsGroup = page.getByTestId("agents-inbox-recent-group-needs");
    this.workingGroup = page.getByTestId("agents-inbox-recent-group-working");
    this.needsHeader = page.getByTestId("agents-inbox-recent-group-header-needs");
    this.workingHeader = page.getByTestId("agents-inbox-recent-group-header-working");
    this.needsPager = page.getByTestId("agents-inbox-recent-pager-needs");
    this.workingPager = page.getByTestId("agents-inbox-recent-pager-working");
    this.needsEmptyStrip = page.getByTestId("agents-inbox-lane-empty-needs");
    this.zeroCard = page.getByTestId("agents-inbox-lane-empty-recent");
    this.staleEmpty = page.getByTestId("agents-inbox-lane-empty-stale");
    this.doneEmpty = page.getByTestId("agents-inbox-lane-empty-done");
    this.zeroPrimary = this.zeroCard.getByRole("button", { name: "New agent" });
    this.zeroSecondary = this.zeroCard.getByRole("button", { name: /Review \d+ done/ });
    this.backToRecent = page.getByRole("button", { name: "Back to Recent" });
    this.clearSearch = this.zeroCard.getByRole("button", { name: "Clear search" });
    this.stalePrimary = this.staleEmpty.getByRole("button", { name: "New agent" });
  }

  async open(scenario: RecentInboxScenario): Promise<void> {
    await setupApp(this.page);
    await seedRecentInboxScenario(this.page, scenario);
    await this.page.evaluate(() => window.__queryClient?.invalidateQueries());
    await this.page.getByTestId("nav-agents").click();
    await expect(this.sidebar).toBeVisible();
    await this.page.getByTestId("agents-group-trigger").click();
    await this.page.getByRole("radio", { name: "Inbox" }).click();
    await this.page.keyboard.press("Escape");
    await expect(this.recentChip).toHaveAttribute("aria-selected", "true", { timeout: SETTLE_TIMEOUT_MS });

    if (scenario === "zero") {
      await this.waitForZeroCard();
      return;
    }

    await expect(this.recentScroller).toBeVisible({ timeout: SETTLE_TIMEOUT_MS });
    await this.waitForGroupsSettled();

    if (scenario === "filtered-zero") {
      await this.page.getByTestId("agents-search-toggle").click();
      await this.page.getByTestId("agents-search-input").fill("no matching conversation");
      await this.waitForZeroCard();
      return;
    }
    if (scenario === "stale-zero") {
      await this.selectLane("stale");
      await expect(this.staleEmpty).toBeVisible({ timeout: SETTLE_TIMEOUT_MS });
    }
    if (scenario === "done-zero") {
      await this.selectLane("done");
      await expect(this.doneEmpty).toBeVisible({ timeout: SETTLE_TIMEOUT_MS });
    }
  }
  async selectLane(filter: "recent" | "stale" | "done"): Promise<void> {
    const chip = this.page.getByTestId(`agents-inbox-lane-chip-${filter}`);
    await chip.click();
    await expect(chip).toHaveAttribute("aria-selected", "true");
    await this.page.evaluate(() => new Promise<void>((resolve) => {
      window.requestAnimationFrame(() => window.setTimeout(resolve, 0));
    }));
  }

  async waitForZeroCard(): Promise<void> {
    await expect(this.zeroCard).toBeVisible({ timeout: SETTLE_TIMEOUT_MS });
  }

  async setSidebarWidth(width: number): Promise<void> {
    await this.sidebar.evaluate((sidebar, nextWidth) => {
      sidebar.style.width = `${nextWidth}px`;
      sidebar.style.maxWidth = `${nextWidth}px`;
    }, width);
  }

  async expectZeroCardFits(): Promise<void> {
    const fits = await this.zeroCard.evaluate(
      (card) => card.scrollWidth <= card.clientWidth,
    );
    expect(fits).toBe(true);
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
}
