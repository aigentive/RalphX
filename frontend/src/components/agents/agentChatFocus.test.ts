import { describe, expect, it } from "vitest";

import {
  getAgentChatFocusSwitchOptions,
  getAgentsChatFocusDisplay,
  getFocusedChatSessionId,
  getFocusedWorkspaceReviewConversationId,
  type AgentsChatFocus,
} from "./agentChatFocus";

const verificationFocus: Extract<AgentsChatFocus, { type: "verification" }> = {
  type: "verification",
  parentSessionId: "session-1",
  childSessionId: "verification-1",
};
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

describe("getAgentChatFocusSwitchOptions", () => {
  it("keeps the full ideation focus switcher in ideation mode", () => {
    const options = getAgentChatFocusSwitchOptions({
      mode: "ideation",
      focusSwitcherIdeationSessionId: "session-1",
      verificationFocusTarget: verificationFocus,
      taskRuntimeFocusTarget: null,
      workspaceReviewFocusTarget: null,
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
});
