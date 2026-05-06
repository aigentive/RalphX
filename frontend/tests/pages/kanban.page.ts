import { Page, Locator } from "@playwright/test";
import { BasePage } from "./base.page";

/**
 * Page object for the Kanban Board view.
 * Centralizes selectors and actions for kanban-related tests.
 */
export class KanbanPage extends BasePage {
  // Board layout
  readonly board: Locator;

  // Loading state
  readonly skeleton: Locator;
  readonly skeletonColumn: (index: number) => Locator;
  readonly skeletonCard: (colIndex: number, cardIndex: number) => Locator;

  // Error state
  readonly error: Locator;

  // Column selectors (by status id)
  readonly column: (status: string) => Locator;
  readonly dropZone: (status: string) => Locator;

  // Task card selectors
  readonly taskCard: (id: string) => Locator;
  readonly taskCards: Locator;
  readonly taskTitle: (id: string) => Locator;

  // Header elements
  readonly branding: Locator;
  readonly reviewsToggle: Locator;

  constructor(page: Page) {
    super(page);

    // Board layout
    this.board = page.locator('[data-testid="task-board"]');

    // Loading state
    this.skeleton = page.locator('[data-testid="task-board-skeleton"]');
    this.skeletonColumn = (index) =>
      page.locator(`[data-testid="skeleton-column-${index}"]`);
    this.skeletonCard = (colIndex, cardIndex) =>
      page.locator(`[data-testid="skeleton-card-${colIndex}-${cardIndex}"]`);

    // Error state
    this.error = page.locator('[data-testid="task-board-error"]');

    // Columns
    this.column = (status) => page.locator(`[data-testid="column-${status}"]`);
    this.dropZone = (status) =>
      page.locator(`[data-testid="drop-zone-${status}"]`);

    // Task cards
    this.taskCard = (id) => page.locator(`[data-testid="task-card-${id}"]`);
    this.taskCards = page.locator('[data-testid^="task-card-"]');
    this.taskTitle = (id) =>
      this.taskCard(id).locator('[data-testid="task-title"]');

    // Header elements
    this.branding = page.locator('[data-testid="left-nav-brand"]');
    this.reviewsToggle = page.locator('[data-testid="reviews-toggle"]');
  }

  /**
   * Wait for the kanban board to be fully loaded with task cards
   */
  async waitForBoard() {
    await this.waitForApp();
    await this.taskCards.first().waitFor({ timeout: 10000 });
  }

  /**
   * Get the count of visible task cards
   */
  async getTaskCount(): Promise<number> {
    return await this.taskCards.count();
  }

  /**
   * Drag a task card to a different column
   */
  async dragTaskToColumn(taskId: string, targetStatus: string) {
    await this.taskCard(taskId).dragTo(this.dropZone(targetStatus));
  }
}
