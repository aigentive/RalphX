import { describe, expect, it } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";

import {
  AGENT_CONVERSATION_MODE_OPTIONS,
  isConversationModeLocked,
} from "./agentConversationMode";
import type { AgentConversation } from "./agentConversations";
import { AGENT_MODE_OPTIONS } from "./agentStartModeOptions";

function conversation(
  overrides: Partial<AgentConversation> = {},
): AgentConversation {
  return {
    id: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    claudeSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    title: "Agent",
    messageCount: 0,
    lastMessageAt: null,
    createdAt: "2026-05-15T00:00:00.000Z",
    updatedAt: "2026-05-15T00:00:00.000Z",
    archivedAt: null,
    projectId: "project-1",
    ideationSessionId: null,
    ...overrides,
  };
}

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

describe("isConversationModeLocked", () => {
  it("uses the backend mode-switch projection when present", () => {
    expect(
      isConversationModeLocked(
        conversation(),
        workspace({
          linkedIdeationSessionId: "session-1",
          linkedPlanBranchId: "plan-branch-1",
          modeSwitchLocked: false,
        }),
      ),
    ).toBe(false);

    expect(
      isConversationModeLocked(
        conversation(),
        workspace({
          modeSwitchLocked: true,
          modeSwitchLockReason: "Plan execution is still active",
        }),
      ),
    ).toBe(true);
  });

  it("falls back to legacy link presence for older responses", () => {
    expect(
      isConversationModeLocked(
        conversation(),
        workspace({ linkedIdeationSessionId: "session-1" }),
      ),
    ).toBe(true);
    expect(isConversationModeLocked(conversation(), workspace())).toBe(false);
  });

  it("locks automation and persona builder conversations without workspace rows", () => {
    expect(
      isConversationModeLocked(conversation({ agentMode: "automation" }), null),
    ).toBe(true);
    expect(
      isConversationModeLocked(conversation({ agentMode: "persona_builder" }), null),
    ).toBe(true);
  });
});

describe("AGENT_CONVERSATION_MODE_OPTIONS", () => {
  it("offers automation as a first-class starter mode", () => {
    expect(AGENT_CONVERSATION_MODE_OPTIONS.map((option) => option.id)).toContain(
      "automation",
    );
  });

  it("excludes persona builder from selectable mode options", () => {
    expect(AGENT_CONVERSATION_MODE_OPTIONS.map((option) => option.id)).not.toContain(
      "persona_builder",
    );
    expect(AGENT_MODE_OPTIONS.map((option) => option.id)).not.toContain(
      "persona_builder",
    );
  });
});
