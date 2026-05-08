import { expect, test } from "@playwright/test";

async function openAppWithGitAuthIssue(page: import("@playwright/test").Page) {
  await page.addInitScript(() => {
    window.__mockGhAuthStatus = false;
    window.__mockGitAuthDiagnostics = {
      fetchUrl: "https://github.com/mock/project.git",
      pushUrl: "git@github.com:mock/project.git",
      fetchKind: "HTTPS",
      pushKind: "SSH",
      mixedAuthModes: true,
      canSwitchToSsh: true,
      suggestedSshUrl: "git@github.com:mock/project.git",
    };
    window.localStorage.setItem(
      "ralphx-project-store",
      JSON.stringify({ state: { activeProjectId: "project-mock-1" }, version: 0 }),
    );
  });
  await page.goto("/");
  await page.waitForSelector('[data-testid="app-header"]', { timeout: 10000 });
}

test.describe("Git Auth Startup Warning", () => {
  test("warns before GitHub-dependent startup work", async ({ page }) => {
    await openAppWithGitAuthIssue(page);

    const toastContent = page.getByTestId("git-auth-startup-toast");
    await expect(toastContent).toBeVisible({ timeout: 6000 });

    const toast = page.locator("[data-sonner-toast]").filter({ has: toastContent });
    await expect(toast).toHaveScreenshot("git-auth-startup-toast.png");

    await toast.getByRole("button", { name: "Open Settings" }).click();
    await expect(page.getByTestId("settings-dialog")).toBeVisible();
    const repairPanel = page.getByTestId("git-auth-repair-panel");
    await expect(repairPanel).toBeVisible();
    await expect(page.getByTestId("git-auth-switch-ssh")).toBeVisible();
    await expect(page.getByTestId("git-auth-copy-gh-login")).toBeVisible();
    await expect(repairPanel).toHaveScreenshot("git-auth-startup-settings-repair-panel.png", {
      maxDiffPixelRatio: 0.01,
    });

    await page.getByTestId("git-auth-switch-ssh").click();
    const confirmation = page.getByRole("alertdialog");
    await expect(confirmation).toBeVisible();
    await expect(confirmation).toHaveScreenshot("git-auth-startup-switch-ssh-confirmation.png", {
      maxDiffPixelRatio: 0.01,
    });

    await confirmation.getByRole("button", { name: "Use SSH" }).click();
    await expect(page.getByTestId("git-auth-switch-ssh")).toBeHidden({ timeout: 6000 });
    await expect(page.getByTestId("git-auth-copy-gh-login")).toBeHidden();
    await expect(repairPanel).toBeHidden();
  });
});
