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

  it("extracts session ids from plain text tool results", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithToolCall(
        "Productive session ae4249ec-43c6-4123-8c55-9b5ddd446889 is accepted.",
        "ralphx::v1_get_ideation_status",
      ),
    ]);

    expect(result).toBe("ae4249ec-43c6-4123-8c55-9b5ddd446889");
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

  it("ignores recognized tool calls without session-bearing payloads", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithToolCall(false, "ralphx::v1_get_ideation_status"),
    ]);

    expect(result).toBeNull();
  });

  it("prefers productive spawned ideation sessions over stale workspace links", () => {
    const result = resolveAttachedIdeationSessionId(
      conversation,
      [
        messageWithToolCall(
          {
            sessions: [
              {
                id: "stale-shell-session",
                title: "Continue ClickUp integration implementation",
                status: "active",
                proposal_count: 0,
              },
              {
                id: "productive-session",
                title: "Implement ClickUp integration",
                status: "accepted",
                proposal_count: 4,
              },
            ],
          },
          "ralphx::v1_list_ideation_sessions",
        ),
      ],
      "stale-shell-session",
    );

    expect(result).toBe("productive-session");
  });

  it("extracts productive sessions from appended task results", () => {
    const result = resolveAttachedIdeationSessionId(
      conversation,
      [
        messageWithToolCall(
          {
            session_id: "productive-session",
            created_task_ids: ["task-1"],
            session_status: "accepted",
          },
          "ralphx::v1_append_task_to_plan",
        ),
      ],
      "stale-shell-session",
    );

    expect(result).toBe("productive-session");
  });

  it("prefers productive status results over a stale workspace link", () => {
    const result = resolveAttachedIdeationSessionId(
      conversation,
      [
        messageWithToolCall(
          {
            content: [
              {
                type: "text",
                text: JSON.stringify({
                  session_id: "productive-session",
                  project_id: "project-1",
                  title: "Implement ClickUp integration",
                  status: "accepted",
                  proposal_count: 4,
                  verification_status: "unverified",
                  delivery_status: "in_progress",
                }),
              },
            ],
            structured_content: null,
          },
          "ralphx::v1_get_ideation_status",
        ),
      ],
      "stale-shell-session",
    );

    expect(result).toBe("productive-session");
  });
});
