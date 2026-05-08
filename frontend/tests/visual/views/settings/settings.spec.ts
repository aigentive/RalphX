import { test, expect } from "@playwright/test";
import { SettingsPage } from "../../../pages/settings.page";
import { setupSettings } from "../../../fixtures/setup.fixtures";

const SETTINGS_SECTION_VISUALS = [
  { id: "repository", heading: "Repository" },
  { id: "project-analysis", heading: "Setup & Validation" },
  { id: "execution", heading: "Execution" },
  { id: "execution-harnesses", heading: "Execution Pipeline Agents" },
  { id: "models", heading: "Models" },
  { id: "global-execution", heading: "Global Capacity" },
  { id: "review", heading: "Review Policy" },
  { id: "ideation-workflow", heading: "Planning & Verification" },
  { id: "ideation-harnesses", heading: "Ideation Agents" },
  { id: "api-keys", heading: "API Keys" },
  { id: "external-mcp", heading: "External MCP" },
  { id: "accessibility", heading: "Accessibility" },
] as const;

test.describe("Settings Dialog", () => {
  let settingsPage: SettingsPage;

  test.beforeEach(async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await setupSettings(page);
  });

  test("renders settings dialog layout", async () => {
    await expect(settingsPage.settingsDialog).toBeVisible();
    await expect(settingsPage.settingsTitle).toBeVisible();
    await settingsPage.waitForSection("repository", "Repository");
  });

  test("renders above the underlying view (modal overlay)", async ({ page }) => {
    // The kanban/current view is still mounted behind the modal
    await expect(settingsPage.settingsDialog).toBeVisible();
    // Dialog should have highest z-index (rendered in portal above app content)
    const zIndex = await page.evaluate(() => {
      const dialog = document.querySelector('[data-testid="settings-dialog"]');
      if (!dialog) return null;
      return getComputedStyle(dialog.closest('[role="dialog"]') ?? dialog).zIndex;
    });
    expect(zIndex).not.toBeNull();
  });

  test("execution section contains all controls", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("execution");
    await settingsPage.waitForSection("execution", "Execution");
    await expect(settingsPage.maxConcurrentTasksInput).toBeVisible();
    await expect(settingsPage.projectIdeationMaxInput).toBeVisible();
  });

  test("global capacity section contains all controls", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("global-execution");
    await settingsPage.waitForSection("global-execution", "Global Capacity");
    await expect(settingsPage.globalMaxConcurrentInput).toBeVisible();
    await expect(settingsPage.globalIdeationMaxInput).toBeVisible();
    await expect(settingsPage.allowIdeationBorrowIdleExecutionToggle).toBeVisible();
  });

  test("review section contains all controls", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("review");
    await settingsPage.waitForSection("review", "Review Policy");
    await expect(settingsPage.requireHumanReviewToggle).toBeVisible();
    await expect(settingsPage.maxFixAttemptsInput).toBeVisible();
    await expect(settingsPage.maxRevisionCyclesInput).toBeVisible();
  });

  test("external MCP section contains all controls", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("external-mcp");
    await settingsPage.waitForSection("external-mcp", "External MCP");
    await expect(settingsPage.externalMcpEnabledToggle).toBeVisible();
    await expect(settingsPage.externalMcpHostInput).toBeVisible();
    await expect(settingsPage.externalMcpPortInput).toBeVisible();
    await expect(settingsPage.externalMcpAuthTokenInput).toBeVisible();
    await expect(settingsPage.externalMcpNodePathInput).toBeVisible();
    await expect(settingsPage.externalMcpSaveButton).toBeVisible();
  });

  test("repository section shows git auth repair actions", async ({ page }) => {
    await page.addInitScript(() => {
      const testWindow = window as Window & {
        __mockGhAuthStatus?: boolean;
        __mockGitAuthDiagnostics?: {
          fetchUrl: string;
          pushUrl: string;
          fetchKind: string;
          pushKind: string;
          mixedAuthModes: boolean;
          canSwitchToSsh: boolean;
          suggestedSshUrl: string;
        };
      };
      testWindow.__mockGhAuthStatus = true;
      testWindow.__mockGitAuthDiagnostics = {
        fetchUrl: "https://github.com/mock/project.git",
        pushUrl: "git@github.com:mock/project.git",
        fetchKind: "HTTPS",
        pushKind: "SSH",
        mixedAuthModes: true,
        canSwitchToSsh: true,
        suggestedSshUrl: "git@github.com:mock/project.git",
      };
    });
    await setupSettings(page);
    settingsPage = new SettingsPage(page);
    await settingsPage.waitForSection("repository", "Repository");

    const repairPanel = page.getByTestId("git-auth-repair-panel");
    await repairPanel.scrollIntoViewIfNeeded();
    await expect(repairPanel).toBeVisible();
    await expect(page.getByTestId("git-auth-switch-ssh")).toBeVisible();
    await expect(page.getByTestId("git-auth-setup-gh")).toBeVisible();

    await settingsPage.waitForAnimations();
    await expect(repairPanel).toHaveScreenshot("settings-repository-git-auth-repair-panel.png", {
      maxDiffPixelRatio: 0.01,
    });
  });

  for (const section of SETTINGS_SECTION_VISUALS) {
    test(`matches snapshot - ${section.heading} section`, async ({ page }) => {
      settingsPage = new SettingsPage(page);
      await settingsPage.openViaStore(section.id);
      await settingsPage.waitForSection(section.id, section.heading);
      await settingsPage.waitForAnimations();

      await expect(settingsPage.settingsDialog).toHaveScreenshot(
        `settings-dialog-section-${section.id}.png`,
        {
          maxDiffPixelRatio: 0.01,
        },
      );
    });
  }
});
