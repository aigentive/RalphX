// Temporary audit spec: captures every settings section for visual review.
// Not committed — deleted after the audit.
import { test } from "@playwright/test";
import { SettingsPage } from "../../../pages/settings.page";
import { setupSettings } from "../../../fixtures/setup.fixtures";

const OUT = "../.artifacts/settings-audit";

const SECTIONS = [
  "providers",
  "models",
  "mcp",
  "agents",
  "personas",
  "capabilities",
  "tasks",
  "planning",
  "workspace",
  "capacity",
  "repository",
  "project-analysis",
  "integrations-hub",
  "github",
  "api-keys",
  "external-mcp",
  "notifications",
  "updates",
  "accessibility",
] as const;

test.describe("Settings audit capture", () => {
  for (const section of SECTIONS) {
    test(`capture ${section}`, async ({ page }) => {
      const settingsPage = new SettingsPage(page);
      await setupSettings(page);
      await settingsPage.openViaStore(section);
      await page
        .getByTestId("settings-section-loading")
        .waitFor({ state: "hidden", timeout: 10000 })
        .catch(() => undefined);
      await page.waitForTimeout(800);
      await settingsPage.settingsDialog.screenshot({
        path: `${OUT}/${section}.png`,
      });
    });
  }
});
