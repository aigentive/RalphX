import { expect, test } from "@playwright/test";

import { AgentsInboxRecentPage } from "../../../pages/views/agents-inbox-recent.page";

test.describe("Agents zero inbox", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 560 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.addInitScript(() => window.localStorage.clear());
  });

  test("celebrates a true Recent zero", async ({ page }) => {
    const inbox = new AgentsInboxRecentPage(page);
    await inbox.open("zero");

    await expect(inbox.zeroCard).toContainText("Inbox zero");
    await expect(inbox.zeroPrimary).toBeVisible();
    await expect(inbox.zeroSecondary).toHaveText("Review 7 done");
    await expect(inbox.needsGroup).toHaveCount(0);
    await expect(inbox.sidebar).toHaveScreenshot("agents-inbox-zero-recent.png");
  });

  test("keeps Stale zero calm and educational", async ({ page }) => {
    const inbox = new AgentsInboxRecentPage(page);
    await inbox.open("stale-zero");

    await expect(inbox.staleEmpty).toContainText("Nothing has gone stale");
    await expect(inbox.backToRecent).toBeVisible();
    await expect(inbox.stalePrimary).toHaveCount(0);
    await expect(inbox.sidebar).toHaveScreenshot("agents-inbox-zero-stale.png");
  });

  test("explains the empty Done tier", async ({ page }) => {
    const inbox = new AgentsInboxRecentPage(page);
    await inbox.open("done-zero");

    await expect(inbox.doneEmpty).toContainText("Nothing finished yet");
    await expect(inbox.backToRecent).toBeVisible();
    await expect(inbox.sidebar).toHaveScreenshot("agents-inbox-zero-done.png");
  });

  test("distinguishes filtered zero from inbox zero", async ({ page }) => {
    const inbox = new AgentsInboxRecentPage(page);
    await inbox.open("filtered-zero");

    await expect(inbox.zeroCard).toContainText("No matches");
    await expect(inbox.zeroCard).not.toContainText("Inbox zero");
    await expect(inbox.clearSearch).toBeVisible();
    await expect(inbox.sidebar).toHaveScreenshot("agents-inbox-zero-filtered.png");
  });

  test("wraps safely in a narrow sidebar", async ({ page }) => {
    const inbox = new AgentsInboxRecentPage(page);
    await inbox.open("zero");
    await inbox.setSidebarWidth(268);

    await expect(inbox.zeroCard).toContainText("Good moment to start the next thing.");
    await inbox.expectZeroCardFits();
    await expect(inbox.sidebar).toHaveScreenshot("agents-inbox-zero-recent-narrow.png");
  });

  test("returns from Done to Recent through the empty-state action", async ({ page }) => {
    const inbox = new AgentsInboxRecentPage(page);
    await inbox.open("done-zero");

    await inbox.backToRecent.click();
    await expect(inbox.recentChip).toHaveAttribute("aria-selected", "true");
  });

  test("opens Done from the Recent review action", async ({ page }) => {
    const inbox = new AgentsInboxRecentPage(page);
    await inbox.open("zero");

    await inbox.zeroSecondary.click();
    await expect(inbox.doneChip).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
});
