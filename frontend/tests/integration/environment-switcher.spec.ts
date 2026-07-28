import { expect, test } from "@playwright/test";

import { dismissProviderCliUpdateToasts, setupApp } from "../fixtures/setup.fixtures";
import { EnvironmentSwitcherPage } from "../pages/components/environment-switcher.page";

test.describe("Environment switcher journey", () => {
  test("switches from local to remote and back in the top bar", async ({ page }) => {
    await dismissProviderCliUpdateToasts(page);
    await setupApp(page);
    const switcher = new EnvironmentSwitcherPage(page);
    await switcher.seedTwoEnvironmentRegistry();

    await expect(switcher.trigger).toContainText("This Mac");
    await switcher.open();
    await switcher.switchTo("Studio-Mac");
    await switcher.open();
    await switcher.switchTo("This Mac");
  });
});
