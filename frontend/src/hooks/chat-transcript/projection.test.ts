import { describe, expect, it } from "vitest";
import type { ChatMessageResponse } from "@/api/chat";
import { projectPersistedStreamingContentBlocks } from "./projection";

function streamingThinking(text: string | undefined): ChatMessageResponse {
  return {
    id: "message-1", sessionId: null, projectId: null, taskId: null, role: "assistant", content: "",
    metadata: null, parentMessageId: null, conversationId: "conversation-1", toolCalls: null,
    contentBlocks: [{ type: "thinking", ...(text !== undefined ? { text } : {}) }], sender: null,
    timelineStatus: "streaming", timelineKind: null, timelineSequence: 1, timelineBlockIndex: 3,
  };
}

describe("projectPersistedStreamingContentBlocks", () => {
  it("drops empty persisted thinking rows while preserving non-empty rows", () => {
    const blocks = projectPersistedStreamingContentBlocks([
      streamingThinking(undefined),
      streamingThinking("durable thought"),
      streamingThinking(""),
    ]);

    expect(blocks).toEqual([{
      type: "thinking", text: "durable thought", blockIndex: 3, seq: 1,
    }]);
  });
});
