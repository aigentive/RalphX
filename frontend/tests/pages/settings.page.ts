import { expect, Page, Locator } from "@playwright/test";
import { BasePage } from "./base.page";

export class SettingsPage extends BasePage {
  // Modal container (replaces old settings-view full-page shell)
  readonly settingsDialog: Locator;
  readonly settingsTitle: Locator;
  readonly closeButton: Locator;
  readonly savingIndicator: Locator;
  readonly errorBanner: Locator;
  readonly agentRuntimePicker: Locator;
  readonly updateChannelGroup: Locator;
  readonly stableUpdateChannel: Locator;
  readonly nightlyUpdateChannel: Locator;
  readonly updateChannelSaveError: Locator;

  // Execution Section
  readonly executionSection: Locator;
  readonly maxConcurrentTasksInput: Locator;
  readonly projectIdeationMaxInput: Locator;
  readonly globalMaxConcurrentInput: Locator;
  readonly globalIdeationMaxInput: Locator;
  readonly allowIdeationBorrowIdleExecutionToggle: Locator;

  // Review Section
  readonly reviewSection: Locator;
  readonly requireHumanReviewToggle: Locator;
  readonly maxFixAttemptsInput: Locator;
  readonly maxRevisionCyclesInput: Locator;

  // External MCP Section
  readonly externalMcpSection: Locator;
  readonly externalMcpEnabledToggle: Locator;
  readonly externalMcpHostInput: Locator;
  readonly externalMcpPortInput: Locator;
  readonly externalMcpAuthTokenInput: Locator;
  readonly externalMcpNodePathInput: Locator;
  readonly externalMcpSaveButton: Locator;

  constructor(page: Page) {
    super(page);

    // Main dialog element (modal overlay)
    this.settingsDialog = page.locator('[data-testid="settings-dialog"]');
    this.settingsTitle = this.settingsDialog.locator("text=Settings").first();
    this.closeButton = this.settingsDialog.getByRole("button", { name: "Close settings" });
    this.savingIndicator = page.locator("text=Saving...");
    this.errorBanner = page.locator('[role="alert"]');
    this.agentRuntimePicker = this.settingsDialog.getByTestId("agent-composer-runtime-pill");
    this.updateChannelGroup = this.settingsDialog.getByRole("radiogroup", {
      name: "Update channel",
    });
    this.stableUpdateChannel = this.updateChannelGroup.getByRole("radio", {
      name: "Stable — Recommended",
    });
    this.nightlyUpdateChannel = this.updateChannelGroup.getByRole("radio", {
      name: "Nightly — Early access",
    });
    this.updateChannelSaveError = this.settingsDialog.getByRole("alert").filter({
      hasText: "Unable to save update channel",
    });

    // Execution Section
    this.executionSection = page.locator("text=Control task execution behavior and concurrency").locator("..");
    this.maxConcurrentTasksInput = page.locator('[data-testid="max-concurrent-tasks"]');
    this.projectIdeationMaxInput = page.locator('[data-testid="project-ideation-max"]');
    this.globalMaxConcurrentInput = page.locator('[data-testid="global-max-concurrent"]');
    this.globalIdeationMaxInput = page.locator('[data-testid="global-ideation-max"]');
    this.allowIdeationBorrowIdleExecutionToggle = page.locator('[data-testid="allow-ideation-borrow-idle-execution"]');

    // Review Section
    this.reviewSection = page.locator("text=Configure global review policy for all projects").locator("..");
    this.requireHumanReviewToggle = page.locator('[data-testid="require-human-review"]');
    this.maxFixAttemptsInput = page.locator('[data-testid="max-fix-attempts"]');
    this.maxRevisionCyclesInput = page.locator('[data-testid="max-revision-cycles"]');

    // External MCP Section
    this.externalMcpSection = page.locator("text=Configure external MCP server access").locator("..");
    this.externalMcpEnabledToggle = page.locator('[data-testid="ext-mcp-enabled"]');
    this.externalMcpHostInput = page.locator('[data-testid="ext-mcp-host"]');
    this.externalMcpPortInput = page.locator('[data-testid="ext-mcp-port"]');
    this.externalMcpAuthTokenInput = page.locator('[data-testid="ext-mcp-auth-token"]');
    this.externalMcpNodePathInput = page.locator('[data-testid="ext-mcp-node-path"]');
    this.externalMcpSaveButton = page.locator('[data-testid="ext-mcp-save"]');
  }

  /** Open settings dialog by clicking the nav button */
  async openViaNavigation() {
    await this.page.click('[data-testid="nav-settings"]');
    await this.settingsDialog.waitFor({ state: "visible" });
  }

  /** Open settings dialog via uiStore.openModal (web-mode shortcut) */
  async openViaStore(section?: string) {
    await this.page.evaluate((sec) => {
      const uiStore = (window as unknown as { __uiStore?: { getState(): { openModal(type: string, ctx?: Record<string, unknown>): void } } }).__uiStore;
      if (uiStore) {
        uiStore.getState().openModal("settings", sec ? { section: sec } : undefined);
      }
    }, section);
    await this.settingsDialog.waitFor({ state: "visible" });
  }

  /** Open settings dialog via keyboard shortcut ⌘7 */
  async openViaKeyboard() {
    await this.page.keyboard.press("Meta+7");
    await this.settingsDialog.waitFor({ state: "visible" });
  }

  /**
   * Select a leaf section. Prefers the real user path — nav entry, then leaf
   * tab or Integrations hub card — and falls back to the store deep link when
   * the rail is collapsed (narrow viewports).
   */
  async selectSection(sectionId: string) {
    const leafTab = this.settingsDialog.locator(
      `[data-testid="settings-leaf-${sectionId}"]`,
    );
    if (await leafTab.isVisible()) {
      await leafTab.click();
      return;
    }
    const hubCard = this.settingsDialog.locator(
      `[data-testid="integration-card-${sectionId}"]`,
    );
    if (await hubCard.isVisible()) {
      await hubCard.getByRole("button").click();
      return;
    }
    await this.openViaStore(sectionId);
  }

  /** Click one of the seven consolidated nav entries. */
  async selectNav(navId: string) {
    await this.settingsDialog.getByTestId(`settings-nav-${navId}`).click();
  }

  async waitForSection(sectionId: string, heading: string) {
    await expect(
      this.settingsDialog.locator('.settings-nav__item[aria-current="page"]'),
    ).toHaveCount(1, { timeout: 10000 });
    await expect(this.page.getByTestId("settings-section-loading")).toBeHidden({
      timeout: 10000,
    });
    const leafTab = this.settingsDialog.locator(
      `[data-testid="settings-leaf-${sectionId}"]`,
    );
    if (await leafTab.count()) {
      await expect(leafTab).toHaveAttribute("aria-selected", "true", {
        timeout: 10000,
      });
    }
    // Scoped to the page body: the nav-level h1 can carry the same words as a
    // section's own card heading (e.g. "Agents", "Repository").
    await expect(
      this.settingsDialog
        .locator(".settings-page__body")
        .getByRole("heading", { name: heading, exact: true }),
    ).toBeVisible({ timeout: 10000 });
  }

  async selectUpdateChannel(channel: "stable" | "nightly") {
    await (channel === "stable"
      ? this.stableUpdateChannel
      : this.nightlyUpdateChannel
    ).click();
  }

  /** Close the settings dialog via the close button */
  async closeModal() {
    await this.closeButton.click();
    await this.settingsDialog.waitFor({ state: "hidden" });
  }

  async waitForSettingsLoaded() {
    await this.settingsDialog.waitFor({ state: "visible" });
    await this.waitForAnimations();
  }

  async isToggleEnabled(toggle: Locator): Promise<boolean> {
    const state = await toggle.getAttribute("data-state");
    return state === "checked";
  }

  async getInputValue(input: Locator): Promise<string> {
    return (await input.inputValue()) || "";
  }
}
