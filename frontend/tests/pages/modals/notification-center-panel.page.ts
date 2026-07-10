import { Page, Locator } from "@playwright/test";
import { BasePage } from "../base.page";

/**
 * Page object for the notification center.
 */
export class NotificationCenterPanelPage extends BasePage {
  readonly reviewsToggle: Locator;
  readonly panel: Locator;
  readonly closeButton: Locator;
  readonly needsActionTab: Locator;
  readonly historyTab: Locator;
  readonly taskCards: Locator;
  readonly emptyState: Locator;
  readonly loadingSpinner: Locator;

  constructor(page: Page) {
    super(page);

    this.reviewsToggle = page.locator('[data-testid="reviews-toggle"]');
    this.panel = page.locator('[data-testid="notifications-panel"]');
    this.closeButton = page.locator('[data-testid="notifications-panel-close"]');
    this.needsActionTab = page.getByRole('tab', { name: /Needs action/ });
    this.historyTab = page.getByRole('tab', { name: /History/ });
    this.taskCards = page.locator('[data-testid^="task-review-card-"]');
    this.emptyState = page.locator('[data-testid="attention-empty-state"]');
    this.loadingSpinner = page.locator('[data-testid="notification-skeletons"]');
  }

  async openPanel() {
    await this.reviewsToggle.click();
    await this.panel.waitFor({ state: "visible", timeout: 5000 });
  }

  async closePanel() {
    await this.closeButton.click();
    await this.panel.waitFor({ state: "hidden", timeout: 5000 });
  }

  async switchToNeedsActionTab() {
    await this.needsActionTab.click();
  }

  async switchToHistoryTab() {
    await this.historyTab.click();
  }

  async getTaskCardCount(): Promise<number> {
    return await this.taskCards.count();
  }
}
