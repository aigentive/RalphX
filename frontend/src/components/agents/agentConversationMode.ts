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
  { id: "ideation", label: "Ideation", description: "Plan work before creating tasks." },
];

export function resolveConversationAgentMode(
  conversation: AgentConversation,
  workspace: AgentConversationWorkspace | null
): AgentConversationWorkspaceMode {
  return conversation.agentMode ?? workspace?.mode ?? "chat";
}

export function isWorkspaceModeLocked(workspace: AgentConversationWorkspace | null): boolean {
  if (!workspace) {
    return false;
  }
  if (workspace.modeSwitchLocked !== undefined) {
    return workspace.modeSwitchLocked;
  }
  return Boolean(workspace.linkedIdeationSessionId || workspace.linkedPlanBranchId);
}
