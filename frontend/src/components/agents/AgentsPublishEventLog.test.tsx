import { describe, expect, it } from "vitest";

import type { AgentConversationWorkspacePublicationEvent } from "@/api/chat";

import { selectPublishHistory } from "./AgentsPublishEventLog";

function event(
  overrides: Partial<AgentConversationWorkspacePublicationEvent> = {},
): AgentConversationWorkspacePublicationEvent {
  return {
    id: "event-1",
    conversationId: "conversation-1",
    step: "published",
    status: "succeeded",
    summary: "Published pull request",
    classification: null,
    attemptId: "attempt-current",
    createdAt: "2026-07-27T11:00:00Z",
    ...overrides,
  };
}

describe("selectPublishHistory", () => {
  it("keeps current-attempt evidence authoritative while preserving legacy history", () => {
    const result = selectPublishHistory(
      [
        event({ id: "legacy", attemptId: null, summary: "Legacy publish" }),
        event({ id: "stale", attemptId: "attempt-stale", summary: "Stale publish" }),
        event({ id: "current", summary: "Current publish" }),
      ],
      false,
      "attempt-current",
    );

    expect(result.visibleEvents.map((item) => item.id)).toEqual([
      "current",
      "legacy",
    ]);
  });
});
