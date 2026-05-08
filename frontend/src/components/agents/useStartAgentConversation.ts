import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { InfiniteData, QueryClient } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationBaseSelection,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceMode,
  type ChatMessageResponse,
  type ConversationMessagesPageResponse,
} from "@/api/chat";
import { chatKeys, invalidateConversationDataQueries } from "@/hooks/useChat";
import { useAgentModels } from "@/hooks/useAgentModels";
import { getModelLabel } from "@/lib/model-utils";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
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
  const queueMessage = useChatStore((s) => s.queueMessage);
  const setAgentRunning = useChatStore((s) => s.setAgentRunning);
  const setSending = useChatStore((s) => s.setSending);
  const setEffectiveModel = useChatStore((s) => s.setEffectiveModel);
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
        optimisticMessages: ChatMessageResponse[] = [],
      ) => {
        const conversationId = conversation.id;
        const optimisticConversation = toProjectAgentConversation(conversation);
        const storeKey = getAgentConversationStoreKey(optimisticConversation);

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
          messages: optimisticMessages,
        });
        if (optimisticMessages.length > 0) {
          queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
            chatKeys.conversationHistory(conversationId),
            {
              pages: [
                {
                  conversation,
                  messages: optimisticMessages,
                  limit: 40,
                  offset: 0,
                  totalMessageCount: optimisticMessages.length,
                  hasOlder: false,
                },
              ],
              pageParams: [0],
            }
          );
        }
        queryClient.setQueryData(
          ["agents", "conversation-workspace", conversationId],
          workspace ?? null,
        );
        setOptimisticSelectedConversationId(conversationId);
        setFocusedProject(targetProjectId);
        setRuntimeForConversation(conversationId, targetProjectId, normalizedRuntime);
        selectConversation(targetProjectId, conversationId);
        setActiveConversation(storeKey, conversationId);
      };
      const resultConversationSeed = await chatApi.createConversation(
        "project",
        targetProjectId
      );
      const seededConversation: ChatConversation = {
        ...resultConversationSeed,
        agentMode: mode,
      };
      const storeKey = getAgentConversationStoreKey({
        id: seededConversation.id,
        contextType: "project",
        contextId: targetProjectId,
      });
      const optimisticUserMessage = buildOptimisticUserMessage({
        conversation: seededConversation,
        content,
        runtime: normalizedRuntime,
      });
      seedConversationState(seededConversation, null, [optimisticUserMessage]);
      setEffectiveModel(storeKey, {
        id: normalizedRuntime.modelId,
        label: getModelLabel(normalizedRuntime.modelId),
      });
      setAgentRunning(storeKey, true);
      setSending(storeKey, true);

      try {
        if (files.length > 0) {
          await Promise.all(
            files.map((file) => uploadDraftAttachment(seededConversation.id, file))
          );
        }

        const result = await chatApi.startAgentConversation({
          projectId: targetProjectId,
          content,
          conversationId: seededConversation.id,
          providerHarness: normalizedRuntime.provider,
          modelId: normalizedRuntime.modelId,
          logicalEffort: normalizedRuntime.effort,
          mode,
          ...(base ? { base } : {}),
        });
        const resolvedConversation: ChatConversation = {
          ...result.conversation,
          agentMode: result.conversation.agentMode ?? mode,
        };
        const resolvedConversationId = resolvedConversation.id;
        const optimisticWorkspace = result.workspace;
        const resolvedStoreKey = getAgentConversationStoreKey(resolvedConversation);
        const resolvedOptimisticUserMessage =
          resolvedConversationId === seededConversation.id
            ? optimisticUserMessage
            : buildOptimisticUserMessage({
                conversation: resolvedConversation,
                content,
                runtime: normalizedRuntime,
              });
        seedConversationState(
          resolvedConversation,
          optimisticWorkspace ?? null,
          [resolvedOptimisticUserMessage]
        );
        if (resolvedStoreKey !== storeKey) {
          setAgentRunning(storeKey, false);
          setSending(storeKey, false);
          setEffectiveModel(resolvedStoreKey, {
            id: normalizedRuntime.modelId,
            label: getModelLabel(normalizedRuntime.modelId),
          });
          setAgentRunning(resolvedStoreKey, true);
        }
        if (
          result.sendResult.wasQueued &&
          result.sendResult.queuedMessageId != null
        ) {
          queueMessage(
            resolvedStoreKey,
            content,
            result.sendResult.queuedMessageId
          );
        }
        if (result.sendResult.wasQueued || result.sendResult.queuedAsPending) {
          setAgentRunning(resolvedStoreKey, false);
        }
        setSending(resolvedStoreKey, false);
        invalidateConversationDataQueries(queryClient, resolvedConversationId);
        await invalidateProjectConversations(targetProjectId);
        handleAutoManagedTitle({
          content,
          conversationId: resolvedConversationId,
          targetProjectId,
          shouldSpawnSessionNamer: true,
        });
      } catch (err) {
        setAgentRunning(storeKey, false);
        setSending(storeKey, false);
        throw err;
      }
    },
    [
      handleAutoManagedTitle,
      invalidateProjectConversations,
      modelRegistry,
      queryClient,
      queueMessage,
      selectConversation,
      setActiveConversation,
      setAgentRunning,
      setEffectiveModel,
      setFocusedProject,
      setOptimisticConversationsById,
      setOptimisticSelectedConversationId,
      setOptimisticWorkspacesByConversationId,
      setRuntimeForConversation,
      setSending,
    ]
  );

  return handleStartAgentConversation;
}

function buildOptimisticUserMessage({
  conversation,
  content,
  runtime,
}: {
  conversation: ChatConversation;
  content: string;
  runtime: AgentRuntimeSelection;
}): ChatMessageResponse {
  return {
    id: `optimistic:${conversation.id}:initial-user`,
    sessionId: null,
    projectId: conversation.contextType === "project" ? conversation.contextId : null,
    taskId: null,
    role: "user",
    content,
    metadata: null,
    parentMessageId: null,
    conversationId: conversation.id,
    toolCalls: null,
    contentBlocks: null,
    sender: null,
    attributionSource: null,
    providerHarness: runtime.provider,
    providerSessionId: null,
    upstreamProvider: null,
    providerProfile: null,
    logicalModel: runtime.modelId,
    effectiveModelId: runtime.modelId,
    logicalEffort: runtime.effort,
    effectiveEffort: null,
    inputTokens: null,
    outputTokens: null,
    cacheCreationTokens: null,
    cacheReadTokens: null,
    estimatedUsd: null,
    createdAt: new Date().toISOString(),
  };
}
