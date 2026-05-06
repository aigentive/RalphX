import type { AgentConversationWorkspace } from "@/api/chat";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";
import { DEFAULT_AGENT_RUNTIME } from "./agentOptions";

export function getAgentTerminalUnavailableReason(
  conversation: AgentConversation | null,
  workspace: AgentConversationWorkspace | null,
): string | null {
  if (!conversation) {
    return "Select an agent conversation";
  }
  if (conversation.contextType !== "project") {
    return "Terminal is available for project conversations";
  }
  if (!workspace) {
    return "Terminal requires a workspace-backed conversation";
  }
  if (workspace.status === "missing") {
    return "Terminal unavailable because the workspace is missing";
  }
  if (workspace.linkedIdeationSessionId || workspace.linkedPlanBranchId) {
    return "Terminal disabled while ideation or execution owns this workspace";
  }
  return null;
}

export function runtimeFromConversation(
  conversation: AgentConversation | null
): AgentRuntimeSelection | null {
  if (!conversation?.providerHarness) {
    return null;
  }

  if (conversation.providerHarness === "claude") {
    return {
      provider: "claude",
      modelId: "sonnet",
      effort: "medium",
    };
  }

  if (conversation.providerHarness === "codex") {
    return {
      provider: "codex",
      modelId: DEFAULT_AGENT_RUNTIME.modelId,
      effort: DEFAULT_AGENT_RUNTIME.effort,
    };
  }

  return null;
}
