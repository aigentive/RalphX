import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
} from "@/api/chat";

import type { AgentConversation } from "./agentConversations";

export const AGENT_CONVERSATION_MODE_OPTIONS: Array<{
  id: AgentConversationWorkspaceMode;
  label: string;
  description: string;
}> = [
  { id: "chat", label: "Chat", description: "Ask read-only questions about the project." },
  { id: "edit", label: "Agent", description: "Build, change, and review code in a branch." },
  { id: "plan", label: "Plan", description: "Draft and refine a plan before execution." },
  { id: "automation", label: "Automation", description: "Create and run a recurring agent workflow." },
  { id: "review_pr", label: "Review PR", description: "Review a linked pull request." },
];

const TASKS_MODE_OPTION = {
  id: "tasks" as const,
  label: "Tasks",
  description: "Review proposals and supervise the attached task pipeline.",
};

const AUTOPILOT_MODE_OPTION = {
  id: "autopilot" as const,
  label: "Autopilot",
  description: "Plan, create tasks, and start execution with minimal supervision.",
};

export function buildAgentConversationModeOptions({
  currentMode,
  taskPipelineAvailable,
  autopilotEnabled,
}: {
  currentMode: AgentConversationWorkspaceMode;
  taskPipelineAvailable: boolean;
  autopilotEnabled: boolean;
}) {
  const options = [...AGENT_CONVERSATION_MODE_OPTIONS];
  if (currentMode === "tasks" || taskPipelineAvailable) {
    options.splice(3, 0, TASKS_MODE_OPTION);
  }
  if (autopilotEnabled || currentMode === "autopilot") {
    options.splice(4, 0, {
      ...AUTOPILOT_MODE_OPTION,
      ...(autopilotEnabled
        ? {}
        : {
            disabled: true,
            disabledReason: "Enable Autopilot in Agent capabilities to re-enter.",
          }),
    });
  }
  return options;
}

export function resolveConversationAgentMode(
  conversation: AgentConversation,
  workspace: AgentConversationWorkspace | null
): AgentConversationWorkspaceMode {
  return conversation.agentMode ?? workspace?.mode ?? "chat";
}

export function isConversationModeLocked(
  conversation: AgentConversation,
  workspace: AgentConversationWorkspace | null,
): boolean {
  const mode = resolveConversationAgentMode(conversation, workspace);
  if (mode === "automation" || mode === "persona_builder") {
    return true;
  }
  if (!workspace) {
    return false;
  }
  if (workspace.modeSwitchLocked !== undefined) {
    return workspace.modeSwitchLocked;
  }
  return Boolean(workspace.linkedIdeationSessionId || workspace.linkedPlanBranchId);
}
