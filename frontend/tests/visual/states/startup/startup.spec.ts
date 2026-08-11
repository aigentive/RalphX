import { expect, test } from "@playwright/test";

import { StartupPage } from "../../../pages/startup.page";

test.describe("Startup surface", () => {
  test("shows a long-running light startup without an app-shell backdrop", async ({ page }) => {
    const startupPage = new StartupPage(page);
    await startupPage.open("long-running", "light");

    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect(startupPage.status).toContainText("Upgrading workspace data");
    await expect(startupPage.status).toContainText("Still working after 1 minute");
    await expect(page.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "2");
    await expect(page).toHaveScreenshot("startup-long-running-light.png", { fullPage: true });
  });

  test("keeps app-state-ready startup accessible in dark reduced-motion mode", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
    const startupPage = new StartupPage(page);
    await startupPage.open("app-state-ready", "dark");

    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(startupPage.status).toContainText("Preparing the app shell");
    await expect(page).toHaveScreenshot("startup-app-state-ready-dark-reduced-motion.png", {
      fullPage: true,
    });
  });

  test("shows runtime-ready background restoration on a narrow viewport", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    const startupPage = new StartupPage(page);
    await startupPage.open("background-restoring", "dark");

    await expect(startupPage.screen).toHaveCount(0);
    await expect(page.getByText("Restoring background work…")).toBeVisible();
    const safeAction = page.getByTestId("safe-shell-action");
    await safeAction.click();
    await expect(safeAction).toHaveText("Workspace opened");
    await expect(page).toHaveScreenshot("startup-background-restoring-narrow.png", {
      fullPage: true,
    });
  });

  test("keeps a failed startup on the accessible recovery surface", async ({ page }) => {
    const startupPage = new StartupPage(page);
    await startupPage.open("failed", "dark");

    await expect(startupPage.status).toContainText("RalphX could not finish starting");
    await expect(startupPage.retryButton).toBeVisible();
    await expect(page.getByRole("button", { name: "Open Logs" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Copy Diagnostics" })).toBeVisible();
    await expect(page).toHaveScreenshot("startup-failed.png", { fullPage: true });
  });
});
