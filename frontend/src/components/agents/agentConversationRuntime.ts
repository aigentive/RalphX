import type { AgentConversationWorkspace } from "@/api/chat";
import type {
  AgentEffort,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";
import { DEFAULT_AGENT_RUNTIME } from "./agentOptions";

const AGENT_EFFORTS = new Set<AgentEffort>([
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
]);

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
  const hasExternalWorkspaceOwner =
    Boolean(workspace.linkedPlanBranchId) ||
    workspaceIsLinkedNonEditWorkspace(workspace);
  if (hasExternalWorkspaceOwner) {
    return "Terminal disabled while ideation or execution owns this workspace";
  }
  return null;
}

function workspaceIsLinkedNonEditWorkspace(
  workspace: AgentConversationWorkspace
): boolean {
  return Boolean(workspace.linkedIdeationSessionId && workspace.mode !== "edit");
}

export function runtimeFromConversation(
  conversation: AgentConversation | null
): AgentRuntimeSelection | null {
  if (!conversation?.providerHarness) {
    return null;
  }

  const modelId =
    conversation.logicalModel?.trim() ||
    conversation.effectiveModelId?.trim() ||
    null;
  const effort = effortFromConversation(conversation);

  if (conversation.providerHarness === "claude") {
    return {
      provider: "claude",
      modelId: modelId ?? "sonnet",
      effort: effort ?? "medium",
    };
  }

  if (conversation.providerHarness === "codex") {
    return {
      provider: "codex",
      modelId: modelId ?? DEFAULT_AGENT_RUNTIME.modelId,
      effort: effort ?? DEFAULT_AGENT_RUNTIME.effort,
    };
  }

  return null;
}

function effortFromConversation(
  conversation: Pick<AgentConversation, "logicalEffort" | "effectiveEffort">
): AgentEffort | null {
  for (const value of [
    conversation.logicalEffort?.trim(),
    conversation.effectiveEffort?.trim(),
  ]) {
    if (AGENT_EFFORTS.has(value as AgentEffort)) {
      return value as AgentEffort;
    }
  }
  return null;
}
