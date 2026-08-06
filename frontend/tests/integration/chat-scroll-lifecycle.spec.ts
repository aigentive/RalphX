import { expect, test } from "@playwright/test";

import {
  dismissProviderCliUpdateToasts,
  setupApp,
} from "../fixtures/setup.fixtures";
import { AgentsChatDiagnosticsPage } from "../pages/views/agents-chat-diagnostics.page";
import { AgentsChatPage } from "../pages/views/agents-chat.page";
import { AgentsChatScrollWritesPage } from "../pages/views/agents-chat-scroll-writes.page";
import { AgentsChatTurnPage } from "../pages/views/agents-chat-turn.page";

const PARKED_TOLERANCE_PX = 4;
const READER_SCROLL_UP_PX = 900;
// Chromium delivers less scroll than the requested wheel delta, so the
// precondition checks that the reader genuinely left the tail, not the delta.
const MIN_PARKED_DISTANCE_PX = 250;

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
    // The growth path is still wired end to end, it just arms Virtuoso's own
    // follow window now instead of writing scrollTop.
    expect(trace).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: "controller", event: "content-growth" }),
      expect.objectContaining({ source: "controller", event: "growth-follow-armed" }),
    ]));
  });

  test("keeps Virtuoso the only scroll writer and never moves the reader up the transcript", async ({ page }, testInfo) => {
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

    await writes.expectSingleWriterNoRewind();
    // Virtuoso's mid-settle corrections are only tolerable because they are
    // intermediates: the turn still has to end at the real bottom.
    await turn.expectAtTrueBottom();
  });

  /**
   * Parks a reader above the tail and returns an assertion bound to where they
   * stopped. The reader must never be dragged down; distance to the bottom is
   * only asserted while the turn is still adding content, because completing a
   * run drops the live streaming row and the browser legitimately clamps a
   * parked reader onto a transcript that became shorter than their position.
   */
  const parkReaderAboveTail = async (
    chat: AgentsChatPage,
  ): Promise<(step: string, expectAwayFromBottom?: boolean) => Promise<void>> => {
    await chat.scrollUp(READER_SCROLL_UP_PX);
    const { clientHeight, scrollHeight, scrollTop: parked } = await chat.geometry();
    expect(
      scrollHeight - clientHeight - parked,
      "wheel-up should park the reader well above the bottom",
    ).toBeGreaterThan(MIN_PARKED_DISTANCE_PX);

    return async (step, expectAwayFromBottom = true) => {
      const geometry = await chat.geometry();
      expect(geometry.scrollTop, `${step} moved the parked reader down`)
        .toBeLessThanOrEqual(parked + PARKED_TOLERANCE_PX);
      if (!expectAwayFromBottom) return;
      expect(
        geometry.scrollHeight - geometry.clientHeight - geometry.scrollTop,
        `${step} returned the reader to the bottom`,
      ).toBeGreaterThan(PARKED_TOLERANCE_PX);
    };
  };

  test("leaves a reader who scrolled away where they are while the turn streams", async ({ page }, testInfo) => {
    const executionId = `${testInfo.workerIndex}-${testInfo.repeatEachIndex}`;
    const conversationId = `reader-parked-${executionId}`;
    const chat = new AgentsChatPage(page);
    const turn = new AgentsChatTurnPage(page, chat, conversationId, `run-parked-${executionId}`);
    await chat.open();
    await chat.seedConversation(conversationId, false, 12);
    await turn.expectAtTrueBottom();
    const expectStillParked = await parkReaderAboveTail(chat);

    await turn.start();
    await expectStillParked("run start");
    for (let sequence = 1; sequence <= 4; sequence += 1) {
      await turn.stream(
        `Streaming block ${sequence}. ${"Measured virtualized content keeps growing. ".repeat(18)}`,
        sequence,
      );
      await expectStillParked(`streaming block ${sequence}`);
    }
  });

  // Known defect, deterministic at 12 of 12 runs: finishing a run yanks a parked
  // reader to the bottom. Nothing in the scroll layer does it. Instrumented
  // mount tracing shows `IntegratedChatPanel` unmounting and remounting twice
  // while `AgentsActiveConversationPanel` stays mounted and the panel key never
  // changes, so a fresh transcript mounts pinned at the bottom by design and
  // the reader's `free` state dies with the old instance. The teardown lives in
  // the Agents panel composition, above anything this suite owns.
  //
  // `test.fail()` keeps the expectation asserted rather than deleted: when the
  // remount is fixed this test reports "expected to fail but passed" and should
  // be folded back into the streaming case above.
  test.fail("leaves a reader who scrolled away where they are when the run completes", async ({ page }, testInfo) => {
    const executionId = `${testInfo.workerIndex}-${testInfo.repeatEachIndex}`;
    const conversationId = `reader-parked-complete-${executionId}`;
    const chat = new AgentsChatPage(page);
    const turn = new AgentsChatTurnPage(page, chat, conversationId, `run-parked-complete-${executionId}`);
    await chat.open();
    await chat.seedConversation(conversationId, false, 12);
    await turn.expectAtTrueBottom();
    const expectStillParked = await parkReaderAboveTail(chat);

    await turn.start();
    await turn.stream(
      `Streaming block 1. ${"Measured virtualized content keeps growing. ".repeat(18)}`,
      1,
    );
    await turn.complete();
    await expectStillParked("run completion", false);

    await turn.finalize("The complete persisted answer replaced the streaming tail.");
    await expectStillParked("finalized message", false);
  });
});
