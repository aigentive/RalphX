import { Page } from "@playwright/test";

export async function setupApp(page: Page) {
  await page.goto("/");
  await page.waitForSelector('[data-testid="app-header"]', { timeout: 10000 });
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
  await setupApp(page);
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

export async function setupReviewsPanel(page: Page) {
  await setupApp(page);
  // Click reviews toggle to open the panel
  await page.click('[data-testid="reviews-toggle"]');
  // Wait for reviews panel to load
  await page.waitForSelector('[data-testid="reviews-panel"]', { timeout: 10000 });
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
