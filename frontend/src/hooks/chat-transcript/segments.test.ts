import { describe, expect, it } from "vitest";

import type { StreamingContentBlock } from "@/types/streaming-task";
import {
  applyTranscriptInput,
  createLiveTranscriptState,
  renderTranscriptBlocks,
} from "./segments";

/**
 * The live transcript has one owner keyed by `(runId, blockIndex)`. These tests
 * assert on the owner's output rather than the DOM, so a regression points at
 * the reducer instead of at whichever component happened to render it.
 */
describe("live transcript owner", () => {
  const RUN = "run-active";

  function toolBlock(id: string): StreamingContentBlock {
    return { type: "tool_use", toolCall: { id, name: "Grep", arguments: {} } };
  }

  /** persisted `text(0) / tool(1) / text(2)` — a turn with a mid-stream tool call. */
  const interleavedPersisted: StreamingContentBlock[] = [
    { type: "text", text: "Before the tool. ", blockIndex: 0 },
    toolBlock("toolu_grep"),
    { type: "text", text: "After the tool.", blockIndex: 2 },
  ];

  function seeded(blocks: readonly StreamingContentBlock[] = interleavedPersisted) {
    return applyTranscriptInput(createLiveTranscriptState(RUN), {
      kind: "persisted", runId: RUN, blocks,
    });
  }

  it("renders one text block when a live chunk lands on an interleaved persisted segment", () => {
    // The production duplicate: block 2 is the third content block but only the
    // second *text* block, so a text-ordinal identity keyed it as 1 and the live
    // copy rendered alongside the durable row.
    const state = applyTranscriptInput(seeded(), {
      kind: "chunk", runId: RUN, blockIndex: 2, text: "After the tool.", appendToPrevious: false,
    });

    const texts = renderTranscriptBlocks(state).filter((block) => block.type === "text");
    expect(texts).toHaveLength(0);
  });

  it("rejects stale thinking and updates its indexed thinking slot in place", () => {
    let state = createLiveTranscriptState(RUN);
    state = applyTranscriptInput(state, {
      kind: "thinking", runId: "run-stale", blockIndex: 4, text: "ignored", appendToPrevious: false,
    });
    expect(state.slots).toEqual([]);

    state = applyTranscriptInput(state, {
      kind: "thinking", runId: RUN, blockIndex: 4, text: "first", appendToPrevious: false,
    });
    state = applyTranscriptInput(state, {
      kind: "thinking", runId: RUN, blockIndex: 4, text: " second", appendToPrevious: true,
    });
    expect(renderTranscriptBlocks(state)).toMatchObject([
      { type: "thinking", text: "first second", blockIndex: 4 },
    ]);
  });

  it("emits only the tail a live chunk adds beyond the persisted segment", () => {
    const state = applyTranscriptInput(seeded(), {
      kind: "chunk",
      runId: RUN,
      blockIndex: 2,
      text: "After the tool. Still going.",
      appendToPrevious: false,
    });

    expect(renderTranscriptBlocks(state).filter((block) => block.type === "text")).toEqual([
      { type: "text", text: " Still going.", blockIndex: 2 },
    ]);
  });

  it("converges persisted, segment-poll, and chunk views of one segment into a single block", () => {
    let state = seeded();
    state = applyTranscriptInput(state, {
      kind: "segments", runId: RUN, segments: ["Before the tool. ", "", "After the tool. Tail"],
    });
    state = applyTranscriptInput(state, {
      kind: "chunk", runId: RUN, blockIndex: 2, text: " more", appendToPrevious: true,
    });

    const texts = renderTranscriptBlocks(state).filter((block) => block.type === "text");
    expect(texts).toHaveLength(1);
    expect(texts[0]).toMatchObject({ blockIndex: 2, text: " Tail more" });
  });

  it("ignores input from a run that is not the active one", () => {
    const state = seeded();
    const stale = applyTranscriptInput(state, {
      kind: "chunk", runId: "run-previous", blockIndex: 2, text: "leaked", appendToPrevious: true,
    });

    expect(stale).toBe(state);
    expect(renderTranscriptBlocks(stale)).toEqual(renderTranscriptBlocks(state));
  });

  it("adopts the first run it sees and rejects every later one", () => {
    // A run-less state is unbound, so the first identified input claims it.
    // After that the gate is closed: a newer run does not get to overwrite the
    // current one here — callers build fresh state for a new run.
    const state = applyTranscriptInput(createLiveTranscriptState(), {
      kind: "chunk", runId: RUN, blockIndex: 0, text: "Owned by the first run.", appendToPrevious: false,
    });
    expect(state.runId).toBe(RUN);

    const intruded = applyTranscriptInput(state, {
      kind: "chunk", runId: "run-next", blockIndex: 0, text: " and the next", appendToPrevious: true,
    });

    expect(intruded).toBe(state);
    expect(renderTranscriptBlocks(intruded)).toEqual([
      { type: "text", text: "Owned by the first run.", blockIndex: 0 },
    ]);
  });

  it("renders live text that carries no block index", () => {
    const state = applyTranscriptInput(createLiveTranscriptState(RUN), {
      kind: "chunk", runId: RUN, text: "Legacy harness text.", appendToPrevious: true,
    });

    expect(renderTranscriptBlocks(state)).toEqual([
      { type: "text", text: "Legacy harness text." },
    ]);
  });

  it("keeps divergent same-segment text instead of hiding it", () => {
    const state = applyTranscriptInput(seeded(), {
      kind: "chunk",
      runId: RUN,
      blockIndex: 2,
      text: "Completely different wording.",
      appendToPrevious: false,
    });

    expect(renderTranscriptBlocks(state).filter((block) => block.type === "text")).toEqual([
      { type: "text", text: "Completely different wording.", blockIndex: 2 },
    ]);
  });

  it("releases a persisted tool block while keeping an unpersisted one", () => {
    const state = applyTranscriptInput(seeded(), {
      kind: "tools", runId: RUN, blocks: [toolBlock("toolu_grep"), toolBlock("toolu_read")],
    });

    expect(renderTranscriptBlocks(state)).toEqual([
      expect.objectContaining({ type: "tool_use" }),
    ]);
    expect(renderTranscriptBlocks(state)[0]).toMatchObject({
      toolCall: expect.objectContaining({ id: "toolu_read" }),
    });
  });

  it("preserves live arrival order rather than sorting by block index", () => {
    // Tool/task interleaving is owned by seq/receivedAt. Ordering text by
    // blockIndex would float every text row above the tools it came after.
    let state = createLiveTranscriptState(RUN);
    state = applyTranscriptInput(state, {
      kind: "chunk", runId: RUN, blockIndex: 2, text: "second", appendToPrevious: false,
    });
    state = applyTranscriptInput(state, { kind: "tools", runId: RUN, blocks: [toolBlock("t1")] });
    state = applyTranscriptInput(state, {
      kind: "chunk", runId: RUN, blockIndex: 0, text: "first", appendToPrevious: false,
    });

    expect(renderTranscriptBlocks(state).map((block) =>
      block.type === "text" ? block.text : block.type,
    )).toEqual(["second", "tool_use", "first"]);
  });
});
