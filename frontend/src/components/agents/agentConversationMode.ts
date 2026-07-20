import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
} from "@/api/chat";

import type { AgentConversation } from "./agentConversations";

export const AGENT_CONVERSATION_MODE_OPTIONS: Array<{
  id: AgentConversationWorkspaceMode;
  label: string;
  description: string;
  disabled?: boolean;
  disabledReason?: string;
}> = [
  { id: "chat", label: "Ask", description: "Ask read-only questions about the project." },
  { id: "edit", label: "Agent", description: "Build, change, and review code in a branch." },
  { id: "plan", label: "Plan", description: "Draft and refine a plan before execution." },
  { id: "automation", label: "Automation", description: "Create and run a recurring agent workflow." },
  { id: "persona_builder", label: "Persona", description: "Build or refine a reusable agent persona.", disabled: true, disabledReason: "Persona mode is fixed when the conversation starts." },
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

export function buildConversationModeOptions(
  conversation: AgentConversation,
  workspace: AgentConversationWorkspace | null,
) {
  if (!isConversationModeLocked(conversation, workspace)) {
    return AGENT_CONVERSATION_MODE_OPTIONS;
  }
  const lockReason =
    workspace?.modeSwitchLockReason ??
    "This conversation's mode is locked.";
  return AGENT_CONVERSATION_MODE_OPTIONS.map((option) => ({
    ...option,
    disabled: true,
    disabledReason: lockReason,
  }));
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
