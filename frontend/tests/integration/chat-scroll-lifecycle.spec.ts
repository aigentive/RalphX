import { expect, test } from "@playwright/test";

import {
  dismissProviderCliUpdateToasts,
  setupApp,
} from "../fixtures/setup.fixtures";
import { expectNoPinChurn } from "../helpers/chat-scroll.helpers";
import { AgentsChatBistablePage } from "../pages/views/agents-chat-bistable.page";
import { AgentsChatDiagnosticsPage } from "../pages/views/agents-chat-diagnostics.page";
import { AgentsChatPage } from "../pages/views/agents-chat.page";
import { AgentsChatScrollWritesPage } from "../pages/views/agents-chat-scroll-writes.page";
import { AgentsChatStillnessPage } from "../pages/views/agents-chat-stillness.page";
import { AgentsChatTurnPage } from "../pages/views/agents-chat-turn.page";

const BISTABLE_TAIL_GAP_PX = 20;
const STILLNESS_FRAMES = 40;
const SETTLE_AFTER_HYDRATION_MS = 500;

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

    // seedConversation only waits for the composer, so the transcript can still
    // be hydrating here; poll rather than sampling the geometry once.
    await expect.poll(
      async () => {
        const { clientHeight, scrollHeight } = await chat.geometry();
        return scrollHeight - clientHeight;
      },
      { message: "seeded transcript should overflow the viewport" },
    ).toBeGreaterThan(0);
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

    // The recorder keeps a bounded ring and one turn can overflow it, so the
    // composer-driven layout events are asserted while they are still held.
    const composerTrace = await diagnostics.read();
    expect(composerTrace).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: "manual", event: "mark:agent-turn-start" }),
      expect.objectContaining({ source: "layout", event: "chrome-inset-write" }),
      expect.objectContaining({ source: "layout", event: "bottom-spacer-resize" }),
    ]));

    await turn.complete();
    await turn.expectAtTrueBottom();

    await turn.finalize("The complete persisted answer replaced the streaming tail.");
    await turn.expectAtTrueBottom();

    const trace = await diagnostics.stop();
    expect(trace).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: "controller", event: "content-growth" }),
      expect.objectContaining({ source: "controller", event: "pin" }),
    ]));
  });

  test("keeps the controller the only scroll writer and never pins back up the transcript", async ({ page }, testInfo) => {
    const executionId = `${testInfo.workerIndex}-${testInfo.repeatEachIndex}`;
    const conversationId = `single-scroll-writer-${executionId}`;
    const chat = new AgentsChatPage(page);
    const writes = new AgentsChatScrollWritesPage(page);
    const turn = new AgentsChatTurnPage(page, chat, conversationId, `run-writer-${executionId}`);
    await chat.open();
    await chat.seedConversation(conversationId, false, 12);
    await turn.expectAtTrueBottom();
    await writes.record();

    await turn.send("Follow this new turn all the way to the real bottom.");
    await turn.start();
    for (let sequence = 1; sequence <= 4; sequence += 1) {
      await turn.stream(
        `Streaming block ${sequence}. ${"Measured virtualized content keeps growing. ".repeat(18)}`,
        sequence,
      );
    }
    await turn.complete();

    const recorded = await writes.writes();
    expect(recorded.length).toBeGreaterThan(0);
    // Item alignment used to leak into coalesced pins, which made Virtuoso a
    // second asynchronous writer landing at the last item's end - above the
    // footer spacer - and dragged the reader 200-600px back up.
    expect(recorded.filter(({ fromController }) => !fromController)).toEqual([]);
    // A bottom pin computed from a transiently under-reported extent used to
    // scroll the reader upward. Genuine shrink is the browser's clamp, not ours.
    expect(recorded.filter(({ requested, before }) => requested < before)).toEqual([]);
  });

  // KNOWN OPEN DEFECT — expected to fail. With the controller now the single
  // scroll writer, the residual churn is driven entirely by the browser: the
  // virtualizer republishes a stale, several-hundred-pixel-short extent for a
  // frame and scrollTop is clamped up before we can read anything. Measured in
  // Chromium, 1-4 such clamps per streaming turn survive on an unmodified tree.
  test("stops the bottom pin loop when the tail measurement collapses on every write", async ({ page }, testInfo) => {
    test.fail();
    const executionId = `${testInfo.workerIndex}-${testInfo.repeatEachIndex}`;
    const conversationId = `bistable-tail-${executionId}`;
    const chat = new AgentsChatPage(page);
    const bistable = new AgentsChatBistablePage(page);
    const diagnostics = new AgentsChatDiagnosticsPage(page);
    const turn = new AgentsChatTurnPage(page, chat, conversationId, `run-bistable-${executionId}`);
    await chat.open();
    await chat.seedConversation(conversationId, false, 12);
    await turn.expectAtTrueBottom();

    await bistable.install(BISTABLE_TAIL_GAP_PX);
    await diagnostics.start();
    await diagnostics.mark("bistable-tail-installed");

    await turn.start();
    await turn.stream(
      `Streaming into a collapsing tail. ${"Measured virtualized content keeps growing. ".repeat(18)}`,
      1,
    );
    // Give the write/measure loop a full second of real frames to run away.
    await page.waitForTimeout(1_000);
    await turn.complete();

    const trace = await diagnostics.stop();
    expectNoPinChurn(trace);
    expect(await bistable.distanceToBottom()).toBeLessThanOrEqual(BISTABLE_TAIL_GAP_PX + 2);
  });

  // KNOWN OPEN DEFECT — expected to fail. This is the up-and-down jitter itself,
  // reproduced without any agent activity: a hydrated transcript sitting at the
  // bottom moves on ~39 of 40 consecutive frames, up to ~1150px per frame.
  //
  // Measured cause (Chromium, per-frame geometry + write attribution):
  //   pin write -> Virtuoso recomputes its render window -> it commits the new
  //   item set and the matching padding in DIFFERENT frames, so for one frame
  //   the scroller reports a several-hundred-pixel-short extent -> the browser
  //   clamps scrollTop up -> the controller reads an unmet bottom intent and
  //   pins again. Freeing the controller stops the oscillation dead (0 of 40
  //   frames move), so the writes sustain it; the geometry itself is stable.
  //
  // Two fixes were measured and rejected rather than shipped: gating pins on a
  // true bottom that held still for 2 frames cut the writes from 24 to 8 per
  // 300ms but left the reader parked short of the bottom (it broke
  // expectAtTrueBottom); persisting the last-row height across remounts changed
  // nothing measurable. The remaining candidate is an extent floor that keeps
  // the scroller from reporting a shorter extent than it just published, so the
  // torn frame can never clamp the position away.
  test("holds a settled transcript still while it sits at the bottom", async ({ page }, testInfo) => {
    test.fail();
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
});
