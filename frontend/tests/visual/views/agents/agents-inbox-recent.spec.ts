import { expect, test } from "@playwright/test";

import { AgentsInboxRecentPage } from "../../../pages/views/agents-inbox-recent.page";

test.describe("Agents Recent inbox", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 560 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.addInitScript(() => window.localStorage.clear());
  });

  test("shows populated groups with load-more and exhausted pagers", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 1000 });
    const recent = new AgentsInboxRecentPage(page);
    await recent.open("populated");

    await expect(recent.needsHeader).toContainText("Needs you");
    await expect(recent.workingHeader).toContainText("Working");
    await expect(recent.needsGroup.getByText("Needs you: review 1")).toBeVisible();
    await expect(recent.workingGroup.getByText("Working: supervised PR 1")).toBeVisible();
    await expect(recent.needsPager).toHaveText("Load 2 older");
    await expect(recent.workingPager).toHaveText("All 3 shown");
    await expect(recent.sidebar).toHaveScreenshot("agents-inbox-recent-pagers.png");
  });

  test("keeps the Needs you header pinned while its rows scroll", async ({ page }) => {
    const recent = new AgentsInboxRecentPage(page);
    await recent.open("populated");
    await recent.scrollNeedsHeaderToTop();

    await expect(recent.sidebar).toHaveScreenshot("agents-inbox-recent-sticky-needs.png");
  });

  test("retains an empty Needs you group beside Working rows", async ({ page }) => {
    const recent = new AgentsInboxRecentPage(page);
    await recent.open("empty-needs");

    await expect(recent.needsHeader).toContainText("Needs you");
    await expect(recent.needsEmpty).toBeVisible();
    await expect(recent.workingGroup.getByText("Working: supervised PR 1")).toBeVisible();
    await expect(recent.sidebar).toHaveScreenshot("agents-inbox-recent-empty-needs.png");
  });
});
