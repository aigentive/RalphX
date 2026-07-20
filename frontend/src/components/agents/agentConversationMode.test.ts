import { describe, expect, it } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";

import {
  AGENT_CONVERSATION_MODE_OPTIONS,
  buildConversationModeOptions,
  buildAgentConversationModeOptions,
  isConversationModeLocked,
} from "./agentConversationMode";
import type { AgentConversation } from "./agentConversations";
import {
  AGENT_START_MODE_OPTIONS,
  buildAgentStartModeOptions,
} from "./agentStartModeOptions";

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

  it("does not treat durable Tasks return context as an active ownership lock", () => {
    expect(
      isConversationModeLocked(
        conversation({ agentMode: "tasks" }),
        workspace({
          mode: "tasks",
          taskPipelineSessionId: "session-1",
        }),
      ),
    ).toBe(false);
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

  it("includes Persona as a label-only conversation mode", () => {
    expect(AGENT_CONVERSATION_MODE_OPTIONS).toContainEqual(
      expect.objectContaining({ id: "persona_builder", label: "Persona", disabled: true }),
    );
  });

  it.each(["automation", "persona_builder"] as const)(
    "disables every conversation-mode option while %s is locked",
    (mode) => {
      const options = buildConversationModeOptions(
        conversation({ agentMode: mode }),
        null,
      );
      expect(options).not.toHaveLength(0);
      expect(options.every((option) => option.disabled)).toBe(true);
    },
  );

  it("keeps Persona in the starter registry for feature-gated rendering", () => {
    expect(AGENT_START_MODE_OPTIONS).toContainEqual(
      expect.objectContaining({ id: "persona_builder", label: "Persona" }),
    );
  });

  it("orders ordinary new-run modes by planning, execution, review, and inquiry", () => {
    expect(
      AGENT_START_MODE_OPTIONS.slice(0, 5).map(({ id, label }) => ({ id, label })),
    ).toEqual([
      { id: "plan", label: "Plan" },
      { id: "edit", label: "Agent" },
      { id: "review_pr", label: "Review PR" },
      { id: "chat", label: "Ask" },
      { id: "automation", label: "Automation" },
    ]);
  });

  it("keeps Tasks out of fresh conversations and gates Autopilot", () => {
    expect(
      buildAgentStartModeOptions({ autopilotEnabled: false }).map(
        (option) => option.id,
      ),
    ).not.toContain("tasks");
    expect(
      buildAgentStartModeOptions({ autopilotEnabled: false }).map(
        (option) => option.id,
      ),
    ).not.toContain("autopilot");
    expect(
      buildAgentStartModeOptions({ autopilotEnabled: true }).map(
        (option) => option.id,
      ),
    ).toContain("autopilot");
  });

  it("offers Tasks only for the current or durably attached pipeline", () => {
    expect(
      buildAgentConversationModeOptions({
        currentMode: "edit",
        taskPipelineAvailable: false,
        autopilotEnabled: false,
      }).map((option) => option.id),
    ).not.toContain("tasks");
    expect(
      buildAgentConversationModeOptions({
        currentMode: "edit",
        taskPipelineAvailable: true,
        autopilotEnabled: false,
      }).map((option) => option.id),
    ).toContain("tasks");
  });

  it("keeps a disabled current Autopilot mode visible after opt-out", () => {
    const autopilot = buildAgentConversationModeOptions({
      currentMode: "autopilot",
      taskPipelineAvailable: false,
      autopilotEnabled: false,
    }).find((option) => option.id === "autopilot");

    expect(autopilot).toMatchObject({ disabled: true });
  });
});
