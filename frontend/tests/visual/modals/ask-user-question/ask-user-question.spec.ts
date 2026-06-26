import { test, expect, type Page } from "@playwright/test";
import { setupIdeationChatShell } from "../../../fixtures/chat.fixtures";
import {
  triggerAskUserQuestionBanner,
  createSingleSelectQuestion,
  createMultiSelectQuestion,
} from "../../../helpers/ask-user-question.helpers";

/**
 * Visual regression tests for the inline QuestionInputBanner component.
 */

async function blurQuestionInput(page: Page) {
  await page.evaluate(() => {
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur();
    }
  });
}

test.describe("QuestionInputBanner", () => {
  test.beforeEach(async ({ page }) => {
    await setupIdeationChatShell(page);
  });

  test.describe("single-select question", () => {
    test("renders banner with question and options", async ({ page }) => {
      const question = createSingleSelectQuestion();
      await triggerAskUserQuestionBanner(page, question);

      const banner = page.getByTestId("question-input-banner");
      await expect(banner).toBeVisible();

      await expect(
        banner.locator("span").filter({ hasText: question.header! }).first()
      ).toBeVisible();
      await expect(banner.getByText(question.question)).toBeVisible();
      await expect(banner.getByRole("button", { name: /JWT tokens/i })).toBeVisible();
      await expect(banner.getByRole("button", { name: /Session cookies/i })).toBeVisible();
      await expect(banner.getByRole("button", { name: /OAuth only/i })).toBeVisible();
    });

    test("allows selecting a single option", async ({ page }) => {
      const question = createSingleSelectQuestion();
      await triggerAskUserQuestionBanner(page, question);

      const banner = page.getByTestId("question-input-banner");
      await expect(banner).toBeVisible();

      await banner.getByRole("button", { name: /JWT tokens/i }).click();

      const input = page.getByPlaceholder("Type 1-3 or a custom response...");
      await expect(input).toHaveValue("1");
    });

    test("matches snapshot - default state", async ({ page }) => {
      const question = createSingleSelectQuestion();
      await triggerAskUserQuestionBanner(page, question);

      const banner = page.getByTestId("question-input-banner");
      await expect(banner).toBeVisible();

      await blurQuestionInput(page);
      await page.waitForTimeout(200);

      await expect(banner).toHaveScreenshot("ask-user-question-single-select.png", {
        maxDiffPixelRatio: 0.01,
      });
    });

    test("matches snapshot - with selection", async ({ page }) => {
      const question = createSingleSelectQuestion();
      await triggerAskUserQuestionBanner(page, question);

      const banner = page.getByTestId("question-input-banner");
      await expect(banner).toBeVisible();
      await banner.getByRole("button", { name: /JWT tokens/i }).click();

      await page.waitForTimeout(200);

      await expect(banner).toHaveScreenshot("ask-user-question-single-select-selected.png", {
        maxDiffPixelRatio: 0.01,
      });
    });
  });

  test.describe("multi-select question", () => {
    test("renders banner with multi-select chips", async ({ page }) => {
      const question = createMultiSelectQuestion();
      await triggerAskUserQuestionBanner(page, question);

      const banner = page.getByTestId("question-input-banner");
      await expect(banner).toBeVisible();

      await expect(
        banner.locator("span").filter({ hasText: question.header! }).first()
      ).toBeVisible();
      await expect(banner.getByText(question.question)).toBeVisible();
      await expect(banner.getByRole("button", { name: /Dark mode/i })).toBeVisible();
      await expect(banner.getByRole("button", { name: /Analytics/i })).toBeVisible();
      await expect(banner.getByRole("button", { name: /Notifications/i })).toBeVisible();
    });

    test("allows selecting multiple options", async ({ page }) => {
      const question = createMultiSelectQuestion();
      await triggerAskUserQuestionBanner(page, question);

      const banner = page.getByTestId("question-input-banner");
      await expect(banner).toBeVisible();
      await banner.getByRole("button", { name: /Dark mode/i }).click();
      await banner.getByRole("button", { name: /Analytics/i }).click();

      const input = page.getByPlaceholder("Type 1-3 or a custom response...");
      await expect(input).toHaveValue("1, 2");
    });

    test("matches snapshot - default state", async ({ page }) => {
      const question = createMultiSelectQuestion();
      await triggerAskUserQuestionBanner(page, question);

      const banner = page.getByTestId("question-input-banner");
      await expect(banner).toBeVisible();
      await blurQuestionInput(page);
      await page.waitForTimeout(200);

      await expect(banner).toHaveScreenshot("ask-user-question-multi-select.png", {
        maxDiffPixelRatio: 0.01,
      });
    });

    test("matches snapshot - multiple selections", async ({ page }) => {
      const question = createMultiSelectQuestion();
      await triggerAskUserQuestionBanner(page, question);

      const banner = page.getByTestId("question-input-banner");
      await expect(banner).toBeVisible();
      await banner.getByRole("button", { name: /Dark mode/i }).click();
      await banner.getByRole("button", { name: /Notifications/i }).click();

      await page.waitForTimeout(200);

      await expect(banner).toHaveScreenshot("ask-user-question-multi-select-selected.png", {
        maxDiffPixelRatio: 0.01,
      });
    });
  });

  test.describe("banner controls", () => {
    test("dismiss button hides the banner", async ({ page }) => {
      const question = createSingleSelectQuestion();
      await triggerAskUserQuestionBanner(page, question);

      const banner = page.getByTestId("question-input-banner");
      await expect(banner).toBeVisible();
      await page.getByRole("button", { name: "Dismiss question" }).click();

      await expect(banner).toBeHidden();
    });
  });
});
