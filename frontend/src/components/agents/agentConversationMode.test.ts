import { describe, expect, it } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";

import { isWorkspaceModeLocked } from "./agentConversationMode";

function workspace(
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "ideation",
    baseRefKind: "current_branch",
    baseRef: "main",
    baseDisplayName: "Current branch (main)",
    baseCommit: null,
    branchName: "ralphx/project/agent-conversation",
    worktreePath: "/tmp/agent-conversation",
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: null,
    status: "active",
    createdAt: "2026-05-15T00:00:00.000Z",
    updatedAt: "2026-05-15T00:00:00.000Z",
    ...overrides,
  };
}

describe("isWorkspaceModeLocked", () => {
  it("uses the backend mode-switch projection when present", () => {
    expect(
      isWorkspaceModeLocked(
        workspace({
          linkedIdeationSessionId: "session-1",
          linkedPlanBranchId: "plan-branch-1",
          modeSwitchLocked: false,
        }),
      ),
    ).toBe(false);

    expect(
      isWorkspaceModeLocked(
        workspace({
          modeSwitchLocked: true,
          modeSwitchLockReason: "Plan execution is still active",
        }),
      ),
    ).toBe(true);
  });

  it("falls back to legacy link presence for older responses", () => {
    expect(isWorkspaceModeLocked(workspace({ linkedIdeationSessionId: "session-1" }))).toBe(true);
    expect(isWorkspaceModeLocked(workspace())).toBe(false);
  });
});
