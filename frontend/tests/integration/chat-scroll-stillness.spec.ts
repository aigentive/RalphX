import { expect, test } from "@playwright/test";

import {
  dismissProviderCliUpdateToasts,
  setupApp,
} from "../fixtures/setup.fixtures";
import { AgentsChatBistablePage } from "../pages/views/agents-chat-bistable.page";
import { AgentsChatPage } from "../pages/views/agents-chat.page";
import { AgentsChatScrollWritesPage } from "../pages/views/agents-chat-scroll-writes.page";
import { AgentsChatStillnessPage } from "../pages/views/agents-chat-stillness.page";
import { AgentsChatTurnPage } from "../pages/views/agents-chat-turn.page";

const BISTABLE_TAIL_GAP_PX = 20;
const STILLNESS_FRAMES = 40;
const SETTLE_AFTER_HYDRATION_MS = 500;

test.describe("chat transcript stillness", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await dismissProviderCliUpdateToasts(page);
    await setupApp(page);
  });

  // The jitter itself, reproduced without any agent activity. It was sustained
  // by the controller's own writes: a pin wrote the bottom, Virtuoso committed
  // its new item set and the matching padding in different frames, the scroller
  // reported a several-hundred-pixel-short extent for one frame, the browser
  // clamped scrollTop up, and the controller read that as an unmet bottom
  // intent and pinned again. With no controller write left to start the cycle,
  // a settled transcript has nothing to oscillate against.
  test("holds a settled transcript still while it sits at the bottom", async ({ page }, testInfo) => {
    const executionId = `${testInfo.workerIndex}-${testInfo.repeatEachIndex}`;
    const conversationId = `settled-stillness-${executionId}`;
    const chat = new AgentsChatPage(page);
    const stillness = new AgentsChatStillnessPage(page);
    const turn = new AgentsChatTurnPage(page, chat, conversationId, `run-stillness-${executionId}`);
    await chat.open();
    await chat.seedConversation(conversationId, false, 12);
    await turn.expectAtTrueBottom();
    // Let hydration finish; nothing touches the transcript after this point, so
    // every frame that moves rendered content is churn with no content behind it.
    await page.waitForTimeout(SETTLE_AFTER_HYDRATION_MS);

    const movement = await stillness.measure(STILLNESS_FRAMES);
    expect(movement.frames).toBeGreaterThanOrEqual(STILLNESS_FRAMES - 2);
    expect(
      movement,
      `transcript moved on ${movement.movingFrames} of ${movement.frames} settled frames`
      + ` (largest jump ${movement.maxJumpPx}px)`,
    ).toMatchObject({ movingFrames: 0, maxJumpPx: 0 });
  });

  // The residual case the controller could never win: the trailing measurement
  // collapses on the write itself, so every restored measurement looked like
  // forward progress and the browser clamped it straight back. Virtuoso's own
  // follow window absorbs the flip instead of racing it.
  test("stops the tail churn when the tail measurement collapses on every write", async ({ page }, testInfo) => {
    const executionId = `${testInfo.workerIndex}-${testInfo.repeatEachIndex}`;
    const conversationId = `bistable-tail-${executionId}`;
    const chat = new AgentsChatPage(page);
    const bistable = new AgentsChatBistablePage(page);
    const writes = new AgentsChatScrollWritesPage(page);
    const turn = new AgentsChatTurnPage(page, chat, conversationId, `run-bistable-${executionId}`);
    await chat.open();
    await chat.seedConversation(conversationId, false, 12);
    await turn.expectAtTrueBottom();

    await bistable.install(BISTABLE_TAIL_GAP_PX);
    await writes.record();

    await turn.start();
    await turn.stream(
      `Streaming into a collapsing tail. ${"Measured virtualized content keeps growing. ".repeat(18)}`,
      1,
    );
    // Give the measure loop a full second of real frames to run away.
    await page.waitForTimeout(1_000);
    await turn.complete();

    const recorded = await writes.writes();
    expect(recorded.filter(({ fromController }) => fromController)).toEqual([]);
    expect(recorded.filter(({ requested, before }) => requested < before)).toEqual([]);
    expect(await bistable.distanceToBottom()).toBeLessThanOrEqual(BISTABLE_TAIL_GAP_PX + 2);
  });
});
