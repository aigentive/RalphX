import { test, expect } from "@playwright/test";
import { KanbanPage } from "../../../pages/kanban.page";

/**
 * Visual regression tests for loading states across views.
 * Tests skeleton loaders that appear while data is being fetched.
 */
test.describe("Loading States", () => {
  test("kanban view - skeleton loader", async ({ page }) => {
    const kanban = new KanbanPage(page);

    // Add a delay to the mock invoke to capture loading state
    await page.addInitScript(() => {
      (window as Window & { __mockInvokeDelay?: number }).__mockInvokeDelay = 3000;
    });

    // Navigate to kanban
    await page.goto("/");
    await page.waitForSelector('[data-testid="app-header"]', { timeout: 10000 });
    await page.click('[data-testid="nav-kanban"]');

    // Wait for skeleton to appear
    await kanban.skeleton.waitFor({ state: "visible", timeout: 5000 });

    // Verify skeleton structure
    await expect(kanban.skeleton).toBeVisible();

    // Verify skeleton columns are rendered
    await expect(kanban.skeletonColumn(0)).toBeVisible();
    await expect(kanban.skeletonColumn(1)).toBeVisible();
    await expect(kanban.skeletonColumn(2)).toBeVisible();
    await expect(kanban.skeletonColumn(3)).toBeVisible();
    await expect(kanban.skeletonColumn(4)).toBeVisible();

    // Verify skeleton cards are present in first column
    await expect(kanban.skeletonCard(0, 0)).toBeVisible();

    // Take screenshot of loading state
    await expect(page).toHaveScreenshot("kanban-loading-state.png", {
      fullPage: true,
      animations: "disabled",
    });
  });

  test("kanban view - loading to loaded transition", async ({ page }) => {
    const kanban = new KanbanPage(page);

    // Add a delay to the mock invoke to capture loading state
    await page.addInitScript(() => {
      (window as Window & { __mockInvokeDelay?: number }).__mockInvokeDelay = 2000;
    });

    // Navigate to kanban
    await page.goto("/");
    await page.waitForSelector('[data-testid="app-header"]', { timeout: 10000 });
    await page.click('[data-testid="nav-kanban"]');

    // Verify skeleton is visible
    await kanban.skeleton.waitFor({ state: "visible", timeout: 5000 });
    await expect(kanban.skeleton).toBeVisible();

    // Wait for skeleton to disappear and board to appear
    await kanban.skeleton.waitFor({ state: "hidden", timeout: 10000 });
    await kanban.board.waitFor({ state: "visible", timeout: 10000 });

    // Verify board is now visible
    await expect(kanban.board).toBeVisible();
    await expect(kanban.skeleton).not.toBeVisible();
  });
});
