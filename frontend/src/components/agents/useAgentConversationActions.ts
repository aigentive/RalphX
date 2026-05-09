import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi } from "@/api/chat";
import { executionApi } from "@/api/execution";
import { ideationApi } from "@/api/ideation";
import { projectsApi } from "@/api/projects";
import { projectKeys } from "@/hooks/useProjects";
import type { Project } from "@/types/project";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { preflightAgentWorkspaceFreshness } from "./agentWorkspaceQueries";

interface UseAgentConversationActionsArgs {
  activeProjectId: string | null;
  clearAgentConversationSelection: () => void;
  clearAutoManagedTitle: (conversationId: string) => void;
  closeSidebarOverlay: () => void;
  findConversationById: (conversationId: string) => AgentConversation | null;
  focusedProjectId: string | null;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
  isSidebarOverlayOpen: boolean;
  projectId: string;
  projects: Project[];
  queryClient: QueryClient;
  selectConversation: (projectId: string, conversationId: string) => void;
  selectedConversationId: string | null;
  selectedProjectId: string | null;
  setActiveConversation: (storeKey: string, conversationId: string | null) => void;
  setFocusedProject: (projectId: string | null) => void;
  setOptimisticSelectedConversationId: Dispatch<SetStateAction<string | null>>;
}

export function useAgentConversationActions({
  activeProjectId,
  clearAgentConversationSelection,
  clearAutoManagedTitle,
  closeSidebarOverlay,
  findConversationById,
  focusedProjectId,
  invalidateProjectConversations,
  isSidebarOverlayOpen,
  projectId,
  projects,
  queryClient,
  selectConversation,
  selectedConversationId,
  selectedProjectId,
  setActiveConversation,
  setFocusedProject,
  setOptimisticSelectedConversationId,
}: UseAgentConversationActionsArgs) {
  const handleSelectConversation = useCallback(
    (conversationProjectId: string, conversation: AgentConversation) => {
      if (
        selectedProjectId === conversationProjectId &&
        selectedConversationId === conversation.id
      ) {
        clearAgentConversationSelection();
        return;
      }

      setOptimisticSelectedConversationId(conversation.id);
      selectConversation(conversationProjectId, conversation.id);
      setActiveConversation(
        getAgentConversationStoreKey(conversation),
        conversation.id
      );
      if (conversation.contextType === "project") {
        void preflightAgentWorkspaceFreshness(queryClient, conversation.id);
      }
    },
    [
      clearAgentConversationSelection,
      queryClient,
      selectConversation,
      selectedConversationId,
      selectedProjectId,
      setActiveConversation,
      setOptimisticSelectedConversationId,
    ]
  );

  const showStarterComposer = useCallback(
    (targetProjectId?: string | null) => {
      const nextProjectId =
        targetProjectId ??
        focusedProjectId ??
        selectedProjectId ??
        projectId ??
        projects[0]?.id ??
        null;
      if (nextProjectId) {
        setFocusedProject(nextProjectId);
      }
      clearAgentConversationSelection();
    },
    [
      clearAgentConversationSelection,
      focusedProjectId,
      projectId,
      projects,
      selectedProjectId,
      setFocusedProject,
    ]
  );

  const handleSidebarFocusProject = useCallback(
    (targetProjectId: string) => {
      setFocusedProject(targetProjectId);
      if (isSidebarOverlayOpen) {
        closeSidebarOverlay();
      }
    },
    [closeSidebarOverlay, isSidebarOverlayOpen, setFocusedProject]
  );

  const handleSidebarSelectConversation = useCallback(
    (conversationProjectId: string, conversation: AgentConversation) => {
      handleSelectConversation(conversationProjectId, conversation);
      if (isSidebarOverlayOpen) {
        closeSidebarOverlay();
      }
    },
    [closeSidebarOverlay, handleSelectConversation, isSidebarOverlayOpen]
  );

  const handleSidebarCreateAgent = useCallback(() => {
    showStarterComposer();
    if (isSidebarOverlayOpen) {
      closeSidebarOverlay();
    }
  }, [closeSidebarOverlay, isSidebarOverlayOpen, showStarterComposer]);

  const handleArchiveProject = useCallback(
    async (targetProjectId: string) => {
      try {
        try {
          await projectsApi.archive(targetProjectId);
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          if (!message.includes("currently active project")) {
            throw err;
          }
          await executionApi.setActiveProject(undefined);
          await projectsApi.archive(targetProjectId);
        }
        if (focusedProjectId === targetProjectId) {
          setFocusedProject(null);
        }
        if (selectedProjectId === targetProjectId) {
          clearAgentConversationSelection();
        }
        await queryClient.invalidateQueries({ queryKey: projectKeys.list() });
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to archive project");
      }
    },
    [
      clearAgentConversationSelection,
      focusedProjectId,
      queryClient,
      selectedProjectId,
      setFocusedProject,
    ]
  );

  const handleArchiveConversation = useCallback(
    async (conversation: AgentConversation) => {
      try {
        if (conversation.contextType === "ideation") {
          await ideationApi.sessions.archive(conversation.contextId);
        }
        await chatApi.archiveConversation(conversation.id);
        if (selectedConversationId === conversation.id) {
          clearAgentConversationSelection();
        }
        await invalidateProjectConversations(conversation.projectId);
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to archive session");
      }
    },
    [clearAgentConversationSelection, invalidateProjectConversations, selectedConversationId]
  );

  const handleRestoreConversation = useCallback(
    async (conversation: AgentConversation) => {
      try {
        if (conversation.contextType === "ideation") {
          await ideationApi.sessions.reopen(conversation.contextId);
        }
        await chatApi.restoreConversation(conversation.id);
        await invalidateProjectConversations(conversation.projectId);
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to restore session");
      }
    },
    [invalidateProjectConversations]
  );

  const handleRenameConversation = useCallback(
    async (conversationId: string, title: string) => {
      const trimmed = title.trim();
      if (!trimmed) {
        return;
      }
      const conversation = findConversationById(conversationId);
      if (conversation?.contextType === "ideation") {
        await Promise.all([
          chatApi.updateConversationTitle(conversationId, trimmed),
          ideationApi.sessions.updateTitle(conversation.contextId, trimmed),
        ]);
      } else {
        await chatApi.updateConversationTitle(conversationId, trimmed);
      }
      clearAutoManagedTitle(conversationId);
      await invalidateProjectConversations(conversation?.projectId ?? activeProjectId ?? projectId);
    },
    [
      activeProjectId,
      clearAutoManagedTitle,
      findConversationById,
      invalidateProjectConversations,
      projectId,
    ]
  );

  return {
    handleArchiveConversation,
    handleArchiveProject,
    handleRenameConversation,
    handleRestoreConversation,
    handleSidebarCreateAgent,
    handleSidebarFocusProject,
    handleSidebarSelectConversation,
  };
}
