import { expect, test, type Page } from "@playwright/test";

const SUMMARY =
  "Agent called 5 tools, created 1 file, edited 2 files, and delegated 2 agents.";

async function openChatActivityPage(
  page: Page,
  options: { theme: "dark" | "light"; width: number },
) {
  await page.setViewportSize({ width: options.width, height: 900 });
  await page.goto(`/?test=chat-activity&theme=${options.theme}`);
  await page.waitForLoadState("networkidle");
  await expect(page.getByTestId("chat-activity-visual-test-page")).toBeVisible();
}

test.describe("shared chat activity presentation", () => {
  test("renders provider-neutral collapsed summaries and promoted delegation cards", async ({ page }) => {
    await openChatActivityPage(page, { theme: "dark", width: 1180 });

    await expect(page.getByText(SUMMARY)).toHaveCount(2);
    await expect(page.getByTestId("task-tool-call-card")).toHaveCount(4);
    await expect(page.getByTestId("streaming-tool-indicator")).toHaveCount(0);
    await expect(page.getByTestId("thinking-group-toggle")).toHaveCount(2);
    await expect(page).toHaveScreenshot("chat-activity-provider-parity-dark.png", {
      animations: "disabled",
      fullPage: true,
    });
  });

  test("renders a collapsed then expanded thinking viewer", async ({ page }) => {
    await openChatActivityPage(page, { theme: "dark", width: 760 });
    const fixture = page.getByTestId("chat-activity-claude");
    const toggle = fixture.getByTestId("thinking-group-toggle");
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await expect(toggle).toHaveText("Agent thought for 2s");
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(fixture.getByTestId("thinking-scroll-body")).toBeVisible();
    await expect(page).toHaveScreenshot("chat-thinking-expanded-dark-narrow.png", {
      animations: "disabled",
      fullPage: true,
    });
  });

  test("keeps the activity sentence stable when details expand", async ({ page }) => {
    await openChatActivityPage(page, { theme: "light", width: 760 });

    const claudeFixture = page.getByTestId("chat-activity-claude");
    const toggle = claudeFixture.getByTestId("tool-call-group-toggle");
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(claudeFixture.getByText(SUMMARY)).toBeVisible();
    await expect(claudeFixture.getByTestId("diff-tool-call-view")).toHaveCount(2);

    await expect(page).toHaveScreenshot("chat-activity-expanded-light-narrow.png", {
      animations: "disabled",
      fullPage: true,
    });
  });

  test("shows provider-correct processed and cached-input usage", async ({ page }) => {
    await openChatActivityPage(page, { theme: "dark", width: 1180 });

    await page.getByRole("button", { name: "Conversation stats" }).click();
    await expect(page.getByText("9.1m")).toHaveCount(2);
    await expect(page.getByText("Cached input", { exact: true })).toBeVisible();
    await expect(page.getByText(/already included in Codex Input/)).toBeVisible();
    await expect(page.getByText("1 provider-fallback sample(s)")).toBeVisible();

    await expect(page).toHaveScreenshot("chat-usage-provider-correct-dark.png", {
      animations: "disabled",
      fullPage: true,
    });
  });
});
