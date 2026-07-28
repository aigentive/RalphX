import { expect, test, type Page } from "@playwright/test";

import { seedAutomationListVisualState } from "../../../fixtures/automation-list.fixtures";
import { setupApp } from "../../../fixtures/setup.fixtures";
import { AutomationListPage } from "../../../pages/views/automation-list.page";

const projectId = "project-mock-1";
const themes = ["dark", "light", "high-contrast"] as const;

async function setTheme(page: Page, theme: (typeof themes)[number]) {
  await page.evaluate(async (nextTheme) => {
    const { useThemeStore } = await import("/src/stores/themeStore");
    useThemeStore.getState().setTheme(nextTheme);
  }, theme);
}

for (const theme of themes) {
  test(`automations list renders all priority groups in ${theme}`, async ({ page }) => {
    await setupApp(page);
    const automationPage = new AutomationListPage(page);
    await automationPage.open();
    await seedAutomationListVisualState(page, projectId);
    await setTheme(page, theme);

    await expect(automationPage.toolbar).toBeVisible();
    await expect(automationPage.group("attention")).toBeVisible();
    await expect(automationPage.group("running")).toBeVisible();
    await expect(automationPage.group("finished")).toBeVisible();
    await expect(automationPage.group("drafts")).toBeVisible();
    await expect(automationPage.view).toHaveScreenshot(`automations-list-${theme}.png`, {
      maxDiffPixelRatio: 0.01,
    });
  });
}
