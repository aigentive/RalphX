import { expect, test } from "@playwright/test";

import {
  dismissProviderCliUpdateToasts,
  setupApp,
} from "../fixtures/setup.fixtures";
import { AgentsChatPage } from "../pages/views/agents-chat.page";
import { AgentsChatTurnPage } from "../pages/views/agents-chat-turn.page";

test.describe("existing conversation bottom follow", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await dismissProviderCliUpdateToasts(page);
    await setupApp(page);
  });

  test("stays at the true bottom through an optimistic send and complete agent turn", async ({ page }) => {
    const conversationId = "existing-conversation-turn";
    const chat = new AgentsChatPage(page);
    const turn = new AgentsChatTurnPage(page, chat, conversationId, "run-bottom-follow");
    await chat.open();
    await chat.seedConversation(conversationId, false, 12);

    const initial = await chat.geometry();
    expect(initial.scrollHeight).toBeGreaterThan(initial.clientHeight);
    await turn.returnToBottom();

    await turn.send("Follow this new turn all the way to the real bottom.");
    await turn.expectAtTrueBottom();

    await turn.start();
    await turn.expectAtTrueBottom();

    for (let sequence = 1; sequence <= 4; sequence += 1) {
      await turn.stream(
        `Streaming block ${sequence}. ${"Measured virtualized content keeps growing. ".repeat(18)}`,
        sequence,
      );
      await turn.expectAtTrueBottom();
    }

    await turn.complete();
    await turn.expectAtTrueBottom();

    await turn.finalize("The complete persisted answer replaced the streaming tail.");
    await turn.expectAtTrueBottom();
  });
});
