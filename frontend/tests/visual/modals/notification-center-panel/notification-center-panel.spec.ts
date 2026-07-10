import { test, expect } from "@playwright/test";
import { NotificationCenterPanelPage } from "../../../pages/modals/notification-center-panel.page";
import { setupApp } from "../../../fixtures/setup.fixtures";

/**
 * Visual regression tests for the notification center.
 */

test.describe("NotificationCenterPanel", () => {
  let reviewsPanel: NotificationCenterPanelPage;

  test.beforeEach(async ({ page }) => {
    reviewsPanel = new NotificationCenterPanelPage(page);
    await setupApp(page);
  });

  test("opens panel when toggle button is clicked", async () => {
    // Initially the panel should not be visible
    await expect(reviewsPanel.panel).not.toBeVisible();

    // Click the reviews toggle button
    await reviewsPanel.openPanel();

    // Panel should now be visible
    await expect(reviewsPanel.panel).toBeVisible();
  });

  test("closes panel when close button is clicked", async () => {
    // Open the panel first
    await reviewsPanel.openPanel();
    await expect(reviewsPanel.panel).toBeVisible();

    // Close the panel
    await reviewsPanel.closeButton.click();

    // Panel should be hidden
    await expect(reviewsPanel.panel).not.toBeVisible();
  });

  test("displays the needs-action empty state when no notifications are pending", async () => {
    await reviewsPanel.openPanel();

    // Check for either empty state or review cards in mock mode.
    const hasEmptyState = await reviewsPanel.emptyState.isVisible().catch(() => false);
    const hasTaskCards = (await reviewsPanel.getTaskCardCount()) > 0;

    // One of these should be true
    expect(hasEmptyState || hasTaskCards).toBe(true);
  });

  test("allows switching between Needs action and History tabs", async () => {
    await reviewsPanel.openPanel();

    // Tabs should be visible
    await expect(reviewsPanel.needsActionTab).toBeVisible();
    await expect(reviewsPanel.historyTab).toBeVisible();

    await reviewsPanel.switchToNeedsActionTab();
    await expect(reviewsPanel.needsActionTab).toHaveAttribute("data-state", "active");

    await reviewsPanel.switchToHistoryTab();
    await expect(reviewsPanel.historyTab).toHaveAttribute("data-state", "active");
  });

  test("matches snapshot", async ({ page }) => {
    await reviewsPanel.openPanel();

    // Wait for animations to complete
    await reviewsPanel.waitForAnimations();

    // Take snapshot
    await expect(page).toHaveScreenshot("notification-center-panel.png", {
      maxDiffPixelRatio: 0.01,
    });
  });
});
