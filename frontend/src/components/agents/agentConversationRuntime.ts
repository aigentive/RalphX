import type { AgentConversationWorkspace } from "@/api/chat";
import type { ManualRoleDefault } from "@/api/manual-role-defaults.types";
import type { AgentModelRegistry } from "@/lib/agent-models";
import type {
  AgentEffort,
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";
import {
  DEFAULT_AGENT_RUNTIME,
  defaultEffortForModel,
  defaultModelForProvider,
} from "./agentOptions";
import { getAgentWorkspaceTerminalPublicationStatus } from "./agentWorkspacePublishState";

const AGENT_EFFORTS = new Set<AgentEffort>([
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
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
  return null;
}

export function getAgentTerminalArchivedReason(
  conversation: AgentConversation | null,
  workspace: AgentConversationWorkspace | null,
): string | null {
  if (!conversation || conversation.contextType !== "project" || !workspace) {
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

export function runtimeFromManualRoleDefault(
  roleDefault: ManualRoleDefault | null,
  modelRegistry: AgentModelRegistry,
): AgentRuntimeSelection | null {
  if (
    !roleDefault ||
    (roleDefault.provider !== "claude" && roleDefault.provider !== "codex")
  ) {
    return null;
  }
  const provider: AgentProvider = roleDefault.provider;
  const modelId =
    roleDefault.model?.trim() || defaultModelForProvider(provider, modelRegistry);
  const effort = roleDefault.effort?.trim();

  return {
    provider,
    modelId,
    effort: AGENT_EFFORTS.has(effort as AgentEffort)
      ? (effort as AgentEffort)
      : defaultEffortForModel(provider, modelId, modelRegistry),
  };
}

export function runtimeForWorkspaceReviewFocus(
  workspaceRuntime: AgentRuntimeSelection | null,
  reviewRuntime: AgentRuntimeSelection | null,
  reviewerRoleRuntime: AgentRuntimeSelection | null,
  reviewRuntimeHint: AgentRuntimeSelection | null = null,
  explicitComposerRuntime: AgentRuntimeSelection | null = null,
): AgentRuntimeSelection | null {
  return (
    explicitComposerRuntime ??
    reviewRuntime ??
    reviewRuntimeHint ??
    reviewerRoleRuntime ??
    workspaceRuntime
  );
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
