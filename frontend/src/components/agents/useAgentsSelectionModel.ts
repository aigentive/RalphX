import { useEffect, useMemo } from "react";

import { useConversationSummary } from "@/hooks/useChat";
import type { Project } from "@/types/project";

import {
  toProjectAgentConversation,
  type AgentConversation,
} from "./agentConversations";
import { useProjectAgentConversations } from "./useProjectAgentConversations";

interface UseAgentsSelectionModelArgs {
  clearAgentConversationSelection: () => void;
  focusedProjectId: string | null;
  optimisticConversationsById: Record<string, AgentConversation>;
  optimisticSelectedConversationId: string | null;
  projectId: string;
  projects: Project[];
  selectedProjectId: string | null;
  showArchived: boolean;
  storedSelectedConversationId: string | null;
}

export function useAgentsSelectionModel({
  clearAgentConversationSelection,
  focusedProjectId,
  optimisticConversationsById,
  optimisticSelectedConversationId,
  projectId,
  projects,
  selectedProjectId,
  showArchived,
  storedSelectedConversationId,
}: UseAgentsSelectionModelArgs) {
  const selectedConversationId = storedSelectedConversationId ?? optimisticSelectedConversationId;
  const defaultProjectId = focusedProjectId || selectedProjectId || projectId || projects[0]?.id || null;
  const activeProjectId = selectedProjectId || defaultProjectId;
  const focusedConversations = useProjectAgentConversations(activeProjectId, showArchived);
  const selectedConversationQuery = useConversationSummary(selectedConversationId, {
    enabled: !!selectedConversationId,
  });
  const selectedConversationData = selectedConversationQuery.data;
  const selectedConversationFallback = useMemo(() => {
    const conversation = selectedConversationData;
    const isArchivedConversation = Boolean(conversation?.archivedAt);
    if (conversation) {
      const isStandaloneConversation =
        conversation.contextType === "standalone" &&
        conversation.contextId === conversation.id;
      if (
        conversation.id !== selectedConversationId ||
        (!isStandaloneConversation &&
          (conversation.contextType !== "project" ||
            conversation.contextId !== activeProjectId)) ||
        (showArchived ? !isArchivedConversation : isArchivedConversation)
      ) {
        return null;
      }

      return toProjectAgentConversation(conversation);
    }

    const optimisticConversation = selectedConversationId
      ? optimisticConversationsById[selectedConversationId]
      : null;
    if (
      !optimisticConversation ||
      (optimisticConversation.contextType !== "standalone" &&
        (optimisticConversation.contextType !== "project" ||
          optimisticConversation.contextId !== activeProjectId)) ||
      (showArchived
        ? !optimisticConversation.archivedAt
        : Boolean(optimisticConversation.archivedAt))
    ) {
      return null;
    }

    return optimisticConversation;
  }, [
    activeProjectId,
    optimisticConversationsById,
    selectedConversationData,
    selectedConversationId,
    showArchived,
  ]);

  const activeConversation = useMemo(() => {
    if (!selectedConversationId) {
      return null;
    }
    return (
      focusedConversations.data?.find(
        (conversation) => conversation.id === selectedConversationId
      ) ?? selectedConversationFallback
    );
  }, [
    focusedConversations.data,
    selectedConversationFallback,
    selectedConversationId,
  ]);
  const selectedConversationMessages = useMemo(
    () => [],
    [],
  );
  useEffect(() => {
    if (
      !selectedConversationId ||
      focusedConversations.isLoading ||
      selectedConversationQuery.isLoading
    ) {
      return;
    }
    const selectedStillExists = focusedConversations.data?.some(
      (conversation) => conversation.id === selectedConversationId
    );
    if (selectedStillExists === false && !selectedConversationFallback) {
      clearAgentConversationSelection();
    }
  }, [
    activeProjectId,
    clearAgentConversationSelection,
    focusedConversations.data,
    focusedConversations.isLoading,
    selectedConversationFallback,
    selectedConversationQuery.isLoading,
    selectedConversationId,
  ]);
  return {
    activeConversation,
    activeProjectId:
      activeConversation?.contextType === "standalone" ? null : activeProjectId,
    defaultProjectId,
    focusedConversations,
    selectedConversationFallback,
    selectedConversationId,
    selectedConversationMessages,
  };
}
