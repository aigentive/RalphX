import type { AgentConversationWorkspace } from "@/api/chat";

type AutoPublishMutationInput = {
  autoPublishEnabled: boolean;
};

type PrSupervisionMutationInput = {
  autoFixEnabled: boolean;
  autoMergeDesired: boolean;
};

type ReviewAutomationMutationInput = {
  enabled: boolean | null;
};

export interface AgentsPublishAutomationSnapshot {
  conversationId: string;
  autoPublishEnabled: boolean;
  canRunPrSupervisionAutomation: boolean;
  isAutoPublishSaving: boolean;
  isPrSupervisionSaving: boolean;
  isReviewAutomationSaving: boolean;
  prAutofixEnabled: boolean;
  prAutoMergeCurrent: boolean | null;
  prAutoMergeDesired: boolean;
  prSupervisionStatus: string | null;
  reviewAutomationOverride: boolean | null;
}

export function deriveAgentsPublishAutomationSnapshot({
  workspace,
  hasPublishedPr,
  pendingAutoPublish = null,
  pendingPrSupervision = null,
  pendingReviewAutomation = null,
  settledPrSupervisionWorkspace = null,
  settledReviewAutomationWorkspace = null,
  isAutoPublishSaving = false,
  isPrSupervisionSaving = false,
  isReviewAutomationSaving = false,
}: {
  workspace: AgentConversationWorkspace;
  hasPublishedPr: boolean;
  pendingAutoPublish?: AutoPublishMutationInput | null;
  pendingPrSupervision?: PrSupervisionMutationInput | null;
  pendingReviewAutomation?: ReviewAutomationMutationInput | null;
  settledPrSupervisionWorkspace?: AgentConversationWorkspace | null;
  settledReviewAutomationWorkspace?: AgentConversationWorkspace | null;
  isAutoPublishSaving?: boolean;
  isPrSupervisionSaving?: boolean;
  isReviewAutomationSaving?: boolean;
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
  const reviewAutomationOverride = pendingReviewAutomation
    ? pendingReviewAutomation.enabled
    : settledReviewAutomationWorkspace
      ? settledReviewAutomationWorkspace.reviewAutomationOverride
      : workspace.reviewAutomationOverride;

  return {
    conversationId: workspace.conversationId,
    autoPublishEnabled,
    canRunPrSupervisionAutomation,
    isAutoPublishSaving,
    isPrSupervisionSaving,
    isReviewAutomationSaving,
    prAutofixEnabled,
    prAutoMergeCurrent,
    prAutoMergeDesired,
    prSupervisionStatus,
    reviewAutomationOverride,
  };
}

export function hasActiveAgentsPublishAutomation(
  snapshot: AgentsPublishAutomationSnapshot,
): boolean {
  return (
    snapshot.autoPublishEnabled ||
    snapshot.prAutofixEnabled ||
    snapshot.prAutoMergeDesired ||
    snapshot.reviewAutomationOverride === true
  );
}
