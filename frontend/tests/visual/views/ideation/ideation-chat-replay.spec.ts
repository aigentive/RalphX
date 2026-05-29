import { expect, type Locator, test } from "@playwright/test";
import { setupIdeationChatScenario } from "../../../fixtures/chat.fixtures";

const finalQuestionText = "Preferred default for automatic PR creation?";
const readToolPathText = "src-tauri/src/application/chat_service/mod.rs";

async function scrollReplayPanelToBottom(panel: Locator) {
  await panel.evaluate((root) => {
    const scroller = root.querySelector("[data-virtuoso-scroller]") as HTMLElement | null;
    if (!scroller) {
      return;
    }

    const top = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
    scroller.scrollTo({ top, behavior: "auto" });
    scroller.scrollTop = top;
    scroller.dispatchEvent(new Event("scroll", { bubbles: true }));
  });
}

async function settleReplayPanelAtBottom(panel: Locator) {
  const scrollToBottom = panel.getByRole("button", { name: /scroll to bottom/i });

  await expect(async () => {
    if (await scrollToBottom.isVisible()) {
      await scrollToBottom.click();
    }

    await scrollReplayPanelToBottom(panel);
    await expect(panel.getByText(finalQuestionText)).toBeVisible({ timeout: 1000 });
    await expect(panel.getByText(readToolPathText)).toBeVisible({ timeout: 1000 });
    await expect(scrollToBottom).toBeHidden({ timeout: 1000 });
  }).toPass({ timeout: 10000 });
}

test.describe("Ideation Chat Replay", () => {
  // The seeded fixture uses absolute UTC timestamps; pin the browser timezone so
  // snapshots do not depend on the runner's local timezone.
  test.use({ timezoneId: "UTC" });

  test.beforeEach(async ({ page }) => {
    await setupIdeationChatScenario(page, "ideation_db_widget_mix");
  });

  test("renders DB-derived chat replay widgets in the ideation conversation panel", async ({ page }) => {
    const panel = page.locator('[data-testid="conversation-panel"]');

    await expect(panel).toBeVisible();
    await settleReplayPanelAtBottom(panel);
    await expect(page.getByTestId("chat-session-provider-badge")).toHaveText(/Claude/i);
    await expect(panel.getByText(finalQuestionText)).toBeVisible();
    await expect(panel.getByText(readToolPathText)).toBeVisible();
  });

  test("shows seeded conversation stats for the ideation replay", async ({ page }) => {
    const panel = page.locator('[data-testid="conversation-panel"]');

    await expect(panel).toBeVisible();
    await page.getByTestId("chat-session-stats-button").click();

    await expect(page.getByText("Conversation stats")).toBeVisible();
    await expect(page.getByText("4,821")).toBeVisible();
    await expect(page.getByText("713")).toBeVisible();
    await expect(page.getByText("$0.08")).toBeVisible();
    await expect(page.getByText("claude-sonnet-4-6", { exact: true })).toBeVisible();
  });

  test("matches ideation chat replay snapshot", async ({ page }) => {
    const panel = page.locator('[data-testid="conversation-panel"]');
    await expect(panel).toBeVisible();
    await settleReplayPanelAtBottom(panel);
    await expect(page.getByTestId("chat-session-provider-badge")).toHaveText(/Claude/i);
    await expect(panel.getByText(finalQuestionText)).toBeVisible();
    await expect(panel.getByText(readToolPathText)).toBeVisible();
    await page.waitForTimeout(150);
    const clip = await panel.evaluate((root) => {
      const rect = root.getBoundingClientRect();
      const x = Math.max(0, Math.floor(rect.x));
      const y = Math.max(0, Math.floor(rect.y));
      return {
        x,
        y,
        width: Math.ceil(rect.right - x),
        height: Math.ceil(rect.bottom - y),
      };
    });
    const screenshot = await page.screenshot({ animations: "disabled", clip });
    expect(screenshot).toMatchSnapshot("ideation-chat-replay.png", {
      maxDiffPixelRatio: 0.01,
    });
  });
});
