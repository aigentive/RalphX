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

function messageWithMetadata(
  metadata: string | Record<string, unknown>,
): ChatMessageResponse {
  return {
    id: "message-with-metadata",
    conversationId: "conversation-1",
    role: "user",
    content: "Verify the referenced plan",
    contentBlocks: [],
    toolCalls: [],
    attachments: [],
    metadata:
      typeof metadata === "string" ? metadata : JSON.stringify(metadata),
    createdAt: "2026-04-22T10:00:30Z",
  } as ChatMessageResponse;
}

describe("resolveAttachedIdeationSessionId", () => {
  it("extracts attached plan sessions from user composer artifact references", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithMetadata({
        composer_artifact_references: [
          {
            artifactId: "plan-artifact-1",
            kind: "plan",
            sessionId: "referenced-session-1",
            status: "approved",
            title: "Referenced plan",
            version: 2,
          },
        ],
      }),
    ]);

    expect(result).toBe("referenced-session-1");
  });

  it("uses the latest compatible composer artifact reference", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithMetadata({
        composer_artifact_references: [
          { kind: "plan", sessionId: "older-session" },
          { kind: "PLAN", session_id: "latest-session" },
        ],
      }),
    ]);

    expect(result).toBe("latest-session");
  });

  it("ignores non-plan composer references before accepting legacy references", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithMetadata({
        composer_artifact_references: [
          { session_id: "legacy-session" },
          { kind: "note", sessionId: "ignored-note-session" },
        ],
      }),
    ]);

    expect(result).toBe("legacy-session");
  });

  it("keeps scanning messages when composer metadata is malformed", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithToolCall({ session_id: "session-from-tool" }),
      messageWithMetadata("{not-json"),
    ]);

    expect(result).toBe("session-from-tool");
  });

  it("falls back when composer metadata has no usable plan reference", () => {
    const result = resolveAttachedIdeationSessionId(
      conversation,
      [
        messageWithMetadata({
          composer_artifact_references: [
            { kind: "plan", sessionId: "" },
            { kind: "note", sessionId: "ignored-note-session" },
            null,
            "ignored",
          ],
        }),
        messageWithMetadata({ composer_artifact_references: "not-a-list" }),
        messageWithMetadata("42"),
      ],
      "session-linked",
    );

    expect(result).toBe("session-linked");
  });

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

  it("extracts the cloned session id from a plan-imported v1_start_ideation result", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithToolCall(
        {
          session_id: "cloned-session-1",
          plan_imported: true,
          cloned_plan_artifact_id: "cloned-artifact-1",
          source_plan_artifact_id: "source-artifact-1",
        },
        "v1_start_ideation",
      ),
    ]);

    expect(result).toBe("cloned-session-1");
  });

  it("falls back to the linked workspace session when no transcript tool result is available", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [], "session-linked");

    expect(result).toBe("session-linked");
  });
});
