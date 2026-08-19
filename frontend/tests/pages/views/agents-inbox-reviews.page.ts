import { expect, type Locator, type Page } from "@playwright/test";

import { AgentsInboxRecentPage } from "./agents-inbox-recent.page";

export class AgentsInboxReviewsPage extends AgentsInboxRecentPage {
  readonly chipRow: Locator;
  readonly reviewsChip: Locator;
  readonly reviewsPanel: Locator;
  readonly reviewsEmpty: Locator;
  readonly needsGroupHeader: Locator;
  readonly workingGroupHeader: Locator;
  readonly watchingGroupHeader: Locator;

  constructor(page: Page) {
    super(page);
    this.chipRow = page.getByTestId("agents-inbox-lane-chips");
    this.reviewsChip = page.getByTestId("agents-inbox-lane-chip-reviews");
    this.reviewsPanel = page.getByTestId("agents-inbox-lane-panel-reviews");
    this.reviewsEmpty = page.getByTestId("agents-inbox-lane-empty-reviews");
    this.needsGroupHeader = page.getByTestId(
      "agents-inbox-reviews-group-header-review_needs",
    );
    this.workingGroupHeader = page.getByTestId(
      "agents-inbox-reviews-group-header-review_working",
    );
    this.watchingGroupHeader = page.getByTestId(
      "agents-inbox-reviews-group-header-review_watching",
    );
  }

  async selectReviews(): Promise<void> {
    await this.reviewsChip.click();
    await expect(this.reviewsChip).toHaveAttribute("aria-selected", "true");
  }

  /**
   * Resizes the sidebar and returns the settled chip-row container width.
   *
   * Container queries only resolve after layout and the sidebar width is
   * animated, so a single frame reports the *previous* width. Polling until two
   * consecutive reads agree is what makes the measurement trustworthy.
   */
  async settleSidebarWidth(width: number): Promise<number> {
    await this.setSidebarWidth(width);
    let previous = -1;
    for (let attempt = 0; attempt < 60; attempt += 1) {
      const current = await this.chipRow.evaluate((row) => row.clientWidth);
      if (current === previous) {
        return current;
      }
      previous = current;
      await this.page.waitForTimeout(50);
    }
    return previous;
  }

  async chipRowOverflows(): Promise<boolean> {
    return this.chipRow.evaluate((row) => row.scrollWidth > row.clientWidth + 1);
  }

  /** Whether the PR Reviews chip renders its full label rather than the icon. */
  async reviewsLabelVisible(): Promise<boolean> {
    return this.reviewsChip.evaluate((chip) =>
      Array.from(chip.querySelectorAll("span")).some(
        (span) => span.textContent === "PR Reviews" && span.clientWidth > 0,
      ),
    );
  }
}
