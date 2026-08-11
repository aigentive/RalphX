import { describe, expect, it } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";
import { AGENT_MODEL_CATALOG } from "@/lib/agent-models";

import type { AgentConversation } from "./agentConversations";
import { DEFAULT_AGENT_RUNTIME } from "./agentOptions";
import {
  getAgentTerminalArchivedReason,
  getAgentTerminalUnavailableReason,
  runtimeFromConversation,
  runtimeFromManualRoleDefault,
  runtimeForWorkspaceReviewFocus,
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

  it("allows terminal access for linked plan-owned workspaces", () => {
    expect(
      getAgentTerminalUnavailableReason(
        projectConversation(),
        agentWorkspace({
          mode: "plan",
          linkedIdeationSessionId: "ideation-session-1",
        }),
      ),
    ).toBeNull();
    expect(
      getAgentTerminalUnavailableReason(
        projectConversation(),
        agentWorkspace({
          mode: "edit",
          linkedPlanBranchId: "plan-branch-1",
        }),
      ),
    ).toBeNull();
  });
});

describe("getAgentTerminalArchivedReason", () => {
  it("returns merge and close continuation copy for terminal-published workspaces", () => {
    expect(
      getAgentTerminalArchivedReason(
        projectConversation(),
        agentWorkspace({ publicationPrStatus: "merged" }),
      ),
    ).toBe(
      "Workspace archived after PR merge. Send a follow-up to continue in a fresh workspace.",
    );
    expect(
      getAgentTerminalArchivedReason(
        projectConversation(),
        agentWorkspace({ publicationPrStatus: " CLOSED " }),
      ),
    ).toBe(
      "Workspace archived after PR close. Send a follow-up to continue in a fresh workspace.",
    );
  });

  it("treats missing workspaces as an archived terminal shell state", () => {
    expect(
      getAgentTerminalUnavailableReason(
        projectConversation(),
        agentWorkspace({ status: "missing" }),
      ),
    ).toBeNull();
    expect(
      getAgentTerminalArchivedReason(
        projectConversation(),
        agentWorkspace({ status: "missing" }),
      ),
    ).toBe(
      "Workspace missing. Send a follow-up to continue in a fresh workspace.",
    );
  });

  it("archives terminal-published plan-owned workspaces", () => {
    const workspace = agentWorkspace({
      mode: "plan",
      linkedIdeationSessionId: "ideation-session-1",
      publicationPrStatus: "merged",
    });

    expect(getAgentTerminalArchivedReason(projectConversation(), workspace)).toBe(
      "Workspace archived after PR merge. Send a follow-up to continue in a fresh workspace.",
    );
    expect(getAgentTerminalUnavailableReason(projectConversation(), workspace)).toBeNull();
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

describe("workspace review runtime", () => {
  it("projects the backend Reviewer role default into a composer runtime", () => {
    expect(
      runtimeFromManualRoleDefault(
        {
          provider: "codex",
          model: "gpt-5.6-terra",
          effort: "ultra",
          serviceTier: "fast",
          coordinationMode: null,
          personaId: null,
          approvalPolicy: "never",
          sandboxMode: "danger-full-access",
        },
        AGENT_MODEL_CATALOG,
      ),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "ultra",
    });
  });

  it("uses the backend-resolved Reviewer role default before a child runtime exists", () => {
    expect(
      runtimeForWorkspaceReviewFocus(
        {
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        },
        null,
        {
          provider: "codex",
          modelId: "gpt-5.6-terra",
          effort: "ultra",
        },
      ),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "ultra",
    });
  });

  it("preserves the actual child runtime over the next-launch role default", () => {
    expect(
      runtimeForWorkspaceReviewFocus(
        {
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        },
        {
          provider: "claude",
          modelId: "sonnet",
          effort: "high",
        },
        {
          provider: "codex",
          modelId: "gpt-5.6-terra",
          effort: "ultra",
        },
      ),
    ).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "high",
    });
  });

  it("uses a committed reviewer focus hint before durable child metadata arrives", () => {
    expect(
      runtimeForWorkspaceReviewFocus(
        {
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        },
        null,
        {
          provider: "claude",
          modelId: "sonnet",
          effort: "high",
        },
        {
          provider: "codex",
          modelId: "gpt-5.6-terra",
          effort: "high",
        },
      ),
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "high",
    });
  });

  it("keeps durable child metadata ahead of a stale reviewer focus hint", () => {
    expect(
      runtimeForWorkspaceReviewFocus(
        null,
        {
          provider: "claude",
          modelId: "sonnet",
          effort: "high",
        },
        {
          provider: "codex",
          modelId: "gpt-5.6-terra",
          effort: "medium",
        },
        {
          provider: "codex",
          modelId: "gpt-5.6-terra",
          effort: "medium",
        },
      ),
    ).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "high",
    });
  });

  it("uses a current-session composer override ahead of hydrated child metadata", () => {
    expect(
      runtimeForWorkspaceReviewFocus(
        null,
        { provider: "codex", modelId: "gpt-5.5", effort: "high" },
        null,
        null,
        { provider: "claude", modelId: "sonnet", effort: "high" },
      ),
    ).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "high",
    });
  });

  it("falls back to the workspace runtime while the role default is unavailable", () => {
    const workspaceRuntime = {
      provider: "codex" as const,
      modelId: "gpt-5.5",
      effort: "xhigh" as const,
    };

    expect(
      runtimeForWorkspaceReviewFocus(workspaceRuntime, null, null),
    ).toEqual(workspaceRuntime);
  });
});
