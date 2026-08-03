import { describe, expect, it } from "vitest";

import { collectPersistedThinkingRun } from "./persisted-thinking-group";

function thinkingRow(sequence: number, text = `Thought ${sequence}`) {
  return {
    role: "assistant",
    parentMessageId: "message-1",
    providerHarness: "codex",
    providerSessionId: "session-1",
    timelineSequence: sequence,
    contentBlocks: [{ type: "thinking", text }],
  };
}

describe("collectPersistedThinkingRun", () => {
  it("collects adjacent sequence-continuous thinking rows", () => {
    const rows = [thinkingRow(4), thinkingRow(5)];
    expect(collectPersistedThinkingRun(rows, 0)).toEqual(rows);
  });

  it("leaves a single persisted thinking row on the existing MessageItem path", () => {
    expect(collectPersistedThinkingRun([thinkingRow(4)], 0)).toBeNull();
  });

  it("breaks at sequence gaps", () => {
    expect(collectPersistedThinkingRun([thinkingRow(4), thinkingRow(6)], 0)).toBeNull();
  });

  it("breaks across provider surfaces", () => {
    const otherSurface = { ...thinkingRow(5), providerSessionId: "session-2" };
    expect(collectPersistedThinkingRun([thinkingRow(4), otherSurface], 0)).toBeNull();
  });

  it("breaks across parent messages", () => {
    const otherParent = { ...thinkingRow(5), parentMessageId: "message-2" };
    expect(collectPersistedThinkingRun([thinkingRow(4), otherParent], 0)).toBeNull();
  });

  it("does not bridge or count whitespace-only thinking rows", () => {
    const rows = [thinkingRow(4), thinkingRow(5, " "), thinkingRow(6)];
    expect(collectPersistedThinkingRun(rows, 0)).toBeNull();
  });
});
