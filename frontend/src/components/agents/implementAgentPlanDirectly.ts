import type { QueryClient } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceMode,
  type ComposerArtifactReference,
  type SendAgentMessageOptions,
  type SendAgentMessageResult,
} from "@/api/chat";

import { PLAN_IMPLEMENT_DIRECTLY_REQUEST } from "./agentPlanModeActions";
import {
  PlanContinuationCommittedError,
  refreshTransitionedAgentWorkspace,
} from "./agentPlanProposalActivation";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";

type DirectImplementationSendOptions = Omit<
  SendAgentMessageOptions,
  | "composerArtifactReferences"
  | "conversationId"
  | "expectedLinkedPlanFingerprint"
  | "requireApprovedLinkedPlan"
  | "suppressUserMessage"
>;

interface ImplementAgentPlanDirectlyParams {
  projectId: string;
  workspace: AgentConversationWorkspace;
  queryClient: QueryClient;
  onConversationModeSwitched?: (
    conversationId: string,
    mode: AgentConversationWorkspaceMode,
    workspace: AgentConversationWorkspace | null,
  ) => void;
  sendOptions?: DirectImplementationSendOptions;
  pinnedActivation?: DirectImplementationActivationSnapshot;
  onActivated?: (snapshot: DirectImplementationActivationSnapshot) => void;
}

export interface DirectImplementationActivationSnapshot {
  workspace: AgentConversationWorkspace;
  artifactReferences: ComposerArtifactReference[];
  planContextFingerprint: string;
}

export async function implementAgentPlanDirectly({
  projectId,
  workspace,
  queryClient,
  onConversationModeSwitched,
  sendOptions,
  pinnedActivation,
  onActivated,
}: ImplementAgentPlanDirectlyParams): Promise<SendAgentMessageResult> {
  const conversationId = workspace.conversationId;

  if (!workspace.linkedIdeationSessionId) {
    throw new Error("Plan workspace is missing its linked planning session");
  }
  let activation: DirectImplementationActivationSnapshot;
  try {
    activation = await chatApi.activateAgentPlanDirectImplementation({
      conversationId,
      sessionId: workspace.linkedIdeationSessionId,
      retry: pinnedActivation !== undefined || workspace.mode === "edit",
    });
  } catch (error) {
    if (!pinnedActivation) throw error;
    throw committedImplementationError(error);
  }
  queryClient.setQueryData(
    agentWorkspaceKeys.workspace(conversationId),
    activation.workspace,
  );
  onConversationModeSwitched?.(
    conversationId,
    "edit",
    activation.workspace,
  );
  void invalidateWorkspaceQueries(queryClient, conversationId);

  if (
    pinnedActivation &&
    !sameActivationSnapshot(pinnedActivation, activation)
  ) {
    throw new PlanContinuationCommittedError(
      "Edit mode is active, but the approved plan changed after direct implementation activation. Return to Plan mode, review the current bundle, and approve it again before retrying.",
    );
  }
  const committed = pinnedActivation ?? cloneActivationSnapshot(activation);
  onActivated?.(committed);

  try {
    return await chatApi.sendAgentMessage(
      "project",
      projectId,
      PLAN_IMPLEMENT_DIRECTLY_REQUEST,
      undefined,
      {
        conversationId,
        ...sendOptions,
        requireApprovedLinkedPlan: true,
        expectedLinkedPlanFingerprint: committed.planContextFingerprint,
        suppressUserMessage: true,
      },
    );
  } catch (error) {
    await refreshTransitionedAgentWorkspace({
      queryClient,
      conversationId,
      ...(onConversationModeSwitched ? { onConversationModeSwitched } : {}),
    });
    throw committedImplementationError(error);
  }
}

function cloneActivationSnapshot(
  activation: DirectImplementationActivationSnapshot,
): DirectImplementationActivationSnapshot {
  return {
    workspace: { ...activation.workspace },
    artifactReferences: activation.artifactReferences.map((reference) => ({
      ...reference,
    })),
    planContextFingerprint: activation.planContextFingerprint,
  };
}

function sameActivationSnapshot(
  pinned: DirectImplementationActivationSnapshot,
  current: DirectImplementationActivationSnapshot,
): boolean {
  if (
    pinned.workspace.conversationId !== current.workspace.conversationId ||
    pinned.workspace.projectId !== current.workspace.projectId ||
    pinned.workspace.mode !== "edit" ||
    current.workspace.mode !== "edit" ||
    pinned.workspace.linkedIdeationSessionId !==
      current.workspace.linkedIdeationSessionId ||
    pinned.planContextFingerprint !== current.planContextFingerprint ||
    pinned.artifactReferences.length !== current.artifactReferences.length
  ) {
    return false;
  }
  return pinned.artifactReferences.every((reference, index) => {
    const candidate = current.artifactReferences[index];
    return (
      candidate !== undefined &&
      reference.artifactId === candidate.artifactId &&
      reference.kind === candidate.kind &&
      reference.sessionId === candidate.sessionId &&
      reference.version === candidate.version
    );
  });
}

function committedImplementationError(error: unknown) {
  if (error instanceof PlanContinuationCommittedError) return error;
  const detail = error instanceof Error ? ` ${error.message}` : "";
  return new PlanContinuationCommittedError(
    `Edit mode is active, but implementation launch failed. Retry will revalidate and resend only the originally approved plan; it will not switch modes again.${detail}`,
  );
}
