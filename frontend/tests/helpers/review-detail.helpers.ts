/**
 * Test helpers for ReviewDetailModal
 *
 * Uses React state manipulation via exposed test helper
 * (similar to task-detail.helpers.ts approach - see Phase 52 for pattern)
 */

import type { Page } from "@playwright/test";
import type { Task } from "@/types/task";
import { setupApp } from "../fixtures/setup.fixtures";

/**
 * Opens ReviewDetailModal programmatically by:
 * 1. Setting up the app shell
 * 2. Finding a task with review_passed status
 * 3. Opening its notification-center review card
 */
export async function openReviewDetailModal(page: Page): Promise<void> {
  // Setup the app shell. The review-detail modal mounts from the reviews panel
  // and does not require the kanban board to have an active plan selected.
  await setupApp(page);

  // Find a task with review_passed status
  const taskId = await page.evaluate(() => {
    const store = window.__mockStore;
    if (!store) {
      throw new Error("Mock store not available");
    }

    const reviewStatuses = ["review_passed", "escalated"];
    const tasks = Array.from(store.tasks.values()) as Task[];
    const reviewTask = tasks.find((t) =>
      reviewStatuses.includes(t.internalStatus)
    );

    if (!reviewTask) {
      throw new Error("No task with review status found");
    }

    return reviewTask.id;
  });

  // Open reviews panel first to mount the component
  await page.click('[data-testid="reviews-toggle"]');

  // Click the review card's unchanged action to preserve in-place modal behavior.
  await page.waitForSelector('[data-testid="notifications-panel"]', { timeout: 5000 });
  await page.click(`[data-testid="review-button-${taskId}"]`);

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
