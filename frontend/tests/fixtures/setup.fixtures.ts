import { Page } from "@playwright/test";
import type { FeatureFlags } from "@/types/feature-flags";

const STANDALONE_IDEATION_FEATURE_FLAGS: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  ideationPage: true,
  battleMode: true,
  teamMode: false,
  atlassianOauth: false,
  ticketingDashboard: false,
};
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

async function installStandaloneIdeationFeatureFlag(page: Page) {
  await page.addInitScript((flags) => {
    window.__mockUiFeatureFlags = flags;
  }, STANDALONE_IDEATION_FEATURE_FLAGS);
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

export async function enableStandaloneIdeationPage(page: Page) {
  await page.evaluate((flags) => {
    const uiStore = (window as Window & {
      __queryClient?: {
        setQueryData(queryKey: unknown[], data: unknown): void;
      };
      __uiStore?: {
        getState(): {
          setFeatureFlags(flags: FeatureFlags): void;
        };
      };
    }).__uiStore;
    const queryClient = (window as Window & {
      __queryClient?: {
        setQueryData(queryKey: unknown[], data: unknown): void;
      };
    }).__queryClient;

    if (!uiStore) {
      throw new Error("UI store not available");
    }

    queryClient?.setQueryData(["featureFlags"], flags);
    uiStore.getState().setFeatureFlags(flags);
  }, STANDALONE_IDEATION_FEATURE_FLAGS);
  await page.waitForSelector('[data-testid="nav-ideation"]', { timeout: 10000 });
}

export async function setupKanban(page: Page) {
  await setupApp(page);
  await page.evaluate(async () => {
    const { useProjectStore } = await import("/src/stores/projectStore");
    const { planApi } = await import("/src/api/plan");
    const planStore = (window as Window & {
      __planStore?: { getState(): { loadActivePlan(projectId: string): Promise<void> } };
    }).__planStore;

    useProjectStore.getState().selectProject("project-mock-1");
    await planApi.setActivePlan("project-mock-1", "plan-mock-2", "kanban_inline");
    await planStore?.getState().loadActivePlan("project-mock-1");
  });
  await page.click('[data-testid="nav-kanban"]');
  await page.waitForSelector('[data-testid^="task-card-"]', { timeout: 10000 });
}

export async function setupIdeation(page: Page) {
  await installStandaloneIdeationFeatureFlag(page);
  await setupApp(page);
  await enableStandaloneIdeationPage(page);
  // Navigate to ideation view
  await page.click('[data-testid="nav-ideation"]');
  // Wait for ideation view to load
  await page.waitForSelector('[data-testid="ideation-view"]', { timeout: 10000 });
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

export async function setupTaskDetail(page: Page) {
  await setupKanban(page);
  // Click the first task card to open detail overlay
  const firstTaskCard = page.locator('[data-testid^="task-card-"]').first();
  await firstTaskCard.click();
  // Wait for task detail overlay to load
  await page.waitForSelector('[data-testid="task-detail-overlay"]', { timeout: 10000 });
}

export async function setupNotificationsPanel(page: Page) {
  await setupApp(page);
  // Click reviews toggle to open the panel
  await page.click('[data-testid="reviews-toggle"]');
  // Wait for notification center to load
  await page.waitForSelector('[data-testid="notifications-panel"]', { timeout: 10000 });
}

export async function setupEmptyKanban(page: Page) {
  await setupApp(page);
  // Clear all tasks from the mock store to create an empty state
  await page.evaluate(async () => {
    const testWindow = window as Window & {
      __mockStore?: {
        tasks: Map<string, unknown>;
        taskSteps: Map<string, unknown>;
      };
      __planStore?: { getState(): { loadActivePlan(projectId: string): Promise<void> } };
      __queryClient?: { invalidateQueries(): Promise<unknown> | unknown };
    };
    const { useProjectStore } = await import("/src/stores/projectStore");
    const { planApi } = await import("/src/api/plan");
    const mockStore = testWindow.__mockStore;
    const planStore = testWindow.__planStore;
    const queryClient = testWindow.__queryClient;
    const projectId = "project-mock-1";
    const planId = "plan-empty-kanban";

    useProjectStore.getState().selectProject(projectId);
    await planApi.setActivePlan(projectId, planId, "kanban_inline");
    await planStore?.getState().loadActivePlan(projectId);

    if (mockStore) {
      // Clear only tasks, keep the project
      mockStore.tasks.clear();
      mockStore.taskSteps.clear();
    }

    // Invalidate React Query cache to trigger refetch with empty data
    if (queryClient) {
      void queryClient.invalidateQueries();
    }
  });
  await page.click('[data-testid="nav-kanban"]');
  // Wait for queries to refetch and render empty state
  await page.waitForTimeout(500);
  // Wait for the board to be visible (even if empty)
  await page.waitForSelector('[data-testid="task-board"]', { timeout: 10000 });
}
