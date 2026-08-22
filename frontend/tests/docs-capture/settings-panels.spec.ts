import { expect, test } from "@playwright/test";

import { captureGuideScreenshot, setupCapture } from "./fixtures/capture.fixtures";
import { applyGuideScenario } from "./fixtures/guide-scenario.fixtures";
import { SettingsPage } from "../pages/settings.page";

test("settings-atlassian", async ({ page }) => {
  await setupCapture(page);
  await applyGuideScenario(page, "guide_settings_atlassian");
  const settings = new SettingsPage(page);
  await settings.openViaStore("integrations");
  await settings.waitForSection("integrations");
  await captureGuideScreenshot(page, "settings-atlassian.png");
});

test("providers-claude-card", async ({ page }) => {
  await setupCapture(page);
  await applyGuideScenario(page, "guide_settings_providers");
  const settings = new SettingsPage(page);
  await settings.openViaStore("providers");
  await settings.waitForSection("providers");
  await expect(page.getByText("claude", { exact: false }).first()).toBeVisible();
  await captureGuideScreenshot(page, "providers-claude-card.png");
});

test("providers-cli-not-ready", async ({ page }) => {
  await setupCapture(page);
  await applyGuideScenario(page, "guide_providers_cli_not_ready");
  // Override the providers query to show CLI not-ready state before opening settings.
  await page.evaluate(() => {
    if (!window.__queryClient) return;
    const notReadySettings = {
      providers: [
        {
          provider: "codex",
          enabled: true,
          isDefault: true,
          model: "gpt-5.5",
          effort: "medium",
          serviceTier: null,
          approvalPolicy: "never",
          sandboxMode: "danger-full-access",
          claudePermissionMode: null,
          claudeDangerouslySkipPermissions: false,
          claudeAllowDangerouslySkipPermissions: false,
          cliManagementMode: "user_managed",
          autoUpdateEnabled: false,
          customBinaryEnabled: false,
          customBinaryPath: null,
          customEnvFileEnabled: false,
          customEnvFilePath: null,
          available: false,
          binaryFound: false,
          binaryPath: null,
          status: "CLI binary not found. Install Codex CLI to proceed.",
          error: "binary not found",
          missingCoreExecFeatures: [],
          cliVersion: null,
          supportedModelAliases: null,
          supportedEfforts: null,
          ultraSupportedModels: [],
          supportsFastMode: false,
          fastModeSupportedModels: [],
          updatedAt: "2026-06-15T10:00:00Z",
        },
        {
          provider: "claude",
          enabled: false,
          isDefault: false,
          model: "claude-sonnet-5",
          effort: null,
          serviceTier: null,
          approvalPolicy: "never",
          sandboxMode: null,
          claudePermissionMode: "bypassPermissions",
          claudeDangerouslySkipPermissions: true,
          claudeAllowDangerouslySkipPermissions: true,
          cliManagementMode: "user_managed",
          autoUpdateEnabled: false,
          customBinaryEnabled: false,
          customBinaryPath: null,
          customEnvFileEnabled: false,
          customEnvFilePath: null,
          available: false,
          binaryFound: false,
          binaryPath: null,
          status: "CLI binary not found. Install Claude CLI to proceed.",
          error: "binary not found",
          missingCoreExecFeatures: [],
          cliVersion: null,
          supportedModelAliases: null,
          supportedEfforts: null,
          ultraSupportedModels: [],
          supportsFastMode: false,
          fastModeSupportedModels: [],
          updatedAt: "2026-06-15T10:00:00Z",
        },
      ],
      defaultProvider: null,
      requiresOnboarding: true,
    };
    window.__queryClient.setQueryData(
      ["agent", "providers", { refreshRuntime: false }],
      notReadySettings,
    );
    window.__queryClient.setQueryData(
      ["agent", "providers", { refreshRuntime: true }],
      notReadySettings,
    );
  });
  const settings = new SettingsPage(page);
  await settings.openViaStore("providers");
  await settings.waitForSection("providers");
  await expect(page.getByText("CLI Not Ready").first()).toBeVisible();
  await captureGuideScreenshot(page, "providers-cli-not-ready.png");
});

test("settings-capacity", async ({ page }) => {
  await setupCapture(page);
  await applyGuideScenario(page, "guide_settings_capacity");
  const settings = new SettingsPage(page);
  await settings.openViaStore("capacity");
  await settings.waitForSection("capacity");
  await captureGuideScreenshot(page, "settings-capacity.png");
});
