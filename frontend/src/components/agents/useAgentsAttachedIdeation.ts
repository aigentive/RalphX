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
        activeConversationMode === "tasks" ||
        activeConversationMode === "plan" ||
        Boolean(
          activeWorkspace?.taskPipelineSessionId ||
            activeWorkspace?.linkedIdeationSessionId ||
            activeWorkspace?.linkedPlanBranchId,
        )));
  const attachedIdeationSessionId = useMemo(
    () =>
      shouldHydrateAttachedIdeation
        ? resolveAttachedIdeationSessionId(
            activeConversation,
            selectedConversationMessages,
            activeWorkspace?.taskPipelineSessionId ??
              activeWorkspace?.linkedIdeationSessionId ??
              null,
          )
        : null,
    [
      activeConversation,
      activeWorkspace?.linkedIdeationSessionId,
      activeWorkspace?.taskPipelineSessionId,
      selectedConversationMessages,
      shouldHydrateAttachedIdeation,
    ],
  );
  const attachedIdeationSessionQuery = useQuery({
    queryKey: ideationKeys.sessionWithData(attachedIdeationSessionId ?? ""),
    queryFn: () => ideationApi.sessions.getWithData(attachedIdeationSessionId!),
    enabled: shouldHydrateAttachedIdeation && !!attachedIdeationSessionId,
    placeholderData: () => null,
    staleTime: 5_000,
  });
  const attachedIdeationSessionData =
    attachedIdeationSessionId &&
    attachedIdeationSessionQuery.data?.session.id === attachedIdeationSessionId
      ? attachedIdeationSessionQuery.data
      : null;
  const attachedIdeationSession = attachedIdeationSessionData?.session ?? null;
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
  const hasAttachedPlanArtifact = Boolean(
    attachedIdeationSession?.planArtifactId ||
      attachedIdeationSession?.inheritedPlanArtifactId,
  );
  const availableArtifactTabs = useMemo(() => {
    const hasExecutionTasks = Boolean(
      activeWorkspace?.linkedPlanBranchId ||
        attachedIdeationSession?.acceptanceStatus === "accepted" ||
        attachedIdeationSession?.convertedAt,
    );
    const hasVerificationEvidence = Boolean(
      attachedIdeationSession?.verificationInProgress ||
        (attachedIdeationSession?.verificationStatus ?? "unverified") !==
          "unverified" ||
        attachedIdeationSession?.gapScore != null,
    );
    const canStartPlan = Boolean(
      activeConversation?.contextType === "project" &&
        (activeWorkspace?.mode
          ? activeWorkspace.mode === "edit" || activeWorkspace.mode === "plan"
          : !activeConversationMode ||
            activeConversationMode === "edit" ||
            activeConversationMode === "plan"),
    );

    return getVisibleIdeationArtifactTabs({
      hasAttachedIdeationSession: Boolean(attachedIdeationSession),
      hasPlanArtifact: hasAttachedPlanArtifact,
      canStartPlan,
      hasVerificationEvidence,
      hasExecutionTasks,
    });
  }, [
    activeConversation?.contextType,
    activeConversationMode,
    activeWorkspace,
    attachedIdeationSession,
    hasAttachedPlanArtifact,
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
    void chatApi.archiveConversation(activeConversation.id, { closePullRequest: false })
      .then(() => {
        if (activeConversation.projectId) {
          return invalidateProjectConversations(activeConversation.projectId);
        }
        return undefined;
      })
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
    hasAttachedPlanArtifact,
    hasAutoOpenArtifacts,
  };
}
