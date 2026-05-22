import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi, type AgentConversationWorkspace } from "@/api/chat";
import { executionApi } from "@/api/execution";
import { ideationApi } from "@/api/ideation";
import { chatKeys, invalidateConversationDataQueries } from "@/hooks/useChat";
import { projectsApi } from "@/api/projects";
import { projectKeys } from "@/hooks/useProjects";
import type { Project } from "@/types/project";

import {
  getAgentConversationStoreKey,
  toProjectAgentConversation,
  type AgentConversation,
} from "./agentConversations";
import {
  agentWorkspaceKeys,
  preflightAgentWorkspaceFreshness,
} from "./agentWorkspaceQueries";

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
  setOptimisticConversationsById: Dispatch<SetStateAction<Record<string, AgentConversation>>>;
  setOptimisticWorkspacesByConversationId: Dispatch<
    SetStateAction<Record<string, AgentConversationWorkspace>>
  >;
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
  setOptimisticConversationsById,
  setOptimisticWorkspacesByConversationId,
  setFocusedProject,
  setOptimisticSelectedConversationId,
}: UseAgentConversationActionsArgs) {
  const handleSelectConversation = useCallback(
    (conversationProjectId: string, conversation: AgentConversation) => {
      if (
        selectedProjectId === conversationProjectId &&
        selectedConversationId === conversation.id
      ) {
        return;
      }

      queryClient.setQueryData(chatKeys.conversationSummary(conversation.id), conversation);
      setOptimisticSelectedConversationId(conversation.id);
      setOptimisticConversationsById((current) =>
        current[conversation.id] === conversation
          ? current
          : { ...current, [conversation.id]: conversation }
      );
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
      queryClient,
      selectConversation,
      selectedConversationId,
      selectedProjectId,
      setActiveConversation,
      setOptimisticConversationsById,
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
      if (selectedConversationId === conversation.id) {
        showStarterComposer(conversationProjectId);
      } else {
        handleSelectConversation(conversationProjectId, conversation);
      }
      if (isSidebarOverlayOpen) {
        closeSidebarOverlay();
      }
    },
    [closeSidebarOverlay, handleSelectConversation, isSidebarOverlayOpen, selectedConversationId, showStarterComposer]
  );

  const handleSidebarCreateAgent = useCallback(() => {
    showStarterComposer();
    if (isSidebarOverlayOpen) {
      closeSidebarOverlay();
    }
  }, [closeSidebarOverlay, isSidebarOverlayOpen, showStarterComposer]);

  const handleForkConversation = useCallback(
    async (conversationId: string) => {
      try {
        const result = await chatApi.forkAgentConversation(conversationId);
        const conversation = toProjectAgentConversation(result.conversation);
        const conversationProjectId = conversation.projectId;

        queryClient.setQueryData(
          chatKeys.conversationSummary(conversation.id),
          result.conversation
        );
        queryClient.setQueryData(
          agentWorkspaceKeys.workspace(conversation.id),
          result.workspace
        );
        setOptimisticConversationsById((current) => ({
          ...current,
          [conversation.id]: conversation,
        }));
        if (result.workspace) {
          setOptimisticWorkspacesByConversationId((current) => ({
            ...current,
            [conversation.id]: result.workspace!,
          }));
        }
        setOptimisticSelectedConversationId(conversation.id);
        setFocusedProject(conversationProjectId);
        selectConversation(conversationProjectId, conversation.id);
        setActiveConversation(
          getAgentConversationStoreKey(conversation),
          conversation.id
        );
        invalidateConversationDataQueries(queryClient, conversation.id);
        void invalidateProjectConversations(conversationProjectId);
        return result;
      } catch (error) {
        toast.error("Failed to fork conversation", {
          description:
            error instanceof Error
              ? error.message
              : "The agent conversation could not be forked.",
          duration: 10000,
        });
        throw error;
      }
    },
    [
      invalidateProjectConversations,
      queryClient,
      selectConversation,
      setActiveConversation,
      setFocusedProject,
      setOptimisticConversationsById,
      setOptimisticSelectedConversationId,
      setOptimisticWorkspacesByConversationId,
    ]
  );

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
    handleForkConversation,
    handleSidebarCreateAgent,
    handleSidebarFocusProject,
    handleSidebarSelectConversation,
  };
}
