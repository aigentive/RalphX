import { expect, test } from "@playwright/test";

import { setMockProjectRepositoryCapability } from "../../../helpers/repository-capability.helpers";
import { RepositorySettingsPage } from "../../../pages/modals/repository-settings.page";
import { SettingsPage } from "../../../pages/settings.page";
import { setupApp } from "../../../fixtures/setup.fixtures";

test.describe("Repository capability settings", () => {
  test("renders an explicit local-only repository state", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await setupApp(page);
    await setMockProjectRepositoryCapability(
      page,
      { kind: "localOnly" },
      false,
    );

    const settingsPage = new SettingsPage(page);
    const repositoryPage = new RepositorySettingsPage(page);
    await settingsPage.openViaStore("repository");
    await settingsPage.waitForSection("repository");
    await repositoryPage.expectLocalOnlyState();
    await settingsPage.waitForAnimations();

    await expect(settingsPage.settingsDialog).toHaveScreenshot(
      "settings-repository-local-only.png",
      { maxDiffPixelRatio: 0.01 },
    );
  });
});
