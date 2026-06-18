import { describe, expect, it } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";

import type { AgentConversation } from "./agentConversations";
import { DEFAULT_AGENT_RUNTIME } from "./agentOptions";
import {
  getAgentTerminalUnavailableReason,
  runtimeFromConversation,
} from "./agentConversationRuntime";

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

function agentWorkspace(
  overrides: Partial<AgentConversationWorkspace> = {}
): AgentConversationWorkspace {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "edit",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    baseCommit: null,
    branchName: "ralphx/ralphx/agent-conversation-1",
    worktreePath: "/tmp/project/conversation-1",
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: null,
    autoPublishEnabled: true,
    autoPublishPausedPrAutofixEnabled: null,
    autoPublishPausedPrAutoMergeDesired: null,
    status: "active",
    createdAt: "2026-05-22T00:00:00.000Z",
    updatedAt: "2026-05-22T00:00:00.000Z",
    ...overrides,
  };
}

describe("getAgentTerminalUnavailableReason", () => {
  it("allows terminal access for linked edit workspaces", () => {
    expect(
      getAgentTerminalUnavailableReason(
        projectConversation(),
        agentWorkspace({
          mode: "edit",
          linkedIdeationSessionId: "ideation-session-1",
        }),
      ),
    ).toBeNull();
  });

  it("keeps terminal access disabled for linked plan-owned workspaces", () => {
    expect(
      getAgentTerminalUnavailableReason(
        projectConversation(),
        agentWorkspace({
          mode: "plan",
          linkedIdeationSessionId: "ideation-session-1",
        }),
      ),
    ).toBe(
      "Terminal disabled while ideation or execution owns this workspace",
    );
    expect(
      getAgentTerminalUnavailableReason(
        projectConversation(),
        agentWorkspace({
          mode: "edit",
          linkedPlanBranchId: "plan-branch-1",
        }),
      ),
    ).toBe(
      "Terminal disabled while ideation or execution owns this workspace",
    );
  });
});

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
