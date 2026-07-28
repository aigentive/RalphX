import type { Page } from "@playwright/test";

/** Seeds the list query with the four priority groups used in visual coverage. */
export async function seedAutomationListVisualState(page: Page, projectId: string) {
  await page.evaluate(async (activeProjectId) => {
    if (!window.__queryClient) {
      throw new Error("Expected query client for automation list fixture");
    }
    const { mockAutomationsApi } = await import("/src/api-mock/automations");
    const automations = await mockAutomationsApi.list({ projectId: activeProjectId });
    window.__queryClient.setQueryData(["automations", "list", activeProjectId], automations);
    await Promise.all(automations.map(async (automation) => {
      const detail = await mockAutomationsApi.get(automation.id);
      window.__queryClient?.setQueryData(["automations", "detail", automation.id], detail);
    }));
  }, projectId);
}
