import { expect, test } from "@playwright/test";

import { dismissProviderCliUpdateToasts } from "../../../fixtures/setup.fixtures";
import { AgentsPublishPage } from "../../../pages/views/agents-publish.page";
import { seedAutomationSpend } from "./publish-automation-tab.fixtures";

test.describe("Agents publish Automation tab", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.addInitScript(() => window.localStorage.clear());
  });

  test("shows the autofix repair budget panel when spend is reportable", async ({
    page,
  }) => {
    await dismissProviderCliUpdateToasts(page);
    const publish = new AgentsPublishPage(page);
    await publish.openRepairPendingScenario();
    await seedAutomationSpend(page, {
      generations: 3,
      minutes: 24,
      budgetMinutes: 60,
      isExhausted: false,
    });
    await publish.selectAutomation();

    const budget = page.getByTestId("agents-pr-autofix-budget");
    await expect(budget).toBeVisible();
    await expect(budget).toContainText("24 / 60 min");
    await expect(budget).toContainText("3 generations");
    await expect(publish.automationContent.getByTestId("agents-auto-publish-switch")).toBeVisible();
    await expect(publish.automationContent.getByTestId("agents-pr-autofix-switch")).toBeVisible();
    await expect(publish.automationContent.getByTestId("agents-pr-auto-merge-switch")).toBeVisible();

    await publish.expectNoPaneOverflow();
    await expect(publish.automationContent).toHaveScreenshot(
      "automation-tab-spend-reportable.png",
      { maxDiffPixelRatio: 0.01 },
    );

    const standardViewport = page.viewportSize() ?? { width: 1440, height: 900 };
    await page.setViewportSize({ width: 960, height: standardViewport.height });
    await publish.expectNoPaneOverflow();
    await expect(publish.automationContent).toHaveScreenshot(
      "automation-tab-spend-reportable-constrained.png",
      { maxDiffPixelRatio: 0.01 },
    );
    await page.setViewportSize(standardViewport);
  });

  test("hides the budget panel for a zero-valued spend record, not just a missing one", async ({
    page,
  }) => {
    await dismissProviderCliUpdateToasts(page);
    const publish = new AgentsPublishPage(page);
    await publish.openRepairPendingScenario();
    // Explicit Some({generations:0, minutes:0, isExhausted:false}) — proves
    // hasReportableAutofixSpend's boolean gate, not merely a null/undefined
    // short-circuit.
    await seedAutomationSpend(page, {
      generations: 0,
      minutes: 0,
      budgetMinutes: 60,
      isExhausted: false,
    });
    await publish.selectAutomation();

    await expect(page.getByTestId("agents-pr-autofix-budget")).toHaveCount(0);
    await expect(publish.automationContent.getByTestId("agents-auto-publish-switch")).toBeVisible();
    await expect(publish.automationContent.getByTestId("agents-pr-autofix-switch")).toBeVisible();
    await expect(publish.automationContent.getByTestId("agents-pr-auto-merge-switch")).toBeVisible();

    await publish.expectNoPaneOverflow();
    await expect(publish.automationContent).toHaveScreenshot(
      "automation-tab-spend-hidden.png",
      { maxDiffPixelRatio: 0.01 },
    );

    const standardViewport = page.viewportSize() ?? { width: 1440, height: 900 };
    await page.setViewportSize({ width: 960, height: standardViewport.height });
    await publish.expectNoPaneOverflow();
    await expect(publish.automationContent).toHaveScreenshot(
      "automation-tab-spend-hidden-constrained.png",
      { maxDiffPixelRatio: 0.01 },
    );
    await page.setViewportSize(standardViewport);
  });
});
