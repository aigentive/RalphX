import { expect, test } from "@playwright/test";

import {
  dismissProviderCliUpdateToasts,
  setupApp,
} from "../fixtures/setup.fixtures";
import { AgentsChatDiagnosticsPage } from "../pages/views/agents-chat-diagnostics.page";
import { AgentsChatPage } from "../pages/views/agents-chat.page";
import { AgentsChatTurnPage } from "../pages/views/agents-chat-turn.page";

test.describe("existing conversation bottom follow", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await dismissProviderCliUpdateToasts(page);
    await setupApp(page);
  });

  test("stays at the true bottom through an optimistic send and complete agent turn", async ({ page }, testInfo) => {
    const executionId = `${testInfo.workerIndex}-${testInfo.repeatEachIndex}`;
    const conversationId = `existing-conversation-turn-${executionId}`;
    const chat = new AgentsChatPage(page);
    const diagnostics = new AgentsChatDiagnosticsPage(page);
    const turn = new AgentsChatTurnPage(page, chat, conversationId, `run-bottom-follow-${executionId}`);
    await chat.open();
    await chat.seedConversation(conversationId, false, 12);

    const initial = await chat.geometry();
    expect(initial.scrollHeight).toBeGreaterThan(initial.clientHeight);
    await turn.expectAtTrueBottom();
    await diagnostics.start();
    await diagnostics.mark("agent-turn-start");

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
      if (sequence === 2) {
        await chat.growComposer();
        await turn.expectAtTrueBottom();
      } else if (sequence === 3) {
        await chat.clearComposer();
        await turn.expectAtTrueBottom();
      }
    }

    await turn.complete();
    await turn.expectAtTrueBottom();

    await turn.finalize("The complete persisted answer replaced the streaming tail.");
    await turn.expectAtTrueBottom();

    const trace = await diagnostics.stop();
    expect(trace).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: "manual", event: "mark:agent-turn-start" }),
      expect.objectContaining({ source: "controller", event: "content-growth" }),
      expect.objectContaining({ source: "controller", event: "pin" }),
      expect.objectContaining({ source: "layout", event: "chrome-inset-write" }),
      expect.objectContaining({ source: "layout", event: "bottom-spacer-resize" }),
    ]));
  });
});
