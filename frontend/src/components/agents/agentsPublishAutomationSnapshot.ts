import type { AgentConversationWorkspace } from "@/api/chat";

type AutoPublishMutationInput = {
  autoPublishEnabled: boolean;
};

type PrSupervisionMutationInput = {
  autoFixEnabled: boolean;
  autoMergeDesired: boolean;
};

export interface AgentsPublishAutomationSnapshot {
  conversationId: string;
  autoPublishEnabled: boolean;
  canRunPrSupervisionAutomation: boolean;
  isAutoPublishSaving: boolean;
  isPrSupervisionSaving: boolean;
  prAutofixEnabled: boolean;
  prAutoMergeCurrent: boolean | null;
  prAutoMergeDesired: boolean;
  prSupervisionStatus: string | null;
}

export function deriveAgentsPublishAutomationSnapshot({
  workspace,
  hasPublishedPr,
  pendingAutoPublish = null,
  pendingPrSupervision = null,
  settledPrSupervisionWorkspace = null,
  isAutoPublishSaving = false,
  isPrSupervisionSaving = false,
}: {
  workspace: AgentConversationWorkspace;
  hasPublishedPr: boolean;
  pendingAutoPublish?: AutoPublishMutationInput | null;
  pendingPrSupervision?: PrSupervisionMutationInput | null;
  settledPrSupervisionWorkspace?: AgentConversationWorkspace | null;
  isAutoPublishSaving?: boolean;
  isPrSupervisionSaving?: boolean;
}): AgentsPublishAutomationSnapshot {
  const storedAutoPublishEnabled = workspace.autoPublishEnabled ?? true;
  const initialAutoPublishEnabled =
    workspace.autoPublishInitialPrEnabled ?? false;
  const autoPublishEnabled =
    pendingAutoPublish?.autoPublishEnabled ??
    (hasPublishedPr ? storedAutoPublishEnabled : initialAutoPublishEnabled);
  const canRunPrSupervisionAutomation = hasPublishedPr
    ? autoPublishEnabled
    : storedAutoPublishEnabled;
  const prAutofixEnabled =
    pendingPrSupervision?.autoFixEnabled ??
    settledPrSupervisionWorkspace?.prAutofixEnabled ??
    workspace.prAutofixEnabled ??
    false;
  const prAutoMergeDesired =
    pendingPrSupervision?.autoMergeDesired ??
    settledPrSupervisionWorkspace?.prAutoMergeDesired ??
    workspace.prAutoMergeDesired ??
    false;
  const prAutoMergeCurrent =
    settledPrSupervisionWorkspace?.prAutoMergeCurrent ??
    workspace.prAutoMergeCurrent ??
    null;
  const prSupervisionStatus =
    settledPrSupervisionWorkspace?.prSupervisionStatus ??
    workspace.prSupervisionStatus ??
    null;

  return {
    conversationId: workspace.conversationId,
    autoPublishEnabled,
    canRunPrSupervisionAutomation,
    isAutoPublishSaving,
    isPrSupervisionSaving,
    prAutofixEnabled,
    prAutoMergeCurrent,
    prAutoMergeDesired,
    prSupervisionStatus,
  };
}
