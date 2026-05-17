import { useEffect, useMemo, useRef } from "react";
import { useQuery } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
  ChatMessageResponse,
} from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { ideationKeys } from "@/hooks/useIdeation";

import type { AgentConversation } from "./agentConversations";
import { getVisibleIdeationArtifactTabs } from "./agentArtifactTabs";
import { resolveAttachedIdeationSessionId } from "./attachedIdeationSession";

interface UseAgentsAttachedIdeationArgs {
  activeConversation: AgentConversation | null;
  activeConversationMode: AgentConversationWorkspaceMode | null;
  activeWorkspace: AgentConversationWorkspace | null;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
  selectedConversationMessages: ChatMessageResponse[];
}

export function useAgentsAttachedIdeation({
  activeConversation,
  activeConversationMode,
  activeWorkspace,
  invalidateProjectConversations,
  selectedConversationMessages,
}: UseAgentsAttachedIdeationArgs) {
  const childArchiveSyncRef = useRef<Set<string>>(new Set());
  const shouldHydrateAttachedIdeation =
    activeConversation?.contextType === "ideation" ||
    (activeConversation?.contextType === "project" &&
      (activeConversationMode === "ideation" ||
        Boolean(activeWorkspace?.linkedIdeationSessionId || activeWorkspace?.linkedPlanBranchId)));
  const attachedIdeationSessionId = useMemo(
    () =>
      shouldHydrateAttachedIdeation
        ? resolveAttachedIdeationSessionId(
            activeConversation,
            selectedConversationMessages,
            activeWorkspace?.linkedIdeationSessionId ?? null,
          )
        : null,
    [
      activeConversation,
      activeWorkspace?.linkedIdeationSessionId,
      selectedConversationMessages,
      shouldHydrateAttachedIdeation,
    ],
  );
  const attachedIdeationSessionQuery = useQuery({
    queryKey: ideationKeys.sessionDetail(attachedIdeationSessionId ?? ""),
    queryFn: () => ideationApi.sessions.get(attachedIdeationSessionId!),
    enabled: shouldHydrateAttachedIdeation && !!attachedIdeationSessionId,
    staleTime: 5_000,
  });
  const attachedIdeationSession =
    attachedIdeationSessionId &&
    attachedIdeationSessionQuery.data?.id === attachedIdeationSessionId
      ? attachedIdeationSessionQuery.data
      : null;
  const hasAutoOpenArtifacts = useMemo(() => {
    if (!attachedIdeationSession) {
      return false;
    }

    return Boolean(
      attachedIdeationSession.planArtifactId ||
        attachedIdeationSession.inheritedPlanArtifactId ||
        attachedIdeationSession.acceptanceStatus === "pending" ||
        attachedIdeationSession.verificationInProgress ||
        attachedIdeationSession.verificationStatus !== "unverified"
    );
  }, [attachedIdeationSession]);
  const availableArtifactTabs = useMemo(() => {
    const hasPlanArtifact = Boolean(
      attachedIdeationSession?.planArtifactId ||
        attachedIdeationSession?.inheritedPlanArtifactId,
    );
    const hasExecutionTasks = Boolean(
      activeWorkspace?.linkedPlanBranchId ||
        attachedIdeationSession?.acceptanceStatus === "accepted" ||
        attachedIdeationSession?.convertedAt,
    );

    return getVisibleIdeationArtifactTabs({
      hasAttachedIdeationSession: Boolean(attachedIdeationSession),
      hasPlanArtifact,
      hasExecutionTasks,
    });
  }, [activeWorkspace?.linkedPlanBranchId, attachedIdeationSession]);
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
