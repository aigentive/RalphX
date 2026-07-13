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
  { id: "automation", label: "Automation", description: "Create and run a recurring agent workflow." },
  { id: "review_pr", label: "Review PR", description: "Review a linked pull request." },
];

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
