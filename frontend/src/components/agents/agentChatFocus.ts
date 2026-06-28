import type { AgentConversationWorkspaceMode } from "@/api/chat";
import type { AgentTaskRuntimeContextType } from "./agentTaskRuntimeContext";

export type AgentsChatFocus =
  | { type: "workspace" }
  | { type: "workspace_review"; conversationId: string }
  | { type: "ideation"; sessionId: string }
  | { type: "verification"; parentSessionId: string; childSessionId: string }
  | { type: "task_runtime"; taskId: string; contextType: AgentTaskRuntimeContextType };

export type AgentsChatFocusType = AgentsChatFocus["type"];
export type AgentsChatFocusTone = "accent" | "warning";

export interface AgentsChatFocusDisplay {
  type: Exclude<AgentsChatFocus["type"], "workspace">;
  label: string;
  description: string;
  tone: AgentsChatFocusTone;
}

export interface AgentsChatFocusSwitchOption {
  type: AgentsChatFocusType;
  label: string;
  description: string;
  tone?: AgentsChatFocusTone;
}

export function getAgentChatFocusSwitchOptions({
  mode,
  focusSwitcherIdeationSessionId,
  verificationFocusTarget,
  taskRuntimeFocusTarget,
  workspaceReviewFocusTarget,
  hasPlanArtifact,
}: {
  mode: AgentConversationWorkspaceMode | null;
  focusSwitcherIdeationSessionId: string | null;
  verificationFocusTarget: Extract<AgentsChatFocus, { type: "verification" }> | null;
  taskRuntimeFocusTarget: Extract<AgentsChatFocus, { type: "task_runtime" }> | null;
  workspaceReviewFocusTarget: Extract<AgentsChatFocus, { type: "workspace_review" }> | null;
  hasPlanArtifact: boolean;
}): AgentsChatFocusSwitchOption[] {
  const options: AgentsChatFocusSwitchOption[] = [
    {
      type: "workspace",
      label: "Workspace",
      description: "Show the workspace agent chat",
    },
  ];

  if (mode === "ideation" && focusSwitcherIdeationSessionId) {
    options.push({
      type: "ideation",
      label: "Ideation",
      description: "Show the attached ideation chat",
      tone: "accent",
    });
  }

  const canShowVerification =
    Boolean(verificationFocusTarget) &&
    (mode === "ideation" || (mode === "plan" && hasPlanArtifact));

  if (canShowVerification) {
    options.push({
      type: "verification",
      label: "Verification",
      description: "Show the verification agent chat",
      tone: "warning",
    });
  }

  if (workspaceReviewFocusTarget) {
    options.push({
      type: "workspace_review",
      label: "Review",
      description: "Show the Review chat",
      tone: "warning",
    });
  }

  if (taskRuntimeFocusTarget) {
    options.push({
      type: "task_runtime",
      label: "Task",
      description: "Show the task agent chat",
      tone: "accent",
    });
  }

  return options;
}

export function latestVerificationChildSessionIdQueryKey(
  parentSessionId: string | null | undefined,
) {
  return [
    "agents",
    "chat-focus",
    "latest-child-session-id",
    parentSessionId,
    "verification",
  ] as const;
}

export function latestVerificationChildSessionData(
  parentSessionId: string,
  childSessionId: string | null,
) {
  return {
    sessionId: parentSessionId,
    purpose: "verification" as const,
    latestChildSessionId: childSessionId,
  };
}

export function getFocusedArtifactIdeationSessionId(
  chatFocus: AgentsChatFocus,
): string | null {
  if (chatFocus.type === "ideation") {
    return chatFocus.sessionId;
  }
  if (chatFocus.type === "verification") {
    return chatFocus.parentSessionId;
  }
  return null;
}

export function getAgentsChatFocusDisplay(
  chatFocus: AgentsChatFocus,
): AgentsChatFocusDisplay | null {
  if (chatFocus.type === "ideation") {
    return {
      type: "ideation",
      label: "Ideation",
      description: "Focused on an ideation run",
      tone: "accent",
    };
  }

  if (chatFocus.type === "verification") {
    return {
      type: "verification",
      label: "Verification",
      description: "Focused on a verification run",
      tone: "warning",
    };
  }

  if (chatFocus.type === "task_runtime") {
    return {
      type: "task_runtime",
      label: "Task",
      description: "Focused on a task agent run",
      tone: "accent",
    };
  }

  if (chatFocus.type === "workspace_review") {
    return {
      type: "workspace_review",
      label: "Review",
      description: "Focused on a Review run",
      tone: "warning",
    };
  }

  return null;
}

export function getFocusedChatSessionId(chatFocus: AgentsChatFocus): string | null {
  if (chatFocus.type === "ideation") {
    return chatFocus.sessionId;
  }
  if (chatFocus.type === "verification") {
    return chatFocus.childSessionId;
  }
  return null;
}

export function getFocusedWorkspaceReviewConversationId(
  chatFocus: AgentsChatFocus,
): string | null {
  if (chatFocus.type === "workspace_review") {
    return chatFocus.conversationId;
  }
  return null;
}
