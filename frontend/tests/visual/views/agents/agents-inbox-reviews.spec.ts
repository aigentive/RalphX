import { expect, test } from "@playwright/test";

import { AgentsInboxReviewsPage } from "../../../pages/views/agents-inbox-reviews.page";

// The three real sidebar widths: large default, medium breakpoint, and the
// minimum of the resize drag range (`useAgentsSidebarResize.ts`, 220-520).
const SIDEBAR_WIDTHS = [340, 276, 220] as const;

// Must match the `@[...]` container-query breakpoint on the chip label in
// `AgentsSidebar.tsx`. This spec is the *source* of that number, not just its
// check: the first test measures the chip-row container width at each real
// sidebar width, and the breakpoint has to separate the widths that fit full
// labels from the ones that collapse to the icon.
//
// Measured chip-row container widths: 340 -> 315px, 276 -> 251px, 220 -> 195px.
// Only 315px fits four full labels with "PR Reviews", so 300 sits in the gap.
const CHIP_LABEL_BREAKPOINT_PX = 300;

test.describe("Agents PR Reviews inbox chip", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.addInitScript(() => window.localStorage.clear());
  });

  test("derives the chip-label collapse threshold from real sidebar widths", async ({
    page,
  }) => {
    const reviews = new AgentsInboxReviewsPage(page);
    await reviews.open("populated");

    const measurements: Array<{
      sidebarWidth: number;
      containerWidth: number;
      chipRowOverflows: boolean;
      labelsVisible: boolean;
    }> = [];

    for (const sidebarWidth of SIDEBAR_WIDTHS) {
      const containerWidth = await reviews.settleSidebarWidth(sidebarWidth);
      measurements.push({
        sidebarWidth,
        containerWidth,
        chipRowOverflows: await reviews.chipRowOverflows(),
        labelsVisible: await reviews.reviewsLabelVisible(),
      });
    }

    await test.info().attach("chip-row-measurements.json", {
      body: JSON.stringify(measurements, null, 2),
      contentType: "application/json",
    });

    // Guards the measurement itself: a lagging read would report widths that do
    // not track the sidebar, and every assertion below would be meaningless.
    const containerWidths = measurements.map((m) => m.containerWidth);
    expect(containerWidths).toEqual([...containerWidths].sort((a, b) => b - a));

    // If a chip label ever changes length, re-derive the breakpoint from the
    // attached measurements rather than rebaselining a screenshot.
    for (const measurement of measurements) {
      expect(
        measurement.labelsVisible,
        `sidebar ${measurement.sidebarWidth}px -> container ${measurement.containerWidth}px`,
      ).toBe(measurement.containerWidth >= CHIP_LABEL_BREAKPOINT_PX);
    }

    // Whichever form renders, the four chips must fit without overflowing.
    for (const measurement of measurements) {
      expect(
        measurement.chipRowOverflows,
        `chip row overflows at sidebar ${measurement.sidebarWidth}px`,
      ).toBe(false);
    }
  });

  for (const sidebarWidth of SIDEBAR_WIDTHS) {
    test(`fits four chips at a ${sidebarWidth}px sidebar`, async ({ page }) => {
      const reviews = new AgentsInboxReviewsPage(page);
      await reviews.open("populated");
      await reviews.settleSidebarWidth(sidebarWidth);

      await expect(reviews.reviewsChip).toBeVisible();
      // The accessible name keeps the full label at every width, so the
      // icon-only form still satisfies the icon-only-buttons rule.
      await expect(reviews.reviewsChip).toHaveAccessibleName(
        /^PR Reviews, \d+ conversations?$/,
      );

      await expect(reviews.chipRow).toHaveScreenshot(
        `agents-inbox-chips-${sidebarWidth}.png`,
      );
    });
  }

  test("shows the reviews panel and its calm empty state when selected", async ({
    page,
  }) => {
    const reviews = new AgentsInboxReviewsPage(page);
    await reviews.open("populated");
    await reviews.selectReviews();

    await expect(reviews.reviewsPanel).toBeVisible();
    // No review conversations in this scenario, so the calm empty state shows.
    await expect(reviews.reviewsEmpty).toBeVisible();
    await expect(reviews.reviewsEmpty).toContainText("No open reviews");
    await expect(reviews.sidebar).toHaveScreenshot("agents-inbox-reviews-empty.png");
  });
});
