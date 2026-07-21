import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
} from "@/api/chat";

import type { AgentConversation } from "./agentConversations";
import { AGENT_START_MODE_OPTIONS } from "./agentStartModeOptions";

export const AGENT_CONVERSATION_MODE_OPTIONS: Array<{
  id: AgentConversationWorkspaceMode;
  label: string;
  description: string;
  disabled?: boolean;
  disabledReason?: string;
}> = AGENT_START_MODE_OPTIONS.map(({ id, label, description }) => ({
  id,
  label,
  description,
  ...(id === "persona_builder"
    ? {
        disabled: true,
        disabledReason: "Persona mode is fixed when the conversation starts.",
      }
    : {}),
}));

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

const IDEATION_MODE_OPTION = {
  id: "ideation" as const,
  label: "Ideation",
  description: "Continue the linked planning conversation.",
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
    options.splice(4, 0, TASKS_MODE_OPTION);
  }
  if (autopilotEnabled || currentMode === "autopilot") {
    const personaIndex = options.findIndex(
      (option) => option.id === "persona_builder",
    );
    options.splice(personaIndex, 0, {
      ...AUTOPILOT_MODE_OPTION,
      ...(autopilotEnabled
        ? {}
        : {
            disabled: true,
            disabledReason: "Enable Autopilot in Agent capabilities to re-enter.",
          }),
    });
  }
  if (currentMode === "ideation") {
    options.push(IDEATION_MODE_OPTION);
  }

  const currentIndex = options.findIndex((option) => option.id === currentMode);
  if (currentIndex <= 0) {
    return options;
  }
  const [currentOption] = options.splice(currentIndex, 1);
  return currentOption ? [currentOption, ...options] : options;
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
