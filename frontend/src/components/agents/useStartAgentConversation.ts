import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { QueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationBaseSelection,
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
} from "@/api/chat";
import { chatKeys, invalidateConversationDataQueries } from "@/hooks/useChat";
import { useAgentModels } from "@/hooks/useAgentModels";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";
import type { ChatConversation } from "@/types/chat-conversation";

import {
  getAgentConversationStoreKey,
  toProjectAgentConversation,
  type AgentConversation,
} from "./agentConversations";
import { normalizeRuntimeSelection } from "./agentOptions";
import { uploadDraftAttachment } from "./chatAttachmentUpload";

interface HandleAutoManagedTitleArgs {
  content: string;
  conversationId: string;
  targetProjectId: string;
  shouldSpawnSessionNamer: boolean;
}

interface UseStartAgentConversationArgs {
  handleAutoManagedTitle: (args: HandleAutoManagedTitleArgs) => void;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
  queryClient: QueryClient;
  selectConversation: (projectId: string, conversationId: string) => void;
  setActiveConversation: (contextKey: string, conversationId: string) => void;
  setFocusedProject: (projectId: string | null) => void;
  setOptimisticConversationsById: Dispatch<SetStateAction<Record<string, AgentConversation>>>;
  setOptimisticSelectedConversationId: Dispatch<SetStateAction<string | null>>;
  setOptimisticWorkspacesByConversationId: Dispatch<
    SetStateAction<Record<string, AgentConversationWorkspace>>
  >;
  setRuntimeForConversation: (
    conversationId: string,
    projectId: string,
    runtime: AgentRuntimeSelection
  ) => void;
}

export function useStartAgentConversation({
  handleAutoManagedTitle,
  invalidateProjectConversations,
  queryClient,
  selectConversation,
  setActiveConversation,
  setFocusedProject,
  setOptimisticConversationsById,
  setOptimisticSelectedConversationId,
  setOptimisticWorkspacesByConversationId,
  setRuntimeForConversation,
}: UseStartAgentConversationArgs) {
  const { registry: modelRegistry } = useAgentModels();
  const handleStartAgentConversation = useCallback(
    async ({
      projectId: targetProjectId,
      content,
      runtime,
      mode,
      base,
      files,
    }: {
      projectId: string;
      content: string;
      runtime: AgentRuntimeSelection;
      mode: AgentConversationWorkspaceMode;
      base: AgentConversationBaseSelection | null;
      files: File[];
    }) => {
      const normalizedRuntime = normalizeRuntimeSelection(runtime, modelRegistry);
      const seedConversationState = (
        conversation: ChatConversation,
        workspace: AgentConversationWorkspace | null | undefined,
      ) => {
        const conversationId = conversation.id;
        const optimisticConversation = toProjectAgentConversation(conversation);

        setOptimisticConversationsById((current) => ({
          ...current,
          [conversationId]: optimisticConversation,
        }));
        if (workspace) {
          setOptimisticWorkspacesByConversationId((current) => ({
            ...current,
            [conversationId]: workspace,
          }));
        }
        queryClient.setQueryData(chatKeys.conversation(conversationId), {
          conversation,
          messages: [],
        });
        queryClient.setQueryData(
          ["agents", "conversation-workspace", conversationId],
          workspace ?? null,
        );
        setOptimisticSelectedConversationId(conversationId);
        setFocusedProject(targetProjectId);
        setRuntimeForConversation(conversationId, targetProjectId, normalizedRuntime);
        selectConversation(targetProjectId, conversationId);
        setActiveConversation(
          getAgentConversationStoreKey({
            id: conversationId,
            contextType: "project",
            contextId: targetProjectId,
          }),
          conversationId
        );
      };
      const resultConversationSeed = await chatApi.createConversation(
        "project",
        targetProjectId
      );
      seedConversationState(resultConversationSeed, null);

      if (files.length > 0) {
        await Promise.all(
          files.map((file) => uploadDraftAttachment(resultConversationSeed.id, file))
        );
      }

      const result = await chatApi.startAgentConversation({
        projectId: targetProjectId,
        content,
        conversationId: resultConversationSeed.id,
        providerHarness: normalizedRuntime.provider,
        modelId: normalizedRuntime.modelId,
        logicalEffort: normalizedRuntime.effort,
        mode,
        ...(base ? { base } : {}),
      });
      const resolvedConversationId = result.conversation.id;
      const optimisticWorkspace = result.workspace;
      seedConversationState(result.conversation, optimisticWorkspace ?? null);
      invalidateConversationDataQueries(queryClient, resolvedConversationId);
      await invalidateProjectConversations(targetProjectId);
      handleAutoManagedTitle({
        content,
        conversationId: resolvedConversationId,
        targetProjectId,
        shouldSpawnSessionNamer: true,
      });
    },
    [
      handleAutoManagedTitle,
      invalidateProjectConversations,
      modelRegistry,
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

  return handleStartAgentConversation;
}
