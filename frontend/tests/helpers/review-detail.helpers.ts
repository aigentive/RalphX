/**
 * Test helpers for ReviewDetailModal
 *
 * Uses the shared visual-test modal opening pattern.
 */

import type { Page } from "@playwright/test";
import { seedReviewAttentionTask, setupApp } from "../fixtures/setup.fixtures";

/**
 * Opens ReviewDetailModal programmatically by:
 * 1. Setting up the app shell
 * 2. Seeding a task into the human-review state
 * 3. Opening its notification-center review card
 */
export async function openReviewDetailModal(page: Page): Promise<void> {
  // Setup the app shell. The review-detail modal mounts from the reviews panel
  // and does not require the kanban board to have an active plan selected.
  await setupApp(page);

  const taskId = "task-mock-6";
  await seedReviewAttentionTask(page, taskId);

  // Open the notification center and use the review card action that opens
  // ReviewDetailModal in place.
  await page.click('[data-testid="reviews-toggle"]');
  await page.waitForSelector('[data-testid="notifications-panel"]', { timeout: 5000 });
  const reviewCard = page.locator(`[data-testid="task-review-card-${taskId}"]`);
  await reviewCard.waitFor({ state: "visible", timeout: 5000 });
  await reviewCard.locator(`[data-testid="review-button-${taskId}"]`).click();

  // Wait for modal to appear
  await page.waitForSelector('[data-testid="review-detail-modal"]', { timeout: 5000 });
}

/**
 * Closes ReviewDetailModal by clicking the close button
 */
export async function closeReviewDetailModal(page: Page): Promise<void> {
  await page.click('[data-testid="review-detail-modal"] [data-testid="dialog-close"]');
  await page.waitForSelector('[data-testid="review-detail-modal"]', {
    state: "hidden",
    timeout: 5000
  });
}
