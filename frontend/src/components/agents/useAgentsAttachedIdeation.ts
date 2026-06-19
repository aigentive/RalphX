import { useEffect, useMemo, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
  ChatMessageResponse,
} from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { useConversationHistoryWindow } from "@/hooks/useChat";
import { ideationKeys } from "@/hooks/useIdeation";

import type { AgentConversation } from "./agentConversations";
import { getVisibleIdeationArtifactTabs } from "./agentArtifactTabs";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";
import { resolveAttachedIdeationSessionId } from "./attachedIdeationSession";

interface UseAgentsAttachedIdeationArgs {
  activeConversation: AgentConversation | null;
  activeConversationMode: AgentConversationWorkspaceMode | null;
  activeWorkspace: AgentConversationWorkspace | null;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
  selectedConversationMessages: ChatMessageResponse[];
}

function compareMessagesByCreatedAt(
  left: ChatMessageResponse,
  right: ChatMessageResponse,
): number {
  const leftTime = Date.parse(left.createdAt);
  const rightTime = Date.parse(right.createdAt);
  if (Number.isFinite(leftTime) && Number.isFinite(rightTime)) {
    return leftTime - rightTime;
  }
  return 0;
}

export function useAgentsAttachedIdeation({
  activeConversation,
  activeConversationMode,
  activeWorkspace,
  invalidateProjectConversations,
  selectedConversationMessages,
}: UseAgentsAttachedIdeationArgs) {
  const queryClient = useQueryClient();
  const childArchiveSyncRef = useRef<Set<string>>(new Set());
  const syncedIdeationLinksRef = useRef<Set<string>>(new Set());
  const shouldHydrateAttachedIdeation =
    activeConversation?.contextType === "ideation" ||
    (activeConversation?.contextType === "project" &&
      (activeConversationMode === "ideation" ||
        activeConversationMode === "plan" ||
        Boolean(activeWorkspace?.linkedIdeationSessionId || activeWorkspace?.linkedPlanBranchId)));
  const shouldLoadConversationHistory =
    shouldHydrateAttachedIdeation &&
    activeConversation?.contextType === "project";
  const conversationHistoryQuery = useConversationHistoryWindow(
    activeConversation?.id ?? null,
    {
      enabled: shouldLoadConversationHistory,
      pageSize: 40,
    },
  );
  const resolvedConversationMessages = useMemo(() => {
    const historyData = conversationHistoryQuery.data;
    if (!historyData || historyData.conversation?.id !== activeConversation?.id) {
      return selectedConversationMessages;
    }
    const byId = new Map<string, ChatMessageResponse>();
    for (const message of selectedConversationMessages) {
      byId.set(message.id, message);
    }
    for (const message of historyData.messages) {
      byId.set(message.id, message);
    }
    return [...byId.values()].sort(compareMessagesByCreatedAt);
  }, [
    activeConversation?.id,
    conversationHistoryQuery.data,
    selectedConversationMessages,
  ]);
  const attachedIdeationSessionId = useMemo(
    () =>
      shouldHydrateAttachedIdeation
        ? resolveAttachedIdeationSessionId(
            activeConversation,
            resolvedConversationMessages,
            activeWorkspace?.linkedIdeationSessionId ?? null,
          )
        : null,
    [
      activeConversation,
      activeWorkspace?.linkedIdeationSessionId,
      resolvedConversationMessages,
      shouldHydrateAttachedIdeation,
    ],
  );
  const attachedIdeationSessionQuery = useQuery({
    queryKey: ideationKeys.sessionWithData(attachedIdeationSessionId ?? ""),
    queryFn: () => ideationApi.sessions.getWithData(attachedIdeationSessionId!),
    enabled: shouldHydrateAttachedIdeation && !!attachedIdeationSessionId,
    staleTime: 0,
    refetchInterval: (query) =>
      query.state.data?.session.verificationInProgress ||
      query.state.data?.session.acceptanceStatus === "pending"
        ? 3_000
        : false,
  });
  const attachedIdeationSessionData =
    attachedIdeationSessionId &&
    attachedIdeationSessionQuery.data?.session.id === attachedIdeationSessionId
      ? attachedIdeationSessionQuery.data
      : null;
  const attachedIdeationSession =
    attachedIdeationSessionId &&
    attachedIdeationSessionData?.session.id === attachedIdeationSessionId
      ? attachedIdeationSessionData.session
      : null;
  const hasCreatedTasks = Boolean(
    attachedIdeationSessionData?.proposals.some(
      (proposal) => proposal.createdTaskId != null,
    ),
  );
  const hasExecutionTasks = Boolean(
    activeWorkspace?.linkedPlanBranchId ||
      hasCreatedTasks ||
      attachedIdeationSession?.status === "accepted" ||
      attachedIdeationSession?.acceptanceStatus === "accepted" ||
      attachedIdeationSession?.convertedAt,
  );
  const hasAutoOpenArtifacts = useMemo(() => {
    if (!attachedIdeationSession) {
      return false;
    }

    return Boolean(
      attachedIdeationSession.planArtifactId ||
        attachedIdeationSession.inheritedPlanArtifactId ||
        attachedIdeationSession.acceptanceStatus === "pending" ||
        hasExecutionTasks ||
        attachedIdeationSession.verificationInProgress ||
        attachedIdeationSession.verificationStatus !== "unverified"
    );
  }, [attachedIdeationSession, hasExecutionTasks]);
  const availableArtifactTabs = useMemo(() => {
    const hasPlanArtifact = Boolean(
      attachedIdeationSession?.planArtifactId ||
        attachedIdeationSession?.inheritedPlanArtifactId,
    );
    return getVisibleIdeationArtifactTabs({
      hasAttachedIdeationSession: Boolean(attachedIdeationSession),
      hasPlanArtifact,
      hasVerificationArtifacts: Boolean(
        attachedIdeationSession?.verificationInProgress ||
          attachedIdeationSession?.verificationStatus !== "unverified" ||
          attachedIdeationSession?.gapScore != null,
      ),
      hasExecutionTasks,
    });
  }, [attachedIdeationSession, hasExecutionTasks]);
  const shouldSyncWorkspaceIdeationLink = Boolean(
    activeConversation?.id &&
      activeConversation.contextType === "project" &&
      activeWorkspace &&
      (activeWorkspace.mode === "ideation" || activeWorkspace.mode === "plan") &&
      attachedIdeationSessionId &&
      attachedIdeationSession?.id === attachedIdeationSessionId &&
      (activeWorkspace.linkedIdeationSessionId !== attachedIdeationSessionId ||
        (!activeWorkspace.linkedPlanBranchId && hasExecutionTasks)),
  );
  useEffect(() => {
    if (
      !shouldSyncWorkspaceIdeationLink ||
      !activeConversation?.id ||
      !attachedIdeationSessionId
    ) {
      return;
    }

    const syncKey = `${activeConversation.id}:${attachedIdeationSessionId}:${activeWorkspace?.linkedPlanBranchId ?? "missing-branch"}`;
    if (syncedIdeationLinksRef.current.has(syncKey)) {
      return;
    }
    syncedIdeationLinksRef.current.add(syncKey);
    void chatApi
      .syncAgentConversationWorkspaceIdeationLink(
        activeConversation.id,
        attachedIdeationSessionId,
      )
      .then((result) => {
        queryClient.setQueryData(
          agentWorkspaceKeys.workspace(activeConversation.id),
          result.workspace,
        );
        return invalidateWorkspaceQueries(queryClient, activeConversation.id);
      })
      .catch(() => {
        syncedIdeationLinksRef.current.delete(syncKey);
      });
  }, [
    activeConversation?.id,
    activeWorkspace?.linkedPlanBranchId,
    attachedIdeationSession?.id,
    attachedIdeationSessionId,
    hasExecutionTasks,
    queryClient,
    shouldSyncWorkspaceIdeationLink,
  ]);
  useEffect(() => {
    if (
      activeConversation?.contextType !== "project" ||
      !attachedIdeationSession ||
      activeConversation.archivedAt ||
      childArchiveSyncRef.current.has(activeConversation.id)
    ) {
      return;
    }
    const sessionArchived =
      attachedIdeationSession.status === "archived" ||
      Boolean(attachedIdeationSession.archivedAt);
    if (!sessionArchived) {
      return;
    }
    childArchiveSyncRef.current.add(activeConversation.id);
    void chatApi.archiveConversation(activeConversation.id)
      .then(() => invalidateProjectConversations(activeConversation.projectId))
      .catch(() => {
        childArchiveSyncRef.current.delete(activeConversation.id);
        // Status sync is best-effort; manual archive remains available.
      });
  }, [
    activeConversation,
    attachedIdeationSession,
    invalidateProjectConversations,
  ]);
  return {
    attachedIdeationSessionData: attachedIdeationSession,
    attachedIdeationSessionId,
    availableArtifactTabs,
    hasAutoOpenArtifacts,
  };
}
