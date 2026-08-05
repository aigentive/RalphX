import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useAgentGate } from "@/hooks/useAgentGate";

import {
  chatApi,
  setAgentConversationMuted,
  type AgentConversationWorkspace,
  type ChatMessageResponse,
} from "@/api/chat";
import { executionApi } from "@/api/execution";
import { ideationApi } from "@/api/ideation";
import { chatKeys, invalidateConversationDataQueries } from "@/hooks/useChat";
import { projectsApi } from "@/api/projects";
import { projectKeys } from "@/hooks/useProjects";
import { agentSidebarConversationKeys } from "@/hooks/agentSidebarConversationKeys";
import type { Project } from "@/types/project";
import type {
  AgentRuntimeSelection,
  AgentStartConversationDraft,
} from "@/stores/agentSessionStore";

import {
  getAgentConversationStoreKey,
  toProjectAgentConversation,
  type AgentConversation,
  type AgentConversationArchiveOptions,
} from "./agentConversations";
import { runtimeFromConversation } from "./agentConversationRuntime";
import {
  agentWorkspaceKeys,
  preflightAgentWorkspaceFreshness,
} from "./agentWorkspaceQueries";
import {
  isBulkArchiveConversationEligible,
  type BulkArchiveConversationTarget,
  type BulkArchiveConversationsResult,
} from "./bulkConversationArchive";

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
  selectConversation: (projectId: string | null, conversationId: string) => void;
  selectedConversationId: string | null;
  selectedProjectId: string | null;
  setActiveConversation: (storeKey: string, conversationId: string | null) => void;
  setOptimisticConversationsById: Dispatch<SetStateAction<Record<string, AgentConversation>>>;
  setOptimisticWorkspacesByConversationId: Dispatch<
    SetStateAction<Record<string, AgentConversationWorkspace>>
  >;
  setFocusedProject: (projectId: string | null) => void;
  setStartConversationDraft: (draft: AgentStartConversationDraft) => void;
  setOptimisticSelectedConversationId: Dispatch<SetStateAction<string | null>>;
  setRuntimeForConversation: (
    conversationId: string,
    projectId: string | null,
    runtime: AgentRuntimeSelection
  ) => void;
}

function getFirstUserMessageContent(messages: ChatMessageResponse[]): string | null {
  for (const message of messages) {
    if (message.role !== "user") {
      continue;
    }
    const content = message.content.trim();
    if (content.length > 0) {
      return content;
    }
  }
  return null;
}

async function archiveAgentConversation(
  conversation: AgentConversation,
  options: AgentConversationArchiveOptions
) {
  if (conversation.contextType === "ideation") {
    await ideationApi.sessions.archive(conversation.contextId);
  }
  return chatApi.archiveConversation(conversation.id, options);
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
  setStartConversationDraft,
  setOptimisticSelectedConversationId,
  setRuntimeForConversation,
}: UseAgentConversationActionsArgs) {
  const forkGate = useAgentGate("conversationFork");
  const archiveGate = useAgentGate("conversationArchive");
  const unarchiveGate = useAgentGate("conversationUnarchive");
  const invalidateConversationLists = useCallback(
    async (conversationProjectId: string | null) => {
      if (conversationProjectId) {
        await invalidateProjectConversations(conversationProjectId);
        return;
      }
      await queryClient.invalidateQueries({
        queryKey: agentSidebarConversationKeys.all,
      });
    },
    [invalidateProjectConversations, queryClient],
  );
  const handleSelectConversation = useCallback(
    (conversationProjectId: string | null, conversation: AgentConversation) => {
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
    (conversationProjectId: string | null, conversation: AgentConversation) => {
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

  const handleStartPersonaBuilder = useCallback(
    (conversation: AgentConversation) => {
      if (conversation.contextType !== "project" || !conversation.projectId) {
        return;
      }
      setStartConversationDraft({
        projectId: conversation.projectId,
        projectLocked: true,
        mode: "persona_builder",
      });
      showStarterComposer(conversation.projectId);
    },
    [setStartConversationDraft, showStarterComposer],
  );

  const handleForkConversation = useCallback(
    async (conversationId: string) => {
      if (forkGate.gated) return Promise.reject(new Error(forkGate.reason ?? "Fork unavailable"));
      try {
        const result = await chatApi.forkAgentConversation(conversationId);
        const conversation = toProjectAgentConversation(result.conversation);
        const conversationProjectId = conversation.projectId;
        const forkRuntime = runtimeFromConversation(conversation);

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
        if (forkRuntime) {
          setRuntimeForConversation(
            conversation.id,
            conversationProjectId,
            forkRuntime
          );
        }
        selectConversation(conversationProjectId, conversation.id);
        setActiveConversation(
          getAgentConversationStoreKey(conversation),
          conversation.id
        );
        invalidateConversationDataQueries(queryClient, conversation.id);
        void invalidateConversationLists(conversationProjectId);
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
      invalidateConversationLists,
      forkGate.gated,
      forkGate.reason,
      queryClient,
      selectConversation,
      setActiveConversation,
      setFocusedProject,
      setOptimisticConversationsById,
      setOptimisticSelectedConversationId,
      setOptimisticWorkspacesByConversationId,
      setRuntimeForConversation,
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
    async (
      conversation: AgentConversation,
      options: AgentConversationArchiveOptions
    ) => {
      if (archiveGate.gated) return;
      try {
        const result = await archiveAgentConversation(conversation, options);
        if (selectedConversationId === conversation.id) {
          clearAgentConversationSelection();
        }
        await invalidateConversationLists(conversation.projectId);
        if (result.cleanup.localCleanup === "failed_unsafe") {
          toast.warning(
            "Session archived, but RalphX refused unsafe local workspace cleanup. Review the workspace metadata before retrying."
          );
        } else if (
          result.cleanup.localCleanup === "failed_operational" ||
          result.cleanup.localCleanup === "pending"
        ) {
          toast.warning(
            "Session archived. Local workspace cleanup is pending and will retry automatically."
          );
        }
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to archive session");
      }
    },
    [archiveGate.gated, clearAgentConversationSelection, invalidateConversationLists, selectedConversationId]
  );

  const handleBulkArchiveConversations = useCallback(
    async (
      targets: BulkArchiveConversationTarget[]
    ): Promise<BulkArchiveConversationsResult> => {
      const archivedConversationIds: string[] = [];
      const failedConversationIds: string[] = [];
      const cleanupPendingConversationIds: string[] = [];
      const cleanupUnsafeConversationIds: string[] = [];
      const failureDetails: string[] = [];
      const affectedProjectIds = new Set<string | null>();

      for (const target of targets) {
        const { conversation } = target;
        if (!isBulkArchiveConversationEligible(target)) {
          failedConversationIds.push(conversation.id);
          failureDetails.push(`${conversation.title || "Untitled agent"}: Already archived`);
          continue;
        }

        affectedProjectIds.add(conversation.projectId);
        try {
          const result = await archiveAgentConversation(conversation, {
            closePullRequest: false,
          });
          archivedConversationIds.push(conversation.id);
          if (result.cleanup.localCleanup === "failed_unsafe") {
            cleanupUnsafeConversationIds.push(conversation.id);
          } else if (
            result.cleanup.localCleanup === "failed_operational" ||
            result.cleanup.localCleanup === "pending"
          ) {
            cleanupPendingConversationIds.push(conversation.id);
          }
        } catch (error) {
          failedConversationIds.push(conversation.id);
          failureDetails.push(
            `${conversation.title || "Untitled agent"}: ${
              error instanceof Error ? error.message : "Archive failed"
            }`
          );
        }
      }

      if (
        selectedConversationId !== null &&
        archivedConversationIds.includes(selectedConversationId)
      ) {
        clearAgentConversationSelection();
      }
      await Promise.all(
        Array.from(affectedProjectIds, (targetProjectId) =>
          invalidateConversationLists(targetProjectId)
        )
      );

      if (archivedConversationIds.length > 0) {
        toast.success(
          `Archived ${archivedConversationIds.length} ${
            archivedConversationIds.length === 1 ? "session" : "sessions"
          }`
        );
      }
      if (failedConversationIds.length > 0) {
        toast.error(
          `Failed to archive ${failedConversationIds.length} ${
            failedConversationIds.length === 1 ? "session" : "sessions"
          }`,
          {
            description: failureDetails.join("\n"),
            duration: 10000,
          }
        );
      }

      if (cleanupPendingConversationIds.length > 0) {
        toast.warning(
          `Local cleanup is pending automatic retry for ${cleanupPendingConversationIds.length} ${
            cleanupPendingConversationIds.length === 1 ? "session" : "sessions"
          }.`
        );
      }
      if (cleanupUnsafeConversationIds.length > 0) {
        toast.warning(
          `RalphX refused unsafe local cleanup for ${cleanupUnsafeConversationIds.length} ${
            cleanupUnsafeConversationIds.length === 1 ? "session" : "sessions"
          }.`
        );
      }

      return {
        archivedConversationIds,
        failedConversationIds,
        cleanupPendingConversationIds,
        cleanupUnsafeConversationIds,
      };
    },
    [
      clearAgentConversationSelection,
      invalidateConversationLists,
      selectedConversationId,
    ]
  );

  const handleRestoreConversation = useCallback(
    async (conversation: AgentConversation) => {
      if (unarchiveGate.gated) {
        toast.error(unarchiveGate.reason);
        return;
      }
      try {
        if (conversation.contextType === "ideation") {
          await ideationApi.sessions.reopen(conversation.contextId);
        }
        await chatApi.restoreConversation(conversation.id);
        await invalidateConversationLists(conversation.projectId);
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to restore session");
      }
    },
    [invalidateConversationLists, unarchiveGate.gated, unarchiveGate.reason]
  );

  // Undo re-enters the same write, so the write lives in its own callback
  // rather than the toast reaching back into the handler that opened it.
  const writeConversationMuted = useCallback(
    async (conversationId: string, muted: boolean) => {
      await setAgentConversationMuted(conversationId, muted);
      await queryClient.invalidateQueries({
        queryKey: agentSidebarConversationKeys.all,
      });
    },
    [queryClient],
  );

  const handleSetConversationMuted = useCallback(
    async (conversation: AgentConversation, muted: boolean) => {
      const title = conversation.title || "Untitled agent";
      try {
        await writeConversationMuted(conversation.id, muted);
        if (muted) {
          toast.success(`Muted "${title}" — comes back on its next change`, {
            action: {
              label: "Undo",
              onClick: () => {
                void writeConversationMuted(conversation.id, false).catch((err: unknown) => {
                  toast.error(
                    err instanceof Error ? err.message : "Failed to update session mute"
                  );
                });
              },
            },
          });
        }
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to update session mute");
      }
    },
    [writeConversationMuted],
  );

  const handleBulkMuteConversations = useCallback(
    async (conversationIds: string[]) => {
      try {
        await Promise.all(
          conversationIds.map((conversationId) => setAgentConversationMuted(conversationId, true))
        );
        await queryClient.invalidateQueries({
          queryKey: agentSidebarConversationKeys.all,
        });
        const mutedLabel =
          conversationIds.length === 1
            ? "Muted 1 session — it returns on its next change"
            : `Muted ${conversationIds.length} sessions — each returns on its next change`;
        toast.success(mutedLabel, {
          action: {
            label: "Undo",
            onClick: () => {
              void Promise.all(
                conversationIds.map((conversationId) => setAgentConversationMuted(conversationId, false))
              )
                .then(() =>
                  queryClient.invalidateQueries({ queryKey: agentSidebarConversationKeys.all })
                )
                .catch((err: unknown) => {
                  toast.error(
                    err instanceof Error ? err.message : "Failed to update session mute"
                  );
                });
            },
          },
        });
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to mute sessions");
      }
    },
    [queryClient],
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
      await invalidateConversationLists(
        conversation?.projectId ?? activeProjectId ?? projectId,
      );
    },
    [
      activeProjectId,
      clearAutoManagedTitle,
      findConversationById,
      invalidateConversationLists,
      projectId,
    ]
  );

  const handleAutoRenameConversation = useCallback(
    async (conversation: AgentConversation) => {
      try {
        const result = await chatApi.getConversation(conversation.id);
        const firstMessage = getFirstUserMessageContent(result.messages);
        if (!firstMessage) {
          throw new Error("No user message is available for auto rename");
        }

        await chatApi.spawnConversationSessionNamer(
          conversation.id,
          firstMessage,
          conversation.providerHarness ?? result.conversation.providerHarness ?? null
        );
        clearAutoManagedTitle(conversation.id);
        await invalidateConversationLists(conversation.projectId);
        toast.success("Auto rename started");
      } catch (error) {
        toast.error(
          error instanceof Error ? error.message : "Failed to start auto rename"
        );
        throw error;
      }
    },
    [clearAutoManagedTitle, invalidateConversationLists]
  );

  return {
    handleAutoRenameConversation,
    handleArchiveConversation,
    handleBulkArchiveConversations,
    handleArchiveProject,
    handleRenameConversation,
    handleRestoreConversation,
    handleSetConversationMuted,
    handleBulkMuteConversations,
    handleForkConversation,
    handleSidebarCreateAgent,
    handleStartPersonaBuilder,
    handleSidebarFocusProject,
    handleSidebarSelectConversation,
  };
}
