import { describe, expect, it } from "vitest";

import type { AgentConversation } from "./agentConversations";
import { DEFAULT_AGENT_RUNTIME } from "./agentOptions";
import { runtimeFromConversation } from "./agentConversationRuntime";

function projectConversation(
  overrides: Partial<AgentConversation> = {}
): AgentConversation {
  return {
    id: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    projectId: "project-1",
    ideationSessionId: null,
    claudeSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    upstreamProvider: null,
    providerProfile: null,
    logicalModel: null,
    effectiveModelId: null,
    logicalEffort: null,
    effectiveEffort: null,
    agentMode: "chat",
    parentConversationId: null,
    title: "Conversation",
    messageCount: 0,
    lastMessageAt: null,
    createdAt: "2026-05-22T00:00:00.000Z",
    updatedAt: "2026-05-22T00:00:00.000Z",
    archivedAt: null,
    ...overrides,
  };
}

describe("runtimeFromConversation", () => {
  it("hydrates Claude runtime from conversation attribution", () => {
    expect(
      runtimeFromConversation(
        projectConversation({
          providerHarness: "claude",
          logicalModel: "opus",
          effectiveModelId: "claude-opus-4-7-20260501",
          logicalEffort: "max",
          effectiveEffort: "max",
        }),
      ),
    ).toEqual({
      provider: "claude",
      modelId: "opus",
      effort: "max",
    });
  });

  it("hydrates Codex runtime from conversation attribution", () => {
    expect(
      runtimeFromConversation(
        projectConversation({
          providerHarness: "codex",
          logicalModel: "gpt-5.4",
          effectiveModelId: "gpt-5.4-2026-04-01",
          logicalEffort: "high",
          effectiveEffort: "high",
        }),
      ),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.4",
      effort: "high",
    });
  });

  it("falls back to effective fields and provider defaults", () => {
    expect(
      runtimeFromConversation(
        projectConversation({
          providerHarness: "codex",
          effectiveModelId: "gpt-5.4-mini",
          logicalEffort: "retired-effort",
          effectiveEffort: "medium",
        }),
      ),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.4-mini",
      effort: "medium",
    });

    expect(
      runtimeFromConversation(
        projectConversation({
          providerHarness: "codex",
          logicalEffort: "retired-effort",
        }),
      ),
    ).toEqual(DEFAULT_AGENT_RUNTIME);
  });
});
