import { describe, expect, it } from "vitest";

import {
  focusWorkspaceReview,
  getAgentChatFocusSwitchOptions,
  getAgentsChatFocusDisplay,
  getConversationScopedChatFocus,
  getFocusedAutomationRunConversationId,
  getFocusedArtifactIdeationSession,
  getFocusedChatSessionId,
  getFocusedWorkspaceReviewConversationId,
  type AgentsChatFocus,
} from "./agentChatFocus";

const verificationFocus: Extract<AgentsChatFocus, { type: "verification" }> = {
  type: "verification",
  conversationId: "conversation-1",
  parentSessionId: "session-1",
  childSessionId: "verification-1",
};

describe("conversation-scoped plan focus", () => {
  it("rejects a stale ideation focus owned by another conversation", () => {
    const staleFocus: AgentsChatFocus = {
      type: "ideation",
      conversationId: "conversation-1",
      sessionId: "session-1",
    };

    expect(
      getConversationScopedChatFocus(staleFocus, "conversation-2"),
    ).toEqual({ type: "workspace" });
    expect(
      getFocusedArtifactIdeationSession(
        getConversationScopedChatFocus(staleFocus, "conversation-2"),
      ),
    ).toBeNull();
  });

  it("preserves the focused session owned by the visible conversation", () => {
    expect(
      getFocusedArtifactIdeationSession(
        getConversationScopedChatFocus(verificationFocus, "conversation-1"),
      ),
    ).toEqual({
      conversationId: "conversation-1",
      sessionId: "session-1",
    });
  });
});
const taskRuntimeFocus: Extract<AgentsChatFocus, { type: "task_runtime" }> = {
  type: "task_runtime",
  taskId: "task-1",
  contextType: "review",
};
const workspaceReviewFocus: Extract<
  AgentsChatFocus,
  { type: "workspace_review" }
> = {
  type: "workspace_review",
  conversationId: "review-conversation-1",
};
const automationRunFocus: Extract<AgentsChatFocus, { type: "automation_run" }> = {
  type: "automation_run",
  automationId: "automation-1",
  runId: "run-1",
  conversationId: "automation-run-conversation-1",
};

describe("getAgentChatFocusSwitchOptions", () => {
  it("keeps the full ideation focus switcher in ideation mode", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "ideation",
      focusSwitcherIdeationSessionId: "session-1",
      verificationFocusTarget: verificationFocus,
      taskRuntimeFocusTarget: null,
      workspaceReviewFocusTarget: null,
      automationRunFocusTarget: null,
      hasPlanArtifact: true,
    });

    expect(options.map((option) => option.type)).toEqual([
      "workspace",
      "ideation",
      "verification",
    ]);
  });

  it("shows only verification as a child focus in plan mode when a plan and verification child exist", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "plan",
      focusSwitcherIdeationSessionId: "session-1",
      verificationFocusTarget: verificationFocus,
      taskRuntimeFocusTarget: null,
      workspaceReviewFocusTarget: null,
      automationRunFocusTarget: null,
      hasPlanArtifact: true,
    });

    expect(options.map((option) => option.type)).toEqual([
      "workspace",
      "verification",
    ]);
  });

  it("hides verification in plan mode until a plan exists", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "plan",
      focusSwitcherIdeationSessionId: "session-1",
      verificationFocusTarget: verificationFocus,
      taskRuntimeFocusTarget: null,
      workspaceReviewFocusTarget: null,
      automationRunFocusTarget: null,
      hasPlanArtifact: false,
    });

    expect(options.map((option) => option.type)).toEqual(["workspace"]);
  });

  it("keeps non-planning modes workspace-only", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "edit",
      focusSwitcherIdeationSessionId: "session-1",
      verificationFocusTarget: verificationFocus,
      taskRuntimeFocusTarget: null,
      workspaceReviewFocusTarget: null,
      automationRunFocusTarget: null,
      hasPlanArtifact: true,
    });

    expect(options.map((option) => option.type)).toEqual(["workspace"]);
  });

  it("adds task runtime focus whenever a task runtime target is active", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "edit",
      focusSwitcherIdeationSessionId: null,
      verificationFocusTarget: null,
      taskRuntimeFocusTarget: taskRuntimeFocus,
      workspaceReviewFocusTarget: null,
      automationRunFocusTarget: null,
      hasPlanArtifact: false,
    });

    expect(options.map((option) => option.type)).toEqual([
      "workspace",
      "task_runtime",
    ]);
    expect(options[1]).toMatchObject({
      label: "Task",
      description: "Show the task agent chat",
      tone: "accent",
    });
  });

  it("adds workspace Review focus whenever the child review chat exists", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "edit",
      focusSwitcherIdeationSessionId: null,
      verificationFocusTarget: null,
      taskRuntimeFocusTarget: null,
      workspaceReviewFocusTarget: workspaceReviewFocus,
      automationRunFocusTarget: null,
      hasPlanArtifact: false,
    });

    expect(options.map((option) => option.type)).toEqual([
      "workspace",
      "workspace_review",
    ]);
    expect(options[1]).toMatchObject({
      label: "Review",
      description: "Show the Review chat",
      tone: "warning",
    });
  });

  it("adds automation run focus when a run conversation is selected from setup", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "automation",
      focusSwitcherIdeationSessionId: null,
      verificationFocusTarget: null,
      taskRuntimeFocusTarget: null,
      workspaceReviewFocusTarget: null,
      automationRunFocusTarget: automationRunFocus,
      hasPlanArtifact: false,
    });

    expect(options.map((option) => option.type)).toEqual([
      "workspace",
      "automation_run",
    ]);
    expect(options[1]).toMatchObject({
      label: "Run",
      description: "Show the automation run chat",
      tone: "accent",
    });
  });
});

describe("task runtime focus helpers", () => {
  it("describes task runtime focus without mapping it to an ideation chat session", () => {
    expect(getAgentsChatFocusDisplay(taskRuntimeFocus)).toEqual({
      type: "task_runtime",
      label: "Task",
      description: "Focused on a task agent run",
      tone: "accent",
    });
    expect(getFocusedChatSessionId(taskRuntimeFocus)).toBeNull();
  });
});

describe("automation run focus helpers", () => {
  it("describes automation run focus and maps it to a child conversation", () => {
    expect(getAgentsChatFocusDisplay(automationRunFocus)).toEqual({
      type: "automation_run",
      label: "Run",
      description: "Focused on an automation run",
      tone: "accent",
    });
    expect(getFocusedAutomationRunConversationId(automationRunFocus)).toBe(
      "automation-run-conversation-1",
    );
    expect(getFocusedChatSessionId(automationRunFocus)).toBeNull();
  });
});

describe("workspace Review focus helpers", () => {
  it("describes workspace Review focus without mapping it to an ideation chat session", () => {
    expect(getAgentsChatFocusDisplay(workspaceReviewFocus)).toEqual({
      type: "workspace_review",
      label: "Review",
      description: "Focused on a Review run",
      tone: "warning",
    });
    expect(getFocusedChatSessionId(workspaceReviewFocus)).toBeNull();
    expect(getFocusedWorkspaceReviewConversationId(workspaceReviewFocus)).toBe(
      "review-conversation-1",
    );
  });

  it("clears a transient runtime hint when focus switches to another Review child", () => {
    expect(
      focusWorkspaceReview(
        {
          type: "workspace_review",
          conversationId: "review-conversation-1",
          runtimeHint: {
            provider: "codex",
            modelId: "gpt-5.5",
            effort: "high",
          },
        },
        "review-conversation-2",
      ),
    ).toEqual({
      type: "workspace_review",
      conversationId: "review-conversation-2",
    });
  });
});
