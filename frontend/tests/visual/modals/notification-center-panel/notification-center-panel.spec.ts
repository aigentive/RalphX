import { test, expect, type Page } from "@playwright/test";
import { NotificationCenterPanelPage } from "../../../pages/modals/notification-center-panel.page";
import {
  seedMockNotificationHistory,
  seedReviewAttentionTask,
  setupApp,
} from "../../../fixtures/setup.fixtures";
import type { MockNotification } from "@/api-mock/store";
import type { Task } from "@/types/task";

/**
 * Visual regression tests for the notification center.
 */

const LONG_REVIEW_TITLE =
  "Review containment check for a generated task title with a very long unbroken token DrawerReviewTitleAlphaBetaGammaDeltaEpsilonZeta";
const LONG_REVIEW_DETAIL =
  "This review description intentionally includes a long filesystem-like segment /Users/example/project/packages/notification-drawer/regression/content-overflow-proof and enough text to verify the compact panel card wraps without clipping the Review action.";
const LONG_ATTENTION_TITLE =
  "Human input needed before continuing the notification workflow with LongAttentionTitleAlphaBetaGammaDeltaEpsilonZeta";
const LONG_ATTENTION_DETAIL =
  "Please confirm the release note wording for a narrow drawer scenario where copy, project metadata, and the Open task action must all remain contained inside the notification shell.";
const LONG_HISTORY_TITLE =
  "Durable history row with a long title and UnbrokenHistoryRowTokenAlphaBetaGammaDeltaEpsilonZeta inside the notification drawer";
const LONG_HISTORY_BODY =
  "History details include a long path /tmp/ralphx/visual/notification-drawer/super-long-file-name-for-overflow-checks.tsx plus normal copy to verify wrapping and time-column containment.";
const OVERFLOW_TOLERANCE_PX = 1;

async function seedLongNotificationDrawerContent(page: Page) {
  const updatedAt = new Date().toISOString();
  await page.evaluate(
    async (seed) => {
      const { useTaskStore } = await import("/src/stores/taskStore");
      const testWindow = window as Window & {
        __mockStore?: { tasks: Map<string, Task> };
        __queryClient?: {
          invalidateQueries(filters?: { queryKey: readonly unknown[] }): Promise<unknown> | unknown;
        };
      };
      const mockStore = testWindow.__mockStore;
      if (!mockStore) {
        throw new Error("Mock store is unavailable for notification drawer seeding");
      }

      const reviewTask = mockStore.tasks.get(seed.reviewTaskId);
      const attentionTask = mockStore.tasks.get(seed.attentionTaskId);
      if (!reviewTask || !attentionTask) {
        throw new Error("Expected seeded visual task fixtures to be available");
      }

      const nextReviewTask = {
        ...reviewTask,
        title: seed.reviewTitle,
        description: seed.reviewDetail,
        internalStatus: "review_passed" as const,
        updatedAt: seed.updatedAt,
      };
      const nextAttentionTask = {
        ...attentionTask,
        title: seed.attentionTitle,
        description: seed.attentionDetail,
        internalStatus: "blocked" as const,
        blockedReason: "human: confirm notification drawer visual fixture",
        updatedAt: seed.updatedAt,
      };

      mockStore.tasks.set(seed.reviewTaskId, nextReviewTask);
      mockStore.tasks.set(seed.attentionTaskId, nextAttentionTask);
      useTaskStore.getState().addTask(nextReviewTask);
      useTaskStore.getState().addTask(nextAttentionTask);
      await testWindow.__queryClient?.invalidateQueries({ queryKey: ["attention"] });
      await testWindow.__queryClient?.invalidateQueries({ queryKey: ["tasks"] });
    },
    {
      reviewTaskId: "task-mock-6",
      attentionTaskId: "task-mock-1",
      reviewTitle: LONG_REVIEW_TITLE,
      reviewDetail: LONG_REVIEW_DETAIL,
      attentionTitle: LONG_ATTENTION_TITLE,
      attentionDetail: LONG_ATTENTION_DETAIL,
      updatedAt,
    },
  );

  const longHistoryNotifications: MockNotification[] = [{
    id: "notification-long-drawer-history",
    createdAt: updatedAt,
    projectId: "project-mock-1",
    category: "agent_question",
    severity: "action_required",
    title: LONG_HISTORY_TITLE,
    body: LONG_HISTORY_BODY,
    target: { kind: "project", projectId: "project-mock-1" },
    dedupeKey: "visual:notification-drawer:long-history",
    readAt: null,
  }];
  await seedMockNotificationHistory(page, longHistoryNotifications);
}

function overflowingElements(panel: NotificationCenterPanelPage) {
  return panel.getContainedElementReport().then((items) =>
    items.filter((item) =>
      item.leftOverflowPixels > OVERFLOW_TOLERANCE_PX
      || item.rightOverflowPixels > OVERFLOW_TOLERANCE_PX,
    ),
  );
}

async function expectDrawerViewportContained(panel: NotificationCenterPanelPage) {
  const geometry = await panel.getDrawerGeometry();
  const overflow = await panel.getHorizontalOverflow();

  expect(geometry.shell.left).toBeGreaterThanOrEqual(0);
  expect(geometry.shell.right).toBeLessThanOrEqual(geometry.viewportWidth + OVERFLOW_TOLERANCE_PX);
  expect(geometry.shell.width).toBeLessThanOrEqual(geometry.viewportWidth + OVERFLOW_TOLERANCE_PX);
  expect(overflow.documentOverflowPixels).toBeLessThanOrEqual(OVERFLOW_TOLERANCE_PX);
  expect(overflow.shellViewportOverflowPixels).toBeLessThanOrEqual(OVERFLOW_TOLERANCE_PX);
  expect(overflow.panelScrollOverflowPixels).toBeLessThanOrEqual(OVERFLOW_TOLERANCE_PX);
  await expect.poll(() => overflowingElements(panel)).toEqual([]);
}

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

  test("closes panel when the outside backdrop is clicked", async () => {
    await reviewsPanel.openPanel();
    await expect(reviewsPanel.backdrop).toBeVisible();

    await reviewsPanel.closeByOutsideClick();

    await expect(reviewsPanel.panel).not.toBeVisible();
    await expect(reviewsPanel.backdrop).not.toBeVisible();
  });

  test("displays the needs-action empty state when no notifications are pending", async () => {
    await reviewsPanel.openPanel();
    await expect(reviewsPanel.emptyState).toBeVisible();
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
    await seedReviewAttentionTask(page);
    await reviewsPanel.openPanel();

    // Wait for animations to complete
    await reviewsPanel.waitForAnimations();

    // Take snapshot
    await expect(page).toHaveScreenshot("notification-center-panel.png", {
      maxDiffPixelRatio: 0.01,
    });
  });

  test("keeps long notification content contained at a narrow viewport", async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 760 });
    await setupApp(page);
    await seedLongNotificationDrawerContent(page);

    await reviewsPanel.openPanel();
    await expect(reviewsPanel.taskCards.first()).toBeVisible();
    await expect(reviewsPanel.attentionItems.first()).toBeVisible();
    await expect(reviewsPanel.taskCards.first()).toContainText("Review containment check");
    await expect(reviewsPanel.attentionItems.first()).toContainText("Human input needed");

    await expectDrawerViewportContained(reviewsPanel);
    await expect(reviewsPanel.shell).toHaveScreenshot(
      "notification-center-panel-narrow-long-action.png",
      { maxDiffPixelRatio: 0.01 },
    );

    await reviewsPanel.switchToHistoryTab();
    await expect(reviewsPanel.historyRows.first()).toBeVisible();
    await expect(reviewsPanel.historyRows.first()).toContainText("Durable history row");

    await expectDrawerViewportContained(reviewsPanel);
    await expect(reviewsPanel.shell).toHaveScreenshot(
      "notification-center-panel-narrow-long-history.png",
      { maxDiffPixelRatio: 0.01 },
    );
  });
});
