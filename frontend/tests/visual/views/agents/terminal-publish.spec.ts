import { expect, test } from "@playwright/test";

import { dismissProviderCliUpdateToasts } from "../../../fixtures/setup.fixtures";
import { AgentsPublishPage } from "../../../pages/views/agents-publish.page";

test.describe("Agents terminal publish history", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.addInitScript(() => window.localStorage.clear());
  });

  test("merged workspace renders historical paged diffs without live changes", async ({
    page,
  }) => {
    await dismissProviderCliUpdateToasts(page);
    const publish = new AgentsPublishPage(page);
    await publish.openMergedPublishScenario();

    await expect(publish.terminalHeading).toBeVisible();
    await expect(
      page.getByText("a new workspace branch will be created automatically."),
    ).toBeVisible();
    await expect(publish.historicalFilter).toContainText("Published changes");
    await expect(publish.pagedDiffContent).toBeVisible();
    await expect(
      publish.inlineDiffs.getByTestId("file-diff-pre-hydration"),
    ).toHaveCount(0);
    await expect(
      publish.composerContext.getByText("Workspace changes"),
    ).toHaveCount(0);

    await expect(page).toHaveScreenshot("agents-terminal-publish-history.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("repair-pending action strip renders a calm status chip, not the accent CTA", async ({
    page,
  }) => {
    await dismissProviderCliUpdateToasts(page);
    const publish = new AgentsPublishPage(page);
    await publish.openRepairPendingScenario();

    const actionbar = page.getByTestId("agents-publish-actionbar");
    await expect(
      actionbar.getByRole("heading", { name: "Repair in progress" }),
    ).toBeVisible();
    const repairButton = actionbar.getByTestId("agents-publish-repair-pending");
    await expect(repairButton).toBeVisible();
    await expect(repairButton).toBeDisabled();
    await expect(
      actionbar.getByTestId("agents-pr-supervision-status"),
    ).toBeVisible();

    await expect(actionbar).toHaveScreenshot(
      "agents-publish-repair-pending-strip.png",
      {
        maxDiffPixelRatio: 0.01,
      },
    );
  });

  test("durable repair operation renders one live status action", async ({ page }) => {
    await dismissProviderCliUpdateToasts(page);
    const publish = new AgentsPublishPage(page);
    await publish.openMaintenanceRepairScenario();

    const actionbar = page.getByTestId("agents-publish-actionbar");
    await expect(
      actionbar.getByRole("heading", { name: "Repairing workspace" }),
    ).toBeVisible();
    await expect(actionbar.getByRole("status")).toContainText(
      "Will continue automatically.",
    );
    const operation = actionbar.getByTestId("agents-publish-maintenance-active");
    await expect(operation).toBeDisabled();
    await expect(actionbar.getByTestId("agents-publish-confirm")).toHaveCount(0);

    await expect(actionbar).toHaveScreenshot(
      "agents-publish-maintenance-repairing-strip.png",
      { maxDiffPixelRatio: 0.01 },
    );
  });
});
