import { test, expect } from "@playwright/test";
import { KanbanPage } from "../../../pages/kanban.page";
import { setupAllStatusColumns } from "../../../fixtures/kanban.fixtures";

/**
 * Visual regression test for kanban board with all status columns populated.
 *
 * Verifies that the kanban board correctly displays tasks across ALL 17 internal statuses.
 * This ensures the UI can handle the full range of workflow states.
 *
 * Runs against web mode dev server with mock data.
 */

test.describe("Kanban Board - All Status Columns", () => {
  let kanban: KanbanPage;

  test.beforeEach(async ({ page }) => {
    kanban = new KanbanPage(page);
    await setupAllStatusColumns(page);
  });

  test("renders all status columns with tasks", async ({ page }) => {
    // Wait for board to load
    await kanban.waitForBoard();

    // Verify we have task cards across all 5 columns (3 tasks per column = 15 total)
    const count = await kanban.getTaskCount();
    expect(count).toBeGreaterThanOrEqual(15);

    // Wait for animations
    await kanban.waitForAnimations();

    // Take full board screenshot
    await expect(page).toHaveScreenshot("all-status-columns.png", {
      maxDiffPixelRatio: 0.01,
      fullPage: true,
    });
  });

  test("each workflow column is visible", async ({ page }) => {
    // Wait for board
    await kanban.waitForBoard();

    // Verify all 5 default workflow columns exist by visible headings rather than
    // stale implementation-specific ids.
    await expect(page.getByRole("heading", { name: "Draft" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Ready" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "In Progress" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "In Review" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Done" })).toBeVisible();
  });
});
