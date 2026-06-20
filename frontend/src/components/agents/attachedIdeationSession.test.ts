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
  it("returns the fallback when there is no active conversation", () => {
    const result = resolveAttachedIdeationSessionId(null, [], "fallback-session");

    expect(result).toBe("fallback-session");
  });

  it("returns the ideation context id for ideation conversations", () => {
    const ideationConversation = toProjectAgentConversation({
      id: "conversation-ideation",
      contextType: "ideation",
      contextId: "ideation-session-1",
      claudeSessionId: null,
      providerSessionId: null,
      providerHarness: null,
      upstreamProvider: null,
      providerProfile: null,
      title: "Ideation",
      messageCount: 1,
      lastMessageAt: null,
      createdAt: "2026-04-22T10:00:00Z",
      updatedAt: "2026-04-22T10:00:00Z",
      archivedAt: null,
    } satisfies ChatConversation);

    const result = resolveAttachedIdeationSessionId(
      ideationConversation,
      [],
      "fallback-session",
    );

    expect(result).toBe("ideation-session-1");
  });

  it("extracts attached plan sessions from user composer artifact references", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      {
        id: "message-with-reference",
        conversationId: "conversation-1",
        role: "user",
        content: "Verify the referenced plan",
        contentBlocks: [],
        toolCalls: [],
        attachments: [],
        metadata: JSON.stringify({
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
        createdAt: "2026-04-22T10:00:30Z",
      } as ChatMessageResponse,
    ]);

    expect(result).toBe("referenced-session-1");
  });

  it("uses the latest valid composer plan reference", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      {
        id: "message-with-reference",
        conversationId: "conversation-1",
        role: "user",
        content: "Verify the referenced plan",
        contentBlocks: [],
        toolCalls: [],
        attachments: [],
        metadata: JSON.stringify({
          composer_artifact_references: [
            {
              kind: "plan",
              session_id: "older-session",
            },
            {
              kind: "issue",
              session_id: "ignored-issue-session",
            },
            {
              session_id: "latest-plan-session",
            },
          ],
        }),
        createdAt: "2026-04-22T10:00:30Z",
      } as ChatMessageResponse,
    ]);

    expect(result).toBe("latest-plan-session");
  });

  it("ignores malformed composer metadata and unsupported tools", () => {
    const result = resolveAttachedIdeationSessionId(
      conversation,
      [
        {
          id: "bad-metadata",
          conversationId: "conversation-1",
          role: "user",
          content: "Bad metadata",
          contentBlocks: [],
          toolCalls: [],
          attachments: [],
          metadata: "{not-json",
          createdAt: "2026-04-22T10:00:30Z",
        } as ChatMessageResponse,
        messageWithToolCall(
          { session_id: "ignored-session" },
          "mcp__ralphx__unrelated_tool",
        ),
      ],
      "fallback-session",
    );

    expect(result).toBe("fallback-session");
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

  it("extracts session ids from tool call arguments when results are empty", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      {
        ...messageWithToolCall(null, "mcp__ralphx__get_session_plan"),
        contentBlocks: [],
        toolCalls: [
          {
            id: "tool-call-1",
            name: "mcp__ralphx__get_session_plan",
            arguments: {
              session: {
                childSessionId: "session-from-arguments",
              },
            },
            result: null,
          },
        ],
      } as ChatMessageResponse,
    ]);

    expect(result).toBe("session-from-arguments");
  });

  it("extracts nested child session ids from array payloads", () => {
    const result = resolveAttachedIdeationSessionId(conversation, [
      messageWithToolCall(
        [
          { data: null },
          { structuredContent: { child_session_id: "nested-child-session" } },
        ],
        "mcp__ralphx__create_child_session",
      ),
    ]);

    expect(result).toBe("nested-child-session");
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
