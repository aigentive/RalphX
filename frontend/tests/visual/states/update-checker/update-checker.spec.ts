import { test, expect } from "@playwright/test";

async function openAppWithMockUpdate(page: import("@playwright/test").Page) {
  await page.addInitScript(() => {
    window.localStorage.setItem("ralphx-mock-update", "available");
  });
  await page.goto("/");
  await page.waitForSelector('[data-testid="app-header"]', { timeout: 10000 });
}

test.describe("Update Checker", () => {
  test("shows an available update toast", async ({ page }) => {
    await openAppWithMockUpdate(page);

    const toastContent = page.getByTestId("update-available-toast");
    await expect(toastContent).toBeVisible({ timeout: 6000 });

    const toast = page.locator("[data-sonner-toast]").filter({ has: toastContent });
    await expect(toast).toHaveScreenshot("update-available-toast.png");
  });

  test("shows install success after update action", async ({ page }) => {
    await openAppWithMockUpdate(page);

    await expect(page.getByTestId("update-available-toast")).toBeVisible({
      timeout: 6000,
    });
    await page.getByTestId("update-install-button").click();

    const successToast = page
      .locator("[data-sonner-toast]")
      .filter({ hasText: "Update installed! Restarting..." });
    await expect(successToast).toBeVisible({ timeout: 6000 });
    await expect(successToast).toHaveScreenshot("update-install-success.png");
  });
});
