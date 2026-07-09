import { describe, expect, it } from "vitest";

import { StateTransitionResponseSchemaRaw } from "./tasks.schemas";
import { transformStateTransition } from "./tasks.transforms";

describe("task API transforms", () => {
  it("carries optional runtime context and transition identity metadata", () => {
    const raw = StateTransitionResponseSchemaRaw.parse({
      from_status: "executing",
      to_status: "reviewing",
      trigger: "agent",
      timestamp: "2026-07-07T10:05:00Z",
      conversation_id: "conversation-1",
      agent_run_id: "run-1",
      context_type: "review",
      transition_id: "transition-1",
    });

    expect(transformStateTransition(raw)).toEqual({
      fromStatus: "executing",
      toStatus: "reviewing",
      trigger: "agent",
      timestamp: "2026-07-07T10:05:00Z",
      conversationId: "conversation-1",
      agentRunId: "run-1",
      contextType: "review",
      transitionId: "transition-1",
    });
  });

  it("ignores non-runtime context metadata instead of exposing invalid history context", () => {
    const raw = StateTransitionResponseSchemaRaw.parse({
      from_status: "backlog",
      to_status: "ready",
      trigger: "user",
      timestamp: "2026-07-07T10:00:00Z",
      context_type: "project",
    });

    expect(transformStateTransition(raw)).toEqual({
      fromStatus: "backlog",
      toStatus: "ready",
      trigger: "user",
      timestamp: "2026-07-07T10:00:00Z",
    });
  });
});
