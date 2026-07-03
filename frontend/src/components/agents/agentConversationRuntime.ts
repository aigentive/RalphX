import type { AgentConversationWorkspace } from "@/api/chat";
import type {
  AgentEffort,
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";
import { DEFAULT_AGENT_RUNTIME } from "./agentOptions";
import { getAgentWorkspaceTerminalPublicationStatus } from "./agentWorkspacePublishState";

const AGENT_EFFORTS = new Set<AgentEffort>([
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
]);

const WORKSPACE_REVIEW_UTILITY_MODEL_BY_PROVIDER: Record<AgentProvider, string> = {
  claude: "haiku",
  codex: "gpt-5.4-mini",
};

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
  if (workspaceHasExternalOwner(workspace)) {
    return "Terminal disabled while ideation or execution owns this workspace";
  }
  return null;
}

export function getAgentTerminalArchivedReason(
  conversation: AgentConversation | null,
  workspace: AgentConversationWorkspace | null,
): string | null {
  if (!conversation || conversation.contextType !== "project" || !workspace) {
    return null;
  }
  if (workspaceHasExternalOwner(workspace)) {
    return null;
  }

  const terminalPublicationStatus =
    getAgentWorkspaceTerminalPublicationStatus(workspace);
  if (terminalPublicationStatus === "merged") {
    return "Workspace archived after PR merge. Send a follow-up to continue in a fresh workspace.";
  }
  if (terminalPublicationStatus === "closed") {
    return "Workspace archived after PR close. Send a follow-up to continue in a fresh workspace.";
  }
  if (workspace.status === "missing") {
    return "Workspace missing. Send a follow-up to continue in a fresh workspace.";
  }
  return null;
}

function workspaceHasExternalOwner(workspace: AgentConversationWorkspace): boolean {
  return (
    Boolean(workspace.linkedPlanBranchId) ||
    workspaceIsLinkedNonEditWorkspace(workspace)
  );
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

export function workspaceReviewUtilityRuntimeForProvider(
  provider: AgentProvider
): AgentRuntimeSelection {
  return {
    provider,
    modelId: WORKSPACE_REVIEW_UTILITY_MODEL_BY_PROVIDER[provider],
    effort: "medium",
  };
}

export function runtimeForWorkspaceReviewFocus(
  workspaceRuntime: AgentRuntimeSelection | null,
  reviewRuntime: AgentRuntimeSelection | null
): AgentRuntimeSelection | null {
  if (reviewRuntime) {
    return reviewRuntime;
  }
  if (!workspaceRuntime) {
    return null;
  }
  return workspaceReviewUtilityRuntimeForProvider(workspaceRuntime.provider);
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
