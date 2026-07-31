import { test, expect } from "@playwright/test";
import { SettingsPage } from "../../../pages/settings.page";
import { setupSettings } from "../../../fixtures/setup.fixtures";

// `label` only names the snapshot test; sections carry no heading of their own.
const SETTINGS_SECTION_VISUALS = [
  { id: "providers", label: "Providers" },
  { id: "models", label: "Models" },
  { id: "repository", label: "Repository" },
  { id: "project-analysis", label: "Setup & Validation" },
  { id: "agents", label: "Default new-run mode" },
  { id: "tasks", label: "Task policies" },
  { id: "planning", label: "Plan verification" },
  { id: "github", label: "GitHub" },
  { id: "api-keys", label: "API Keys" },
  { id: "mcp", label: "MCP" },
  { id: "updates", label: "Updates" },
  { id: "accessibility", label: "Accessibility" },
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
    await settingsPage.waitForSection("providers");
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

  test("External MCP section contains bridge controls", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("external-mcp");
    await settingsPage.waitForSection("external-mcp");
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
    await settingsPage.openViaStore("repository");
    await settingsPage.waitForSection("repository");

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
    test(`matches snapshot - ${section.label} section`, async ({ page }) => {
      settingsPage = new SettingsPage(page);
      await settingsPage.openViaStore(section.id);
      await settingsPage.waitForSection(section.id);
      if (section.id === "agents") {
        await expect(
          settingsPage.settingsDialog.getByTestId("agent-family-row").first(),
        ).toBeVisible({ timeout: 10000 });
      }
      await settingsPage.waitForAnimations();

      await expect(settingsPage.settingsDialog).toHaveScreenshot(
        `settings-dialog-section-${section.id}.png`,
        {
          maxDiffPixelRatio: 0.035,
        },
      );
    });
  }

  test("matches the consolidated nav rail and page header", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("providers");
    await settingsPage.waitForSection("providers");
    await expect(
      settingsPage.settingsDialog.getByTestId("settings-page-title"),
    ).toHaveText("Models & Providers");
    await settingsPage.waitForAnimations();

    await expect(
      settingsPage.settingsDialog.locator(".settings-nav"),
    ).toHaveScreenshot("settings-dialog-nav-rail.png", {
      maxDiffPixelRatio: 0.01,
    });
  });

  test("matches the Integrations hub grid", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("integrations-hub");
    await expect(
      settingsPage.settingsDialog.getByTestId("integrations-hub"),
    ).toBeVisible({ timeout: 10000 });
    await settingsPage.waitForAnimations();

    await expect(settingsPage.settingsDialog).toHaveScreenshot(
      "settings-dialog-section-integrations-hub.png",
      { maxDiffPixelRatio: 0.01 },
    );
  });

  test("drills into an integration panel and back to the hub", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("integrations-hub");
    await settingsPage.selectSection("github");
    await settingsPage.waitForSection("github");

    const back = settingsPage.settingsDialog.getByTestId(
      "settings-drill-in-back",
    );
    await expect(back).toBeVisible();
    await settingsPage.waitForAnimations();
    await expect(settingsPage.settingsDialog).toHaveScreenshot(
      "settings-dialog-integrations-drill-in.png",
      { maxDiffPixelRatio: 0.01 },
    );

    await back.click();
    await expect(
      settingsPage.settingsDialog.getByTestId("integrations-hub"),
    ).toBeVisible();
  });

  test("matches the settings search results dropdown", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("providers");
    await settingsPage.waitForSection("providers");

    // The focused input's blinking caret and the racy post-fill text
    // selection make the element screenshot unstable — hide both.
    await page.addStyleTag({
      content:
        ".settings-search__input { caret-color: transparent; } " +
        ".settings-search__input::selection { background-color: transparent; color: inherit; }",
    });
    const search = settingsPage.settingsDialog.getByRole("searchbox", {
      name: "Search settings",
    });
    await search.fill("review");
    // fill() can leave the inserted text selected, which paints an unstable
    // highlight in the screenshot — collapse the selection deterministically.
    await search.press("End");
    await expect(
      settingsPage.settingsDialog.getByRole("listbox", {
        name: "Settings search results",
      }),
    ).toBeVisible();
    await settingsPage.waitForAnimations();

    await expect(
      settingsPage.settingsDialog.getByTestId("settings-search"),
    ).toHaveScreenshot("settings-dialog-search-box.png", {
      maxDiffPixelRatio: 0.01,
    });

    // The dropdown is absolutely positioned, so it falls outside the search
    // box's own bounds — capture the listbox directly.
    await expect(
      settingsPage.settingsDialog.getByRole("listbox", {
        name: "Settings search results",
      }),
    ).toHaveScreenshot("settings-dialog-search-results.png", {
      maxDiffPixelRatio: 0.01,
    });
  });

  test("matches the settings search empty state", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("providers");
    await settingsPage.waitForSection("providers");

    await page.addStyleTag({ content: ".settings-search__input { caret-color: transparent; }" });
    await settingsPage.settingsDialog
      .getByRole("searchbox", { name: "Search settings" })
      .fill("zzzznomatch");

    const empty = settingsPage.settingsDialog.getByRole("status");
    await expect(empty).toBeVisible();
    await settingsPage.waitForAnimations();

    await expect(empty).toHaveScreenshot("settings-dialog-search-empty.png", {
      maxDiffPixelRatio: 0.01,
    });
  });

  test("Updates defaults to Stable and persists the Nightly radio selection", async () => {
    await settingsPage.openViaStore("updates");
    await settingsPage.waitForSection("updates");

    await expect(settingsPage.updateChannelGroup).toBeVisible();
    await expect(settingsPage.stableUpdateChannel).toHaveAttribute("aria-checked", "true");
    await expect(settingsPage.nightlyUpdateChannel).toHaveAttribute("aria-checked", "false");

    await settingsPage.selectUpdateChannel("nightly");
    await expect(settingsPage.nightlyUpdateChannel).toHaveAttribute("aria-checked", "true");
    await expect(settingsPage.nightlyUpdateChannel).toContainText(
      "Switching back to Stable stops future Nightly delivery but never downgrades the installed app; updates resume when Stable advances beyond it.",
    );
    await settingsPage.waitForAnimations();

    await expect(settingsPage.settingsDialog).toHaveScreenshot(
      "settings-dialog-section-updates-nightly.png",
      { maxDiffPixelRatio: 0.035 },
    );
  });

  test("matches the Updates save error state", async ({ page }) => {
    await page.addInitScript(() => {
      (
        window as Window & { __mockUpdateChannelError?: "read" | "write" }
      ).__mockUpdateChannelError = "write";
    });
    await setupSettings(page);
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("updates");
    await settingsPage.waitForSection("updates");

    await settingsPage.selectUpdateChannel("nightly");
    await expect(settingsPage.updateChannelSaveError).toBeVisible();
    await settingsPage.waitForAnimations();

    await expect(settingsPage.settingsDialog).toHaveScreenshot(
      "settings-dialog-section-updates-save-error.png",
      { maxDiffPixelRatio: 0.035 },
    );
  });

  test("matches populated Agents expanded editor", async ({ page }) => {
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("agents");
    await settingsPage.waitForSection("agents");
    await settingsPage.settingsDialog
      .getByTestId("agent-family-row")
      .first()
      .getByRole("button")
      .first()
      .click();
    await settingsPage.settingsDialog
      .getByRole("button", { name: "Edit Edit" })
      .click();
    await expect(settingsPage.agentRuntimePicker).toBeVisible();
    await settingsPage.waitForAnimations();

    await expect(settingsPage.settingsDialog).toHaveScreenshot(
      "settings-dialog-section-agents-expanded.png",
      { maxDiffPixelRatio: 0.01 },
    );
  });

  test("matches populated Agents narrow layout", async ({ page }) => {
    await page.setViewportSize({ width: 760, height: 900 });
    settingsPage = new SettingsPage(page);
    await settingsPage.openViaStore("agents");
    await settingsPage.waitForSection("agents");
    await expect(
      settingsPage.settingsDialog.getByTestId("agent-family-row").first(),
    ).toBeVisible({ timeout: 10000 });
    await settingsPage.settingsDialog
      .getByTestId("agent-family-row")
      .first()
      .getByRole("button")
      .first()
      .click();
    await settingsPage.settingsDialog
      .getByRole("button", { name: "Edit Edit" })
      .click();
    await expect(settingsPage.agentRuntimePicker).toBeVisible();
    await settingsPage.waitForAnimations();
    await expect(
      settingsPage.settingsDialog
        .getByTestId("agent-family-row")
        .first()
        .getByRole("button")
        .first(),
    ).toHaveAttribute("aria-expanded", "true");
    await expect(settingsPage.agentRuntimePicker).toBeVisible();
    await settingsPage.agentRuntimePicker.scrollIntoViewIfNeeded();

    await expect(settingsPage.settingsDialog).toHaveScreenshot(
      "settings-dialog-section-agents-narrow.png",
      { maxDiffPixelRatio: 0.01 },
    );
    await expect(
      settingsPage.settingsDialog.locator(
        '[data-testid="manual-role-row"]:has(button[aria-label="Edit Edit"])',
      ),
    ).toHaveScreenshot("settings-dialog-section-agents-narrow-editor.png", {
      maxDiffPixelRatio: 0.01,
    });
  });
});
