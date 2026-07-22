import { expect, test } from "@playwright/test";

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
});
