import { expect, test, type Page } from "@playwright/test";

import { seedAutomationDetailVisualState } from "../../../fixtures/automation-detail.fixtures";
import { setupApp } from "../../../fixtures/setup.fixtures";
import { AutomationDetailPage } from "../../../pages/views/automation-detail.page";

const automationId = "automation-detail-visual-1";
const projectId = "project-mock-1";
const themes = ["dark", "light", "high-contrast"] as const;

async function setTheme(page: Page, theme: (typeof themes)[number]) {
  await page.evaluate(async (nextTheme) => {
    const { useThemeStore } = await import("/src/stores/themeStore");
    useThemeStore.getState().setTheme(nextTheme);
  }, theme);
}

async function openSeededDetail(
  detailPage: AutomationDetailPage,
  theme: (typeof themes)[number] = "dark",
) {
  await setupApp(detailPage.page);
  await detailPage.openAutomationsView();
  await seedAutomationDetailVisualState(detailPage.page, { automationId, projectId });
  await setTheme(detailPage.page, theme);
  await detailPage.openDetail(automationId);
}

test.describe("automation detail page tabs", () => {
  for (const theme of themes) {
    test(`overview matches the redesigned detail hierarchy in ${theme}`, async ({ page }) => {
      const detailPage = new AutomationDetailPage(page);
      await openSeededDetail(detailPage, theme);

      await expect(page.getByTestId("automation-runs-count")).toHaveText("3");
      await expect(detailPage.runsTabLiveDot).toBeVisible();
      await expect(detailPage.runsTimeline).toHaveCount(0);
      await expect(detailPage.statCards).toBeVisible();
      await expect(detailPage.phasesCard).toBeVisible();
      await expect(detailPage.executionCard).toBeVisible();
      await expect(detailPage.specInputsCard).toBeVisible();
      await expect(page.getByTestId("automation-config-pr-link")).toHaveText(/PR #841/);

      await expect(page).toHaveScreenshot(`automation-detail-overview-${theme}.png`, {
        fullPage: false,
        maxDiffPixelRatio: 0.01,
      });
      await detailPage.specInputsCard.scrollIntoViewIfNeeded();
      await expect(page).toHaveScreenshot(`automation-detail-overview-lower-${theme}.png`, {
        fullPage: false,
        maxDiffPixelRatio: 0.01,
      });
    });
  }

  test("runs tab shows the deduped timeline with open, merged, and failed cards", async ({
    page,
  }) => {
    const detailPage = new AutomationDetailPage(page);
    await openSeededDetail(detailPage);
    await detailPage.openRunsTab();

    // Open running card: single accent status badge, no "Run 3 in progress" echo.
    const runningHeader = page.getByTestId(
      `automation-run-${automationId}-run-3-header-status`,
    );
    await expect(runningHeader).toHaveText("Running");
    await expect(
      page.getByTestId(`automation-run-${automationId}-run-3-header-stage`),
    ).toHaveCount(0);
    // Settled merged card collapses judge language into the facts row.
    await expect(
      page.getByTestId(`automation-run-${automationId}-run-2-header-status`),
    ).toHaveText("Merged");
    // Failed card keeps its visible failure reason.
    await expect(
      page.getByTestId(`automation-run-${automationId}-run-1-failure`),
    ).toHaveText(/Publish step exited with code 1/);

    await expect(page).toHaveScreenshot("automation-detail-runs-tab.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("plan icon opens the markdown plan dialog", async ({ page }) => {
    const detailPage = new AutomationDetailPage(page);
    await openSeededDetail(detailPage);
    await detailPage.openRunsTab();

    await detailPage.runPlanIcon(`${automationId}-run-3`).click();
    await expect(detailPage.planDialog).toBeVisible();
    // Wait for the lazy markdown chunk to hydrate (H2 replaces plaintext "## Plan").
    await expect(
      detailPage.planDialog.locator(
        '[data-testid="automation-plan-dialog-markdown"] h2',
      ),
    ).toHaveText("Plan");
    await expect(detailPage.planDialog).toContainText("Version the skill schema.");

    await expect(page).toHaveScreenshot("automation-detail-plan-dialog.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });
});
