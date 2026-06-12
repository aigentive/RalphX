import { describe, expect, it } from "vitest";

import type { ChatMessageResponse } from "@/api/chat";
import type { ChatConversation } from "@/types/chat-conversation";
import { toProjectAgentConversation } from "./agentConversations";
import { resolveAttachedIdeationSessionId } from "./attachedIdeationSession";

const conversation = toProjectAgentConversation({
  id: "conversation-1",
  contextType: "project",
  contextId: "project-1",
  claudeSessionId: null,
  providerSessionId: null,
  providerHarness: null,
  upstreamProvider: null,
  providerProfile: null,
  title: "Project agent",
  messageCount: 1,
  lastMessageAt: null,
  createdAt: "2026-04-22T10:00:00Z",
  updatedAt: "2026-04-22T10:00:00Z",
  archivedAt: null,
} satisfies ChatConversation);

function messageWithToolCall(
  toolCall: unknown,
  name = "mcp__ralphx__v1_send_ideation_message",
): ChatMessageResponse {
  return {
    id: "message-1",
    conversationId: "conversation-1",
    role: "assistant",
    content: "",
    contentBlocks: [
      {
        type: "tool_use",
        id: "tool-1",
        name,
        arguments: {},
        result: toolCall,
      },
    ],
    toolCalls: [],
    attachments: [],
    metadata: null,
    createdAt: "2026-04-22T10:01:00Z",
  } as ChatMessageResponse;
}

describe("resolveAttachedIdeationSessionId", () => {
  it("extracts reused ideation sessions from v1_send_ideation_message results", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithToolCall({ session_id: "session-reused" }),
    ]);

    expect(result).toBe("session-reused");
  });

  it("extracts session ids from encoded MCP text payloads", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithToolCall({
        content: [
          {
            text: JSON.stringify({
              structured_content: { sessionId: "session-from-text" },
            }),
          },
        ],
      }),
    ]);

    expect(result).toBe("session-from-text");
  });

  it("extracts planning sessions from plan artifact tool results", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithToolCall(
        { session_id: "planning-session-1", artifact_id: "artifact-1" },
        "mcp__ralphx__create_plan_artifact",
      ),
    ]);

    expect(result).toBe("planning-session-1");
  });

  it("falls back to the linked workspace session when no transcript tool result is available", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [], "session-linked");

    expect(result).toBe("session-linked");
  });
});
