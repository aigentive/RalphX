import { test, expect } from "@playwright/test";
import { setupIdeationChatShell } from "../../../fixtures/chat.fixtures";

/**
 * Minimal test to verify AskUserQuestionModal can be triggered
 */

test("can trigger ask user question banner via event bus", async ({ page }) => {
  await setupIdeationChatShell(page);

  await page.evaluate(() => {
    const payload = {
      requestId: "req-event-bus-1",
      taskId: "test-123",
      sessionId: "session-mock-1",
      header: "Test Header",
      question: "Test question?",
      options: [
        { label: "Option 1", description: "First option" },
        { label: "Option 2", description: "Second option" },
      ],
      multiSelect: false,
    };

    window.__eventBus?.emit("agent:ask_user_question", payload);
  });

  const banner = page.getByTestId("question-input-banner");
  await expect(banner).toBeVisible({ timeout: 5000 });
  await expect(banner.getByText("Test Header")).toBeVisible();
  await expect(banner.getByText("Test question?")).toBeVisible();
});
