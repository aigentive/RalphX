import type {
  AgentConversationWorkspace,
  AgentSidebarConversationRow,
} from "@/api/chat";

import {
  toProjectAgentConversation,
  type AgentConversation,
} from "./agentConversations";

export interface BulkArchiveConversationTarget {
  conversation: AgentConversation;
  workspace: AgentConversationWorkspace | null;
}

export interface BulkArchiveConversationsResult {
  archivedConversationIds: string[];
  failedConversationIds: string[];
  cleanupPendingConversationIds: string[];
  cleanupUnsafeConversationIds: string[];
}

export type BulkArchiveConversationHandler = (
  targets: BulkArchiveConversationTarget[]
) => Promise<BulkArchiveConversationsResult>;

export function hasPotentialOpenPullRequest(
  workspace: AgentConversationWorkspace | null
): boolean {
  if (!workspace) {
    return false;
  }
  if (workspace.linkedPlanBranchId) {
    return true;
  }
  if (workspace.publicationPrNumber == null) {
    return false;
  }
  const status = workspace.publicationPrStatus?.trim().toLowerCase();
  return status !== "closed" && status !== "merged";
}

export function isBulkArchiveConversationEligible(
  target: BulkArchiveConversationTarget
): boolean {
  return !target.conversation.archivedAt;
}

export function toBulkArchiveConversationTarget(
  row: AgentSidebarConversationRow
): BulkArchiveConversationTarget {
  return {
    conversation: toProjectAgentConversation(row.conversation),
    workspace: row.workspace,
  };
}
