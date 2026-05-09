import { expect, Page, Locator } from "@playwright/test";
import { BasePage } from "./base.page";

export class SettingsPage extends BasePage {
  // Modal container (replaces old settings-view full-page shell)
  readonly settingsDialog: Locator;
  readonly settingsTitle: Locator;
  readonly closeButton: Locator;
  readonly savingIndicator: Locator;
  readonly errorBanner: Locator;

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

  /** Select a section by its ID using the left-rail navigation */
  async selectSection(sectionId: string) {
    const sectionButton = this.settingsDialog.locator(
      `[data-testid="settings-section-${sectionId}"]`,
    );
    if (await sectionButton.isVisible()) {
      await sectionButton.click();
    } else {
      await this.openViaStore(sectionId);
    }
  }

  async waitForSection(sectionId: string, heading: string) {
    const sectionButton = this.settingsDialog.locator(
      `[data-testid="settings-section-${sectionId}"]`,
    );
    await expect(sectionButton).toHaveAttribute("aria-current", "page", {
      timeout: 10000,
    });
    await expect(this.page.getByTestId("settings-section-loading")).toBeHidden({
      timeout: 10000,
    });
    await expect(
      this.settingsDialog.getByRole("heading", { name: heading, exact: true }),
    ).toBeVisible({ timeout: 10000 });
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
