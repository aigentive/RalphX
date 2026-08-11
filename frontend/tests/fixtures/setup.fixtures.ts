import { Page } from "@playwright/test";
import type { MockNotification } from "@/api-mock/store";
import type { Task } from "@/types/task";

const PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY =
  "ralphx-provider-cli-dismissed-updates";
const MOCK_PROVIDER_CLI_UPDATE_KEYS = ["claude:2.1.175", "codex:0.137.0"];

export async function dismissProviderCliUpdateToasts(page: Page) {
  await page.addInitScript(
    ({ storageKey, dismissedKeys }) => {
      window.localStorage.setItem(storageKey, JSON.stringify(dismissedKeys));
    },
    {
      storageKey: PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY,
      dismissedKeys: MOCK_PROVIDER_CLI_UPDATE_KEYS,
    },
  );
}

export async function setupApp(page: Page) {
  const appHeader = page.locator('[data-testid="app-header"]');

  const gotoApp = () => page.goto("/", { waitUntil: "commit", timeout: 7000 });

  try {
    await gotoApp();
  } catch (error) {
    if (page.isClosed()) {
      throw error;
    }
    await gotoApp();
  }

  await appHeader.waitFor({ state: "visible", timeout: 15000 });
}

export async function setupActivity(page: Page) {
  await setupApp(page);
  // Navigate to activity view
  await page.click('[data-testid="nav-activity"]');
  // Wait for activity view to load
  await page.waitForSelector('[data-testid="activity-view"]', { timeout: 10000 });
}

export async function setupSettings(page: Page) {
  await setupApp(page);
  // Open settings modal via uiStore (exposed on window in web mode)
  await page.evaluate(() => {
    const uiStore = (window as unknown as { __uiStore?: { getState(): { openModal(type: string, ctx?: Record<string, unknown>): void } } }).__uiStore;
    if (uiStore) {
      uiStore.getState().openModal("settings");
    }
  });
  // Wait for settings dialog to open
  await page.waitForSelector('[data-testid="settings-dialog"]', { timeout: 10000 });
}

export async function setupExtensibility(page: Page) {
  await setupApp(page);
  // Navigate to extensibility view
  await page.click('[data-testid="nav-extensibility"]');
  // Wait for extensibility view to load
  await page.waitForSelector('[data-testid="extensibility-view"]', { timeout: 10000 });
}

export async function setupNotificationsPanel(page: Page) {
  await setupApp(page);
  // Click reviews toggle to open the panel
  await page.click('[data-testid="reviews-toggle"]');
  // Wait for notification center to load
  await page.waitForSelector('[data-testid="notifications-panel"]', { timeout: 10000 });
}

/** Seeds a human-review task through the same mock store that attention queries read. */
export async function seedReviewAttentionTask(page: Page, taskId = "task-mock-6") {
  await page.evaluate(async (reviewTaskId) => {
    const { useTaskStore } = await import("/src/stores/taskStore");
    const testWindow = window as Window & {
      __mockStore?: { tasks: Map<string, Task> };
      __queryClient?: { invalidateQueries(filters?: { queryKey: readonly string[] }): Promise<unknown> };
    };
    const task = testWindow.__mockStore?.tasks.get(reviewTaskId);
    if (!task) {
      throw new Error(`Mock review task ${reviewTaskId} is unavailable`);
    }

    const reviewTask = { ...task, internalStatus: "review_passed" as const, updatedAt: new Date().toISOString() };
    testWindow.__mockStore?.tasks.set(reviewTaskId, reviewTask);
    useTaskStore.getState().addTask(reviewTask);
    await testWindow.__queryClient?.invalidateQueries({ queryKey: ["attention"] });
  }, taskId);
}

/** Seeds durable notification rows for a visual state and invalidates their history queries. */
export async function seedMockNotificationHistory(page: Page, notifications: readonly MockNotification[]) {
  await page.evaluate(async (notificationRows) => {
    const mockStore = window.__mockStore;
    if (!mockStore) {
      throw new Error(
        "Mock store is unavailable; interact with the app first so a mock API handler initializes it before seeding notification history.",
      );
    }

    mockStore.notifications.clear();
    notificationRows.forEach((notification) => {
      mockStore.notifications.set(notification.id, notification);
    });
    await window.__queryClient?.invalidateQueries({ queryKey: ["notifications"] });
  }, notifications);
}
