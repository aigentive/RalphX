import { expect, test } from "@playwright/test";

import {
  dismissProviderCliUpdateToasts,
  setupApp,
} from "../../../fixtures/setup.fixtures";
import { AgentsChatPage } from "../../../pages/views/agents-chat.page";

test.describe("pinned chat composer inset", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await dismissProviderCliUpdateToasts(page);
    await setupApp(page);
  });

  test("composer growth keeps the viewport fixed and the last message visible", async ({ page }) => {
    const chat = new AgentsChatPage(page);
    await chat.open();
    await chat.seedConversation("composer-inset-growth");

    const before = await chat.geometry();
    await chat.growComposer();
    await expect.poll(async () => (await chat.geometry()).clientHeight)
      .toBe(before.clientHeight);

    const [lastMessage, chrome] = await Promise.all([
      chat.lastRenderedRow.boundingBox(),
      chat.chrome.boundingBox(),
    ]);
    expect(lastMessage).not.toBeNull();
    expect(chrome).not.toBeNull();
    expect(lastMessage!.y + lastMessage!.height).toBeLessThanOrEqual(chrome!.y + 1);
  });

  test("composer shrink stays pinned while scrolled-up growth preserves free state", async ({ page }) => {
    const chat = new AgentsChatPage(page);
    await chat.open();
    await chat.seedConversation("composer-inset-free");
    await chat.growComposer();
    await chat.clearComposer();

    await expect.poll(async () => {
      const geometry = await chat.geometry();
      return geometry.scrollHeight - geometry.clientHeight - geometry.scrollTop;
    }).toBeLessThanOrEqual(1);

    await chat.scroller.evaluate((element) => element.scrollTo({ top: 0 }));
    const freeBefore = await chat.geometry();
    await chat.growComposer();
    await expect.poll(async () => (await chat.geometry()).scrollTop)
      .toBe(freeBefore.scrollTop);
    await expect(chat.scrollToBottomButton).toBeVisible();
    await expect(chat.chrome).toBeInViewport();
  });

  test("empty conversations keep the composer flush with the panel bottom", async ({ page }) => {
    const chat = new AgentsChatPage(page);
    await chat.open();
    await chat.seedConversation("composer-inset-empty", true);

    const [panel, chrome] = await Promise.all([
      chat.panel.boundingBox(),
      chat.chrome.boundingBox(),
    ]);
    expect(panel).not.toBeNull();
    expect(chrome).not.toBeNull();
    expect(Math.abs(panel!.y + panel!.height - chrome!.y - chrome!.height))
      .toBeLessThanOrEqual(1);
    await expect(chat.messages).toBeVisible();
    expect(await chat.messages.evaluate(
      (element) => element.scrollHeight <= element.clientHeight,
    )).toBe(true);
    await expect(page).toHaveScreenshot("chat-composer-inset-empty.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });
});
