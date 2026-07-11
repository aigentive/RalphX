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

  // Seed the notification query and its backing task store. Web mock mode does
  // not implement list_attention_items, so its mock task map alone cannot
  // render the notification-center review card.
  await page.evaluate(async (reviewTaskId) => {
    const { useTaskStore } = await import("/src/stores/taskStore");
    const testWindow = window as Window & {
      __mockStore?: { tasks: Map<string, Task> };
      __queryClient?: { setQueryData(queryKey: readonly string[], data: unknown): void };
    };
    const task = testWindow.__mockStore?.tasks.get(reviewTaskId);
    if (!task) {
      throw new Error(`Mock review task ${reviewTaskId} is unavailable`);
    }

    useTaskStore.getState().addTask(task);
    testWindow.__queryClient?.setQueryData(["attention", "list", "all"], [{
      id: `review:${reviewTaskId}`,
      category: "review_needed",
      title: task.title,
      detail: "Review is ready.",
      projectId: task.projectId,
      createdAt: "2026-07-11T10:00:00Z",
      target: { kind: "task", taskId: reviewTaskId },
    }]);
  }, taskId);

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
