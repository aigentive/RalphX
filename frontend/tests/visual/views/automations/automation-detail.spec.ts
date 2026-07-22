import { expect, test } from "@playwright/test";

import { seedAutomationDetailVisualState } from "../../../fixtures/automation-detail.fixtures";
import { setupApp } from "../../../fixtures/setup.fixtures";
import { AutomationDetailPage } from "../../../pages/views/automation-detail.page";

const automationId = "automation-detail-visual-1";
const projectId = "project-mock-1";

async function openSeededDetail(detailPage: AutomationDetailPage) {
  await setupApp(detailPage.page);
  await detailPage.openAutomationsView();
  await seedAutomationDetailVisualState(detailPage.page, { automationId, projectId });
  await detailPage.openDetail(automationId);
}

test.describe("automation detail page tabs", () => {
  test("overview tab shows goal, phases with plan icons, and grouped config", async ({
    page,
  }) => {
    const detailPage = new AutomationDetailPage(page);
    await openSeededDetail(detailPage);

    // Overview is the default tab; the Runs trigger carries the live dot while
    // a run is open and no timeline content is mounted yet.
    await expect(detailPage.runsTab).toHaveText(/Runs \(3\)/);
    await expect(detailPage.runsTabLiveDot).toBeVisible();
    await expect(detailPage.runsTimeline).toHaveCount(0);
    await expect(detailPage.detailsCard).toBeVisible();
    await expect(
      page.getByTestId("automation-config-pr-link"),
    ).toHaveText(/PR #841/);
    await expect(page.getByTestId("automation-goal-card-plan-icon")).toHaveCount(0);

    await expect(page).toHaveScreenshot("automation-detail-overview-tab.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

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
