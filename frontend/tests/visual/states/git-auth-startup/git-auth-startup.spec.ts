import { expect, test } from "@playwright/test";
import { seedMockNotificationHistory } from "../../../fixtures/setup.fixtures";

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

test.describe("Git Auth Startup Notification", () => {
  test("shows the durable Git authentication alert in notification history", async ({ page }) => {
    await openAppWithGitAuthIssue(page);
    await seedMockNotificationHistory(page, [{
      id: "notification-mock-git-auth-preflight",
      createdAt: new Date().toISOString(),
      projectId: null,
      category: "git_auth_preflight",
      severity: "warning",
      title: "Git authentication needs attention",
      body: "1 project blocked by Git or GitHub authentication",
      target: { kind: "none" },
      dedupeKey: "git-auth-preflight",
      readAt: null,
    }]);

    await page.getByTestId("reviews-toggle").click();
    await expect(page.getByTestId("notifications-panel")).toBeVisible();
    await page.getByRole("tab", { name: "History" }).click();

    const notification = page.getByTestId("notification-history-row-notification-mock-git-auth-preflight");
    await expect(notification).toBeVisible();
    await expect(notification).toContainText("Git authentication needs attention");
    await expect(notification).toHaveScreenshot("git-auth-startup-notification-row.png");
  });
});
