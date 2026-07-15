import { Page, Locator } from "@playwright/test";
import { BasePage } from "../base.page";

export interface NotificationDrawerRect {
  left: number;
  right: number;
  top: number;
  bottom: number;
  width: number;
  height: number;
}

export interface NotificationDrawerGeometry {
  viewportWidth: number;
  documentScrollWidth: number;
  shell: NotificationDrawerRect;
  frame: NotificationDrawerRect;
  panel: NotificationDrawerRect;
}

export interface NotificationDrawerOverflow {
  documentOverflowPixels: number;
  shellViewportOverflowPixels: number;
  panelScrollOverflowPixels: number;
}

export interface NotificationContainedElement {
  testId: string;
  leftOverflowPixels: number;
  rightOverflowPixels: number;
  width: number;
}

/**
 * Page object for the notification center.
 */
export class NotificationCenterPanelPage extends BasePage {
  readonly reviewsToggle: Locator;
  readonly shell: Locator;
  readonly frame: Locator;
  readonly backdrop: Locator;
  readonly panel: Locator;
  readonly closeButton: Locator;
  readonly needsActionTab: Locator;
  readonly historyTab: Locator;
  readonly attentionItems: Locator;
  readonly taskCards: Locator;
  readonly historyRows: Locator;
  readonly emptyState: Locator;
  readonly loadingSpinner: Locator;

  constructor(page: Page) {
    super(page);

    this.reviewsToggle = page.locator('[data-testid="reviews-toggle"]');
    this.shell = page.locator('[data-testid="notifications-panel-shell"]');
    this.frame = page.locator('[data-testid="notifications-panel-frame"]');
    this.backdrop = page.locator('[data-testid="notifications-panel-backdrop"]');
    this.panel = page.locator('[data-testid="notifications-panel"]');
    this.closeButton = page.locator('[data-testid="notifications-panel-close"]');
    this.needsActionTab = page.getByRole('tab', { name: /Needs action/ });
    this.historyTab = page.getByRole('tab', { name: /History/ });
    this.attentionItems = page.locator('[data-testid^="attention-item-"]');
    this.taskCards = page.locator('[data-testid^="task-review-card-"]');
    this.historyRows = page.locator('[data-testid^="notification-history-row-"]');
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

  async closeByOutsideClick() {
    await this.backdrop.click({ position: { x: 8, y: 8 } });
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

  async getDrawerGeometry(): Promise<NotificationDrawerGeometry> {
    return await this.page.evaluate(() => {
      const rectFor = (selector: string) => {
        const element = document.querySelector<HTMLElement>(selector);
        if (!element) throw new Error(`Missing notification drawer element: ${selector}`);
        const rect = element.getBoundingClientRect();
        return {
          left: rect.left,
          right: rect.right,
          top: rect.top,
          bottom: rect.bottom,
          width: rect.width,
          height: rect.height,
        };
      };

      return {
        viewportWidth: window.innerWidth,
        documentScrollWidth: Math.max(
          document.documentElement.scrollWidth,
          document.body.scrollWidth,
        ),
        shell: rectFor('[data-testid="notifications-panel-shell"]'),
        frame: rectFor('[data-testid="notifications-panel-frame"]'),
        panel: rectFor('[data-testid="notifications-panel"]'),
      };
    });
  }

  async getHorizontalOverflow(): Promise<NotificationDrawerOverflow> {
    return await this.page.evaluate(() => {
      const shell = document.querySelector<HTMLElement>('[data-testid="notifications-panel-shell"]');
      const panel = document.querySelector<HTMLElement>('[data-testid="notifications-panel"]');
      if (!shell || !panel) {
        throw new Error("Notification drawer shell and panel must be mounted before checking overflow");
      }

      const shellRect = shell.getBoundingClientRect();
      const documentScrollWidth = Math.max(
        document.documentElement.scrollWidth,
        document.body.scrollWidth,
      );

      return {
        documentOverflowPixels: Math.max(0, documentScrollWidth - window.innerWidth),
        shellViewportOverflowPixels: Math.max(
          0,
          -shellRect.left,
          shellRect.right - window.innerWidth,
        ),
        panelScrollOverflowPixels: Math.max(0, panel.scrollWidth - panel.clientWidth),
      };
    });
  }

  async getContainedElementReport(): Promise<NotificationContainedElement[]> {
    return await this.page.evaluate(() => {
      const shell = document.querySelector<HTMLElement>('[data-testid="notifications-panel-shell"]');
      if (!shell) throw new Error("Notification drawer shell must be mounted before checking containment");

      const shellRect = shell.getBoundingClientRect();
      const selectors = [
        '[data-testid^="task-review-card-"]',
        '[data-testid^="attention-item-"]',
        '[data-testid^="notification-history-row-"]',
      ];

      return Array.from(document.querySelectorAll<HTMLElement>(selectors.join(",")))
        .filter((element) => {
          const style = window.getComputedStyle(element);
          const rect = element.getBoundingClientRect();
          return style.display !== "none"
            && style.visibility !== "hidden"
            && rect.width > 0
            && rect.height > 0;
        })
        .map((element) => {
          const rect = element.getBoundingClientRect();
          return {
            testId: element.getAttribute("data-testid") ?? element.tagName,
            leftOverflowPixels: Math.max(0, shellRect.left - rect.left),
            rightOverflowPixels: Math.max(0, rect.right - shellRect.right),
            width: rect.width,
          };
        });
    });
  }
}
