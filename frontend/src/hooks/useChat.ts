/**
 * useChat hook - TanStack Query wrapper for context-aware chat
 *
 * Provides hooks for fetching and sending chat messages based on context.
 * Supports conversation management, agent run status, and real-time updates.
 */

import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type InfiniteData,
  type QueryClient,
} from "@tanstack/react-query";
import { useEffect, useCallback, useMemo, useRef } from "react";
import {
  chatApi,
  parseContentBlocks,
  parseToolCalls,
  type ChatMessageResponse,
  type CapabilityIntent,
  type ComposerArtifactReference,
  type ComposerIntegrationReference,
  type ComposerProjectReference,
  type ComposerSelectionSnapshot,
  type ConversationMessagesPageResponse,
  type ConversationTimelinePageResponse,
  type SendAgentMessageOptions,
  type SendAgentMessageResult,
  type TeamIntent,
  type TeamMessageTarget,
} from "@/api/chat";
import { isVisibleChatMessage } from "@/api/chat-message-visibility";
import {
  appendMessageToConversationHistory,
  appendMessageIfMissing,
  createOptimisticUserMessage,
  matchesOptimisticMessage,
  removeMessageFromConversationHistory,
  type ConversationHistoryCacheData,
} from "./chat-cache";
import {
  serializeComposerReferencesMetadata,
  type MessageFolderReference,
} from "@/components/Chat/MessageReferences.parse";
import type { ChatContext } from "@/types/chat";
import type { ChatConversation, AgentRun, ContextType } from "@/types/chat-conversation";
import { useChatStore } from "@/stores/chatStore";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { ideationKeys } from "./useIdeation";
import { useAgentEvents } from "./useAgentEvents";

/**
 * Query key factory for chat
 */
export const chatKeys = {
  all: ["chat"] as const,
  messages: () => [...chatKeys.all, "messages"] as const,
  conversations: () => [...chatKeys.all, "conversations"] as const,
  conversation: (conversationId: string) =>
    [...chatKeys.conversations(), conversationId] as const,
  conversationSummary: (conversationId: string) =>
    [...chatKeys.conversation(conversationId), "summary"] as const,
  conversationHistory: (conversationId: string) =>
    [...chatKeys.conversation(conversationId), "history"] as const,
  conversationTimeline: (conversationId: string) =>
    [...chatKeys.conversation(conversationId), "timeline"] as const,
  conversationList: (contextType: ContextType, contextId: string) =>
    [...chatKeys.conversations(), contextType, contextId] as const,
  agentRun: (conversationId: string) =>
    [...chatKeys.all, "agent-run", conversationId] as const,
  // Legacy keys for backward compatibility
  sessionMessages: (sessionId: string) =>
    [...chatKeys.messages(), "session", sessionId] as const,
  projectMessages: (projectId: string) =>
    [...chatKeys.messages(), "project", projectId] as const,
  taskMessages: (taskId: string) =>
    [...chatKeys.messages(), "task", taskId] as const,
};

export type ConversationQueryData = {
  conversation: ChatConversation;
  messages: ChatMessageResponse[];
};

type SendMessageVariables = {
  content: string;
  composerFolderReferences?: MessageFolderReference[];
  attachmentIds?: string[];
  composerArtifactReferences?: ComposerArtifactReference[];
  composerProjectReferences?: ComposerProjectReference[];
  composerIntegrationReferences?: ComposerIntegrationReference[];
  capabilityIntent?: CapabilityIntent | null;
  composerSelectionSnapshot?: ComposerSelectionSnapshot;
  teamIntent?: TeamIntent | null;
  teamMessageTarget?: TeamMessageTarget | null;
};

type SendMessageMutationContext = {
  optimisticConversationId?: string;
  optimisticMessageId?: string;
};

export const OPTIMISTIC_CONVERSATION_ID_PREFIX = "optimistic-conversation:";

export function createOptimisticConversationId() {
  const randomId =
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `${OPTIMISTIC_CONVERSATION_ID_PREFIX}${randomId}`;
}

export function isOptimisticConversationId(conversationId: string | null | undefined) {
  return Boolean(conversationId?.startsWith(OPTIMISTIC_CONVERSATION_ID_PREFIX));
}

function normalizeExplicitMaxPages(maxPages: number | undefined): number | null {
  return maxPages === undefined ? null : Math.max(1, maxPages);
}

function reachedExplicitPageLimit(pageCount: number, maxPages: number | null) {
  return maxPages !== null && pageCount >= maxPages;
}

export type ConversationHistoryWindowData = ConversationQueryData & {
  totalMessageCount: number;
  loadedStartIndex: number;
};

function getConversationMessagesFromHistoryData(
  data: InfiniteData<ConversationMessagesPageResponse> | undefined
): ConversationHistoryWindowData | undefined {
  if (!data || data.pages.length === 0) {
    return undefined;
  }

  const [newestPage] = data.pages;
  if (!newestPage) {
    return undefined;
  }
  const rawMessages = data.pages
    .slice()
    .reverse()
    .flatMap((page) => page.messages);
  const messages = rawMessages.filter(isVisibleChatMessage);
  const totalMessageCount = Math.max(
    0,
    newestPage.totalMessageCount - (rawMessages.length - messages.length)
  );

  return {
    conversation: newestPage.conversation,
    messages,
    totalMessageCount,
    loadedStartIndex: Math.max(0, totalMessageCount - messages.length),
  };
}

function getConversationMessagesFromTimelineData(
  data: InfiniteData<ConversationTimelinePageResponse> | undefined
): ConversationHistoryWindowData | undefined {
  if (!data || data.pages.length === 0) {
    return undefined;
  }

  const [newestPage] = data.pages;
  if (!newestPage) {
    return undefined;
  }
  const loadedMessages = data.pages
    .slice()
    .reverse()
    .flatMap((page) => page.messages);
  // Timeline queries are exclusive in normal operation, but refetches can still
  // briefly overlap an older cached page. Prefer the newer query-page copy while
  // retaining a single chronological item for the virtualizer.
  const rawMessages = dedupeTimelineMessages(loadedMessages);
  const messages = rawMessages.filter(isVisibleChatMessage);
  const totalMessageCount = Math.max(
    0,
    newestPage.totalItemCount - (rawMessages.length - messages.length)
  );

  return {
    conversation: newestPage.conversation,
    messages,
    totalMessageCount,
    loadedStartIndex: Math.max(0, totalMessageCount - messages.length),
  };
}

function dedupeTimelineMessages(
  messages: ChatMessageResponse[],
): ChatMessageResponse[] {
  const slots: Array<ChatMessageResponse | undefined> = [];
  const indexById = new Map<string, number>();
  const indexBySequence = new Map<number, number>();
  for (const message of messages) {
    const existingIndex = indexById.get(message.id)
      ?? (message.timelineSequence != null ? indexBySequence.get(message.timelineSequence) : undefined);
    if (existingIndex != null) {
      const previous = slots[existingIndex];
      if (previous) {
        indexById.delete(previous.id);
        if (previous.timelineSequence != null) indexBySequence.delete(previous.timelineSequence);
      }
      slots[existingIndex] = message;
      indexById.set(message.id, existingIndex);
      if (message.timelineSequence != null) indexBySequence.set(message.timelineSequence, existingIndex);
      continue;
    }
    const index = slots.length;
    slots.push(message);
    indexById.set(message.id, index);
    if (message.timelineSequence != null) indexBySequence.set(message.timelineSequence, index);
  }
  return slots.filter((message): message is ChatMessageResponse => message != null);
}

function createOptimisticTimelineItem(
  message: ChatMessageResponse,
  sequence: number
): ConversationTimelinePageResponse["items"][number] {
  const contentBlocks: NonNullable<ChatMessageResponse["contentBlocks"]> = [
    { type: "text", text: message.content },
  ];
  const asMessage: ChatMessageResponse = {
    ...message,
    id: `optimistic-timeline:${message.id}`,
    parentMessageId: message.id,
    contentBlocks,
    timelineStatus: "streaming",
    timelineKind: "text",
    timelineSequence: sequence,
  };

  return {
    id: asMessage.id,
    conversationId: message.conversationId ?? "",
    messageId: message.id,
    runId: null,
    sequence,
    blockIndex: 0,
    role: message.role,
    kind: "text",
    status: "streaming",
    content: message.content,
    contentBlocks,
    toolCall: null,
    metadata: message.metadata,
    providerHarness: message.providerHarness ?? null,
    providerSessionId: message.providerSessionId ?? null,
    createdAt: message.createdAt,
    updatedAt: message.createdAt,
    finalizedAt: null,
    asMessage,
  };
}

function appendMessageToConversationTimeline(
  oldData: InfiniteData<ConversationTimelinePageResponse> | undefined,
  message: ChatMessageResponse
): InfiniteData<ConversationTimelinePageResponse> | undefined {
  if (!oldData || oldData.pages.length === 0) {
    return oldData;
  }

  const [newestPage, ...olderPages] = oldData.pages;
  if (!newestPage) {
    return oldData;
  }
  const sequence = (newestPage.newestLoadedSequence ?? newestPage.totalItemCount) + 1;
  const item = createOptimisticTimelineItem(message, sequence);
  return {
    ...oldData,
    pages: [
      {
        ...newestPage,
        items: [...newestPage.items, item],
        messages: [...newestPage.messages, item.asMessage],
        totalItemCount: newestPage.totalItemCount + 1,
        oldestLoadedSequence: newestPage.oldestLoadedSequence ?? item.sequence,
        newestLoadedSequence: item.sequence,
      },
      ...olderPages,
    ],
  };
}

type FinalizedContentBlock = NonNullable<ChatMessageResponse["contentBlocks"]>[number];

function upsertMessage(messages: ChatMessageResponse[], message: ChatMessageResponse) {
  const index = messages.findIndex((existing) => existing.id === message.id);
  if (index < 0) {
    return [...messages, message];
  }
  const next = [...messages];
  next[index] = message;
  return next;
}

function toolCallFromContentBlock(block: FinalizedContentBlock) {
  if (block.type !== "tool_use") return null;
  return {
    id: block.id ?? `tool:${block.name ?? "unknown"}`,
    name: block.name ?? "unknown",
    arguments: block.arguments ?? {},
    ...(block.result !== undefined ? { result: block.result } : {}),
    ...(block.resultPreviewTruncated !== undefined
      ? { resultPreviewTruncated: block.resultPreviewTruncated }
      : {}),
    ...(block.resultPreviewOriginalBytes !== undefined
      ? { resultPreviewOriginalBytes: block.resultPreviewOriginalBytes }
      : {}),
    ...(block.resultPreviewLineCount !== undefined
      ? { resultPreviewLineCount: block.resultPreviewLineCount }
      : {}),
    ...(block.resultPreviewOmittedLines !== undefined
      ? { resultPreviewOmittedLines: block.resultPreviewOmittedLines }
      : {}),
    ...(block.argumentsPreviewTruncated !== undefined
      ? { argumentsPreviewTruncated: block.argumentsPreviewTruncated }
      : {}),
    ...(block.argumentsPreviewOriginalBytes !== undefined
      ? { argumentsPreviewOriginalBytes: block.argumentsPreviewOriginalBytes }
      : {}),
    ...(block.argumentsPreviewLineCount !== undefined
      ? { argumentsPreviewLineCount: block.argumentsPreviewLineCount }
      : {}),
    ...(block.argumentsPreviewOmittedLines !== undefined
      ? { argumentsPreviewOmittedLines: block.argumentsPreviewOmittedLines }
      : {}),
    ...(block.diffPreview ? { diffPreview: block.diffPreview } : {}),
    ...(block.detailRef ? { detailRef: block.detailRef } : {}),
    ...(block.parentToolUseId ? { parentToolUseId: block.parentToolUseId } : {}),
    ...(block.diffContext ? { diffContext: block.diffContext } : {}),
  };
}

function createFinalizedTimelineItem(
  message: ChatMessageResponse,
  block: FinalizedContentBlock,
  blockIndex: number,
  sequence: number
): ConversationTimelinePageResponse["items"][number] {
  const toolCall = toolCallFromContentBlock(block);
  const content = block.type === "text" ? block.text ?? "" : "";
  const itemId = `block:${message.id}:${blockIndex}`;
  const asMessage: ChatMessageResponse = {
    ...message,
    id: itemId,
    parentMessageId: message.id,
    content,
    toolCalls: toolCall ? [toolCall] : null,
    contentBlocks: [block],
    timelineStatus: "finalized",
    timelineKind: block.type,
    timelineSequence: sequence,
  };

  return {
    id: itemId,
    conversationId: message.conversationId ?? "",
    messageId: message.id,
    runId: null,
    sequence,
    blockIndex,
    role: message.role,
    kind: block.type,
    status: "finalized",
    content,
    contentBlocks: [block],
    toolCall,
    metadata: message.metadata,
    providerHarness: message.providerHarness ?? null,
    providerSessionId: message.providerSessionId ?? null,
    upstreamProvider: message.upstreamProvider ?? null,
    providerProfile: message.providerProfile ?? null,
    logicalModel: message.logicalModel ?? null,
    effectiveModelId: message.effectiveModelId ?? null,
    logicalEffort: message.logicalEffort ?? null,
    effectiveEffort: message.effectiveEffort ?? null,
    inputTokens: message.inputTokens ?? null,
    outputTokens: message.outputTokens ?? null,
    cacheCreationTokens: message.cacheCreationTokens ?? null,
    cacheReadTokens: message.cacheReadTokens ?? null,
    estimatedUsd: message.estimatedUsd ?? null,
    createdAt: message.createdAt,
    updatedAt: message.createdAt,
    finalizedAt: message.createdAt,
    asMessage,
  };
}

export function upsertFinalizedMessageIntoConversationCache(
  queryClient: QueryClient,
  conversationId: string,
  message: ChatMessageResponse
): boolean {
  if (!isVisibleChatMessage(message)) {
    return false;
  }

  const contentBlocks =
    message.contentBlocks && message.contentBlocks.length > 0
      ? message.contentBlocks
      : message.content.trim().length > 0
        ? [{ type: "text" as const, text: message.content }]
        : [];
  if (contentBlocks.length === 0) {
    return false;
  }

  let updatedTimeline = false;
  queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
    chatKeys.conversationTimeline(conversationId),
    (oldData) => {
      if (!oldData || oldData.pages.length === 0) {
        return oldData;
      }

      const [newestPage, ...olderPages] = oldData.pages;
      if (!newestPage) {
        return oldData;
      }

      const belongsToMessage = (item: ConversationTimelinePageResponse["items"][number]) =>
        item.messageId === message.id || item.asMessage.parentMessageId === message.id;
      const removedItems = newestPage.items.filter(belongsToMessage);
      const retainedItems = newestPage.items.filter((item) => !belongsToMessage(item));
      // Blocks already in the cache keep the position they were durably
      // written at; re-homing them to the tail would push them above items
      // that legitimately follow them (e.g. a user message sent mid-run).
      const previousSequences = new Map(removedItems.map((item) => [item.id, item.sequence]));
      let nextAppendSequence =
        Math.max(
          newestPage.newestLoadedSequence ?? 0,
          ...retainedItems.map((item) => item.sequence),
          ...previousSequences.values(),
        ) + 1;
      const insertedItems = contentBlocks.map((block, index) => {
        const reusedSequence = previousSequences.get(`block:${message.id}:${index}`);
        const sequence = reusedSequence ?? nextAppendSequence++;
        return createFinalizedTimelineItem(message, block, index, sequence);
      });
      const items = [...retainedItems, ...insertedItems].sort(
        (left, right) => left.sequence - right.sequence
      );
      const totalItemCount = Math.max(
        insertedItems.length,
        newestPage.totalItemCount - (newestPage.items.length - retainedItems.length) + insertedItems.length
      );
      updatedTimeline = true;

      return {
        ...oldData,
        pages: [
          {
            ...newestPage,
            items,
            messages: items.map((item) => item.asMessage),
            totalItemCount,
            oldestLoadedSequence: items[0]?.sequence ?? null,
            newestLoadedSequence: items[items.length - 1]?.sequence ?? null,
          },
          ...olderPages,
        ],
      };
    }
  );

  if (!updatedTimeline) {
    return false;
  }

  queryClient.setQueryData<ConversationQueryData>(
    chatKeys.conversation(conversationId),
    (oldData) => oldData ? { ...oldData, messages: upsertMessage(oldData.messages ?? [], message) } : oldData
  );
  queryClient.setQueryData<ConversationHistoryCacheData>(
    chatKeys.conversationHistory(conversationId),
    (oldData) => appendMessageToConversationHistory(oldData, message)
  );

  return true;
}

export type RenderReadyMessagePayload = {
  id: string;
  conversation_id?: string | null;
  role: string;
  content: string;
  metadata?: string | null;
  tool_calls?: unknown;
  content_blocks?: unknown;
  attribution_source?: string | null;
  provider_harness?: string | null;
  provider_session_id?: string | null;
  upstream_provider?: string | null;
  provider_profile?: string | null;
  logical_model?: string | null;
  effective_model_id?: string | null;
  logical_effort?: string | null;
  effective_effort?: string | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  cache_creation_tokens?: number | null;
  cache_read_tokens?: number | null;
  estimated_usd?: number | null;
  created_at: string;
};

export type RenderReadyTimelineItemPayload = {
  id: string;
  conversation_id?: string | null;
  message_id?: string | null;
  run_id?: string | null;
  sequence: number;
  block_index: number;
  role: string;
  kind: string;
  status: string;
  content: string;
  content_blocks: unknown;
  tool_call?: unknown;
  metadata?: string | null;
  provider_harness?: string | null;
  provider_session_id?: string | null;
  upstream_provider?: string | null;
  provider_profile?: string | null;
  logical_model?: string | null;
  effective_model_id?: string | null;
  logical_effort?: string | null;
  effective_effort?: string | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  cache_creation_tokens?: number | null;
  cache_read_tokens?: number | null;
  estimated_usd?: number | null;
  created_at: string;
  updated_at: string;
  finalized_at?: string | null;
};

export type RenderReadyMessageCreatedPayload = {
  message?: RenderReadyMessagePayload | null;
  timeline_items?: RenderReadyTimelineItemPayload[] | null;
};

function messageFromRenderReadyPayload(
  raw: RenderReadyMessagePayload,
  fallbackConversationId: string
): ChatMessageResponse {
  const toolCalls = parseToolCalls(raw.tool_calls);
  const contentBlocks = parseContentBlocks(raw.content_blocks);
  return {
    id: raw.id,
    sessionId: null,
    projectId: null,
    taskId: null,
    role: raw.role,
    content: raw.content,
    metadata: raw.metadata ?? null,
    parentMessageId: null,
    conversationId: raw.conversation_id ?? fallbackConversationId,
    toolCalls: toolCalls.length > 0 ? toolCalls : null,
    contentBlocks: contentBlocks.length > 0 ? contentBlocks : null,
    sender: null,
    attributionSource: raw.attribution_source ?? null,
    providerHarness: raw.provider_harness ?? null,
    providerSessionId: raw.provider_session_id ?? null,
    upstreamProvider: raw.upstream_provider ?? null,
    providerProfile: raw.provider_profile ?? null,
    logicalModel: raw.logical_model ?? null,
    effectiveModelId: raw.effective_model_id ?? null,
    logicalEffort: raw.logical_effort ?? null,
    effectiveEffort: raw.effective_effort ?? null,
    inputTokens: raw.input_tokens ?? null,
    outputTokens: raw.output_tokens ?? null,
    cacheCreationTokens: raw.cache_creation_tokens ?? null,
    cacheReadTokens: raw.cache_read_tokens ?? null,
    estimatedUsd: raw.estimated_usd ?? null,
    createdAt: raw.created_at,
  };
}

function timelineItemFromRenderReadyPayload(
  raw: RenderReadyTimelineItemPayload,
  fallbackConversationId: string
): ConversationTimelinePageResponse["items"][number] {
  const conversationId = raw.conversation_id ?? fallbackConversationId;
  const contentBlocks = parseContentBlocks(raw.content_blocks);
  const toolCalls = raw.tool_call ? parseToolCalls([raw.tool_call]) : [];
  const toolCall = toolCalls[0] ?? null;
  const asMessage: ChatMessageResponse = {
    id: raw.id,
    sessionId: null,
    projectId: null,
    taskId: null,
    role: raw.role,
    content: raw.content,
    metadata: raw.metadata ?? null,
    parentMessageId: raw.message_id ?? null,
    conversationId,
    toolCalls: toolCall ? [toolCall] : null,
    contentBlocks,
    sender: null,
    attributionSource: null,
    providerHarness: raw.provider_harness ?? null,
    providerSessionId: raw.provider_session_id ?? null,
    upstreamProvider: raw.upstream_provider ?? null,
    providerProfile: raw.provider_profile ?? null,
    logicalModel: raw.logical_model ?? null,
    effectiveModelId: raw.effective_model_id ?? null,
    logicalEffort: raw.logical_effort ?? null,
    effectiveEffort: raw.effective_effort ?? null,
    inputTokens: raw.input_tokens ?? null,
    outputTokens: raw.output_tokens ?? null,
    cacheCreationTokens: raw.cache_creation_tokens ?? null,
    cacheReadTokens: raw.cache_read_tokens ?? null,
    estimatedUsd: raw.estimated_usd ?? null,
    timelineStatus: raw.status,
    timelineKind: raw.kind,
    timelineSequence: raw.sequence,
    runId: raw.run_id ?? null,
    createdAt: raw.created_at,
    finalizedAt: raw.finalized_at ?? null,
  };

  return {
    id: raw.id,
    conversationId,
    messageId: raw.message_id ?? null,
    runId: raw.run_id ?? null,
    sequence: raw.sequence,
    blockIndex: raw.block_index,
    role: raw.role,
    kind: raw.kind,
    status: raw.status,
    content: raw.content,
    contentBlocks,
    toolCall,
    metadata: raw.metadata ?? null,
    providerHarness: raw.provider_harness ?? null,
    providerSessionId: raw.provider_session_id ?? null,
    upstreamProvider: raw.upstream_provider ?? null,
    providerProfile: raw.provider_profile ?? null,
    logicalModel: raw.logical_model ?? null,
    effectiveModelId: raw.effective_model_id ?? null,
    logicalEffort: raw.logical_effort ?? null,
    effectiveEffort: raw.effective_effort ?? null,
    inputTokens: raw.input_tokens ?? null,
    outputTokens: raw.output_tokens ?? null,
    cacheCreationTokens: raw.cache_creation_tokens ?? null,
    cacheReadTokens: raw.cache_read_tokens ?? null,
    estimatedUsd: raw.estimated_usd ?? null,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    finalizedAt: raw.finalized_at ?? null,
    asMessage,
  };
}

export function upsertRenderReadyMessageIntoConversationCache(
  queryClient: QueryClient,
  conversationId: string,
  payload: RenderReadyMessageCreatedPayload
): boolean {
  if (!payload.message || !payload.timeline_items || payload.timeline_items.length === 0) {
    return false;
  }

  const message = messageFromRenderReadyPayload(payload.message, conversationId);
  if (!isVisibleChatMessage(message)) {
    return false;
  }
  const insertedItems = payload.timeline_items.map((item) =>
    timelineItemFromRenderReadyPayload(item, conversationId)
  ).filter((item) => isVisibleChatMessage(item.asMessage));
  if (insertedItems.length === 0) {
    return false;
  }
  let updatedTimeline = false;

  queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
    chatKeys.conversationTimeline(conversationId),
    (oldData) => {
      if (!oldData || oldData.pages.length === 0) {
        return oldData;
      }

      const [newestPage, ...olderPages] = oldData.pages;
      if (!newestPage) {
        return oldData;
      }

      const retainedItems = newestPage.items.filter(
        (item) =>
          item.messageId !== message.id &&
          item.asMessage.parentMessageId !== message.id &&
          // The optimistic row carries a client-generated id, so the backend id
          // cannot retire it — without this the user sees two identical bubbles.
          !matchesOptimisticMessage(
            {
              id: item.messageId ?? item.id,
              conversationId: item.conversationId,
              role: item.role,
              content: item.content,
            },
            message
          )
      );
      const items = [...retainedItems, ...insertedItems].sort(
        (left, right) => left.sequence - right.sequence
      );
      const removedCount = newestPage.items.length - retainedItems.length;
      updatedTimeline = true;

      return {
        ...oldData,
        pages: [
          {
            ...newestPage,
            items,
            messages: items.map((item) => item.asMessage),
            totalItemCount: Math.max(
              insertedItems.length,
              newestPage.totalItemCount - removedCount + insertedItems.length
            ),
            oldestLoadedSequence: items[0]?.sequence ?? null,
            newestLoadedSequence: items[items.length - 1]?.sequence ?? null,
          },
          ...olderPages,
        ],
      };
    }
  );

  if (!updatedTimeline) {
    return false;
  }

  queryClient.setQueryData<ConversationQueryData>(
    chatKeys.conversation(conversationId),
    (oldData) => oldData ? { ...oldData, messages: upsertMessage(oldData.messages ?? [], message) } : oldData
  );
  queryClient.setQueryData<ConversationHistoryCacheData>(
    chatKeys.conversationHistory(conversationId),
    (oldData) => appendMessageToConversationHistory(oldData, message)
  );

  return true;
}

function removeMessageFromConversationTimeline(
  oldData: InfiniteData<ConversationTimelinePageResponse> | undefined,
  messageId: string
): InfiniteData<ConversationTimelinePageResponse> | undefined {
  if (!oldData) {
    return oldData;
  }

  let removed = 0;
  const pages = oldData.pages.map((page) => {
    const items = page.items.filter((item) => {
      const shouldRemove = item.messageId === messageId || item.asMessage.id === messageId;
      if (shouldRemove) {
        removed += 1;
      }
      return !shouldRemove;
    });
    return {
      ...page,
      items,
      messages: items.map((item) => item.asMessage),
      totalItemCount: Math.max(0, page.totalItemCount - removed),
      oldestLoadedSequence: items[0]?.sequence ?? null,
      newestLoadedSequence: items[items.length - 1]?.sequence ?? null,
    };
  });

  return {
    ...oldData,
    pages,
  };
}

export function getCachedConversationMessages(
  queryClient: QueryClient,
  conversationId: string
): ChatMessageResponse[] {
  const fullConversation = queryClient.getQueryData<ConversationQueryData>(
    chatKeys.conversation(conversationId)
  );
  const historyConversation = getConversationMessagesFromHistoryData(
    queryClient.getQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory(conversationId)
    )
  );
  const timelineConversation = getConversationMessagesFromTimelineData(
    queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline(conversationId)
    )
  );

  const mergedMessages = new Map<string, ChatMessageResponse>();
  for (const message of fullConversation?.messages ?? []) {
    if (!isVisibleChatMessage(message)) continue;
    mergedMessages.set(message.id, message);
  }
  for (const message of historyConversation?.messages ?? []) {
    mergedMessages.set(message.id, message);
  }
  for (const message of timelineConversation?.messages ?? []) {
    mergedMessages.set(message.id, message);
  }

  return Array.from(mergedMessages.values());
}

export function invalidateConversationDataQueries(
  queryClient: QueryClient,
  conversationId: string
) {
  queryClient.invalidateQueries({
    queryKey: chatKeys.conversation(conversationId),
    exact: true,
  });
  queryClient.invalidateQueries({
    queryKey: chatKeys.conversationSummary(conversationId),
  });
  queryClient.invalidateQueries({
    queryKey: chatKeys.conversationHistory(conversationId),
  });
  queryClient.invalidateQueries({
    queryKey: chatKeys.conversationTimeline(conversationId),
  });
  queryClient.invalidateQueries({
    queryKey: ["message-attachments", conversationId],
  });
}

export function addOptimisticUserMessageToConversationCache(
  queryClient: QueryClient,
  conversationId: string,
  content: string,
  options?: { metadata: string | null }
) {
  const message = createOptimisticUserMessage({
    conversationId,
    content,
    ...(options && "metadata" in options ? { metadata: options.metadata } : {}),
  });
  queryClient.setQueryData<ConversationQueryData>(
    chatKeys.conversation(conversationId),
    (oldData) => {
      if (!oldData) return oldData;
      return {
        ...oldData,
        messages: appendMessageIfMissing(oldData.messages ?? [], message),
      };
    }
  );
  queryClient.setQueryData<ConversationHistoryCacheData>(
    chatKeys.conversationHistory(conversationId),
    (oldData) => appendMessageToConversationHistory(oldData, message, { replaceOptimistic: false })
  );
  queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
    chatKeys.conversationTimeline(conversationId),
    (oldData) => appendMessageToConversationTimeline(oldData, message)
  );
  return message;
}

export function removeOptimisticMessageFromConversationCache(
  queryClient: QueryClient,
  conversationId: string,
  messageId: string
) {
  queryClient.setQueryData<ConversationQueryData>(
    chatKeys.conversation(conversationId),
    (oldData) => {
      if (!oldData) return oldData;
      return {
        ...oldData,
        messages: oldData.messages.filter((message) => message.id !== messageId),
      };
    }
  );
  queryClient.setQueryData<ConversationHistoryCacheData>(
    chatKeys.conversationHistory(conversationId),
    (oldData) => removeMessageFromConversationHistory(oldData, messageId)
  );
  queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
    chatKeys.conversationTimeline(conversationId),
    (oldData) => removeMessageFromConversationTimeline(oldData, messageId)
  );
}

/**
 * Get context type and ID from ChatContext
 *
 * NOTE: This function currently doesn't distinguish between 'task', 'task_execution', and 'review'
 * context types when view='task_detail'. Components like TaskChatPanel handle this distinction
 * by directly querying conversations with the appropriate contextType based on task state.
 */
function getContextTypeAndId(context: ChatContext): {
  contextType: ContextType;
  contextId: string;
} {
  if (context.contextTypeOverride && context.contextIdOverride) {
    return {
      contextType: context.contextTypeOverride,
      contextId: context.contextIdOverride,
    };
  }
  switch (context.view) {
    case "ideation":
      if (!context.ideationSessionId) {
        throw new Error("Ideation context requires ideationSessionId");
      }
      return { contextType: "ideation", contextId: context.ideationSessionId };
    case "task_detail":
      if (!context.selectedTaskId) {
        throw new Error("Task detail context requires selectedTaskId");
      }
      // Returns 'task' contextType by default. Components should query conversations
      // with 'task_execution' or 'review' contextType directly when needed based on task state.
      return { contextType: "task", contextId: context.selectedTaskId };
    case "kanban":
      if (context.selectedTaskId) {
        return { contextType: "task", contextId: context.selectedTaskId };
      }
      return { contextType: "project", contextId: context.projectId };
    default:
      return { contextType: "project", contextId: context.projectId };
  }
}

/**
 * Hook to fetch conversations for a context
 */
export function useConversations(context: ChatContext) {
  const { contextType, contextId } = getContextTypeAndId(context);

  return useQuery<ChatConversation[], Error>({
    queryKey: chatKeys.conversationList(contextType, contextId),
    queryFn: () => chatApi.listConversations(contextType, contextId),
    staleTime: 0,
  });
}

/**
 * Hook to fetch a single conversation with messages
 */
export function useConversation(
  conversationId: string | null,
  options?: { enabled?: boolean }
) {
  const canFetchConversation = !!conversationId && !isOptimisticConversationId(conversationId);
  const query = useQuery<
    ConversationQueryData,
    Error
  >({
    queryKey: chatKeys.conversation(conversationId ?? ""),
    queryFn: () => {
      if (!conversationId) {
        throw new Error("Conversation ID is required");
      }
      return chatApi.getConversation(conversationId);
    },
    enabled: (options?.enabled ?? true) && canFetchConversation,
  });

  return query;
}

export function useConversationSummary(
  conversationId: string | null,
  options?: { enabled?: boolean }
) {
  const canFetchConversation = !!conversationId && !isOptimisticConversationId(conversationId);
  return useQuery<ChatConversation | null, Error>({
    queryKey: chatKeys.conversationSummary(conversationId ?? ""),
    queryFn: () => {
      if (!conversationId) {
        throw new Error("Conversation ID is required");
      }
      return chatApi.getConversationSummary(conversationId);
    },
    enabled: (options?.enabled ?? true) && canFetchConversation,
    staleTime: 30 * 1000,
  });
}

export function useConversationHistoryWindow(
  conversationId: string | null,
  options?: { enabled?: boolean; pageSize?: number; maxPages?: number }
) {
  const pageSize = options?.pageSize ?? 40;
  const maxPages = normalizeExplicitMaxPages(options?.maxPages);
  const canFetchConversation = !!conversationId && !isOptimisticConversationId(conversationId);
  const query = useInfiniteQuery<
    ConversationMessagesPageResponse,
    Error,
    InfiniteData<ConversationMessagesPageResponse>,
    ReturnType<typeof chatKeys.conversationHistory>,
    number
  >({
    queryKey: chatKeys.conversationHistory(conversationId ?? ""),
    queryFn: ({ pageParam }) => {
      if (!conversationId) {
        throw new Error("Conversation ID is required");
      }
      return chatApi.getConversationMessagesPage(
        conversationId,
        pageSize,
        pageParam
      );
    },
    enabled: (options?.enabled ?? true) && canFetchConversation,
    initialPageParam: 0,
    getNextPageParam: (lastPage, allPages) => {
      if (!lastPage.hasOlder) {
        return undefined;
      }
      if (reachedExplicitPageLimit(allPages.length, maxPages)) {
        return undefined;
      }
      return lastPage.offset + lastPage.messages.length;
    },
    staleTime: 30 * 1000,
  });

  const data = useMemo(
    () => getConversationMessagesFromHistoryData(query.data),
    [query.data]
  );

  const fetchOlderMessages = useCallback(async () => {
    if (!query.hasNextPage || query.isFetchingNextPage) {
      return;
    }
    await query.fetchNextPage();
  }, [query]);

  return {
    ...query,
    data,
    loadedStartIndex: data?.loadedStartIndex ?? 0,
    hasOlderMessages: query.hasNextPage ?? false,
    isFetchingOlderMessages: query.isFetchingNextPage,
    fetchOlderMessages,
  };
}

export function useConversationTimelineWindow(
  conversationId: string | null,
  options?: { enabled?: boolean; pageSize?: number; maxPages?: number }
) {
  const pageSize = options?.pageSize ?? 40;
  const maxPages = normalizeExplicitMaxPages(options?.maxPages);
  const canFetchConversation = !!conversationId && !isOptimisticConversationId(conversationId);
  const query = useInfiniteQuery<
    ConversationTimelinePageResponse,
    Error,
    InfiniteData<ConversationTimelinePageResponse>,
    ReturnType<typeof chatKeys.conversationTimeline>,
    number | null
  >({
    queryKey: chatKeys.conversationTimeline(conversationId ?? ""),
    queryFn: ({ pageParam }) => {
      if (!conversationId) {
        throw new Error("Conversation ID is required");
      }
      return chatApi.getConversationTimelinePage(
        conversationId,
        pageSize,
        pageParam
      );
    },
    enabled: (options?.enabled ?? true) && canFetchConversation,
    initialPageParam: null,
    getNextPageParam: (lastPage, allPages) => {
      if (!lastPage.hasOlder) {
        return undefined;
      }
      if (reachedExplicitPageLimit(allPages.length, maxPages)) {
        return undefined;
      }
      return lastPage.oldestLoadedSequence;
    },
    staleTime: 5 * 1000,
  });

  const data = useMemo(
    () => getConversationMessagesFromTimelineData(query.data),
    [query.data]
  );

  const fetchOlderMessages = useCallback(async () => {
    if (!query.hasNextPage || query.isFetchingNextPage) {
      return;
    }
    await query.fetchNextPage();
  }, [query]);

  return {
    ...query,
    data,
    loadedStartIndex: data?.loadedStartIndex ?? 0,
    hasOlderMessages: query.hasNextPage ?? false,
    isFetchingOlderMessages: query.isFetchingNextPage,
    fetchOlderMessages,
  };
}

/**
 * Hook to fetch agent run status for a conversation
 */
export function useAgentRunStatus(conversationId: string | null) {
  const canFetchRunStatus = !!conversationId && !isOptimisticConversationId(conversationId);
  return useQuery<AgentRun | null, Error>({
    queryKey: chatKeys.agentRun(conversationId ?? ""),
    queryFn: () => {
      if (!conversationId) {
        return null;
      }
      return chatApi.getAgentRunStatus(conversationId);
    },
    enabled: canFetchRunStatus,
    refetchInterval: (query) => {
      // Poll every 2 seconds if agent is running
      const agentRun = query.state.data;
      return agentRun?.status === "running" ? 2000 : false;
    },
    // Prevent excessive refetching when not polling
    staleTime: 10 * 1000, // 10 seconds
    refetchOnWindowFocus: false,
    refetchOnMount: "always", // Always check on mount for initial state
  });
}

/**
 * Hook for chat functionality with context-aware messaging
 *
 * @param context - The chat context
 * @returns Object with messages query, sendMessage mutation, and conversation management
 *
 * @example
 * ```tsx
 * const {
 *   messages,
 *   conversations,
 *   activeConversation,
 *   agentRunStatus,
 *   sendMessage,
 *   switchConversation,
 *   createConversation,
 * } = useChat({
 *   view: "ideation",
 *   projectId: "project-123",
 *   ideationSessionId: "session-123",
 * });
 * ```
 */
export function useChat(
  context: ChatContext,
  options?: {
    isVisible?: boolean;
    storeKey?: string;
    disableAutoSelect?: boolean;
    skipActiveConversationQuery?: boolean;
    sendOptions?: SendAgentMessageOptions;
  }
) {
  const queryClient = useQueryClient();
  const { contextType, contextId } = getContextTypeAndId(context);
  const contextKey = buildStoreKey(contextType, contextId);
  // effectiveStoreKey: caller-provided storeKey takes precedence over the internally derived contextKey.
  // This is critical when IntegratedChatPanel uses execution-mode-aware storeKeys (e.g., "task_execution:id")
  // while chatContext is still view="task_detail" (which would derive "task:id" internally).
  const effectiveStoreKey = options?.storeKey ?? contextKey;
  const disableAutoSelect = options?.disableAutoSelect ?? false;

  const activeConversationId = useChatStore((s) => s.activeConversationIds[effectiveStoreKey] ?? null);
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);
  const setAgentRunning = useChatStore((s) => s.setAgentRunning);
  const setSending = useChatStore((s) => s.setSending);

  // Fetch conversations for this context
  const conversations = useConversations(context);

  // Fetch the active transcript as a newest-message window. The returned
  // message order is chronological inside the loaded window.
  const activeConversation = useConversationHistoryWindow(activeConversationId, {
    enabled: !(options?.skipActiveConversationQuery ?? false),
    pageSize: 40,
  });

  // Fetch agent run status
  const agentRunStatus = useAgentRunStatus(activeConversationId);

  // Update agent running state when status changes
  // NOTE: This only sets to true on initial load (when backend shows agent is running).
  // The false state is handled by the agent:run_completed event (or agent:turn_completed in interactive mode) to avoid race conditions.
  // Track previous contextKey to detect session switches and skip stale recovery
  const prevContextKeyRef = useRef(contextKey);

  const isRunning = agentRunStatus.data?.status === "running";
  const isFailed = agentRunStatus.data?.status === "failed";
  const errorMessage = agentRunStatus.data?.errorMessage;

  useEffect(() => {
    const contextChanged = prevContextKeyRef.current !== effectiveStoreKey;
    prevContextKeyRef.current = effectiveStoreKey;

    // On context change, skip recovery — useChatPanelContext cleanup handles clearing.
    // Without this guard, stale cached isRunning from the old conversation overrides
    // the cleanup and permanently sticks the new session in "agent responding" state.
    if (contextChanged) {
      return;
    }

    // Normal recovery: sync UI with backend state (e.g., page refresh with running agent)
    // Don't set to false here - let the agent:run_completed event (or agent:turn_completed in interactive mode) handle that
    if (isRunning) {
      setAgentRunning(effectiveStoreKey, true);
    }
  }, [effectiveStoreKey, isRunning, setAgentRunning]);

  // Show error toast when a failed run is detected (e.g., when user comes back)
  // Track which errors we've shown to avoid duplicate toasts
  const shownErrorRef = useRef<string | null>(null);
  useEffect(() => {
    if (isFailed && errorMessage && shownErrorRef.current !== agentRunStatus.data?.id) {
      // Mark this error as shown
      shownErrorRef.current = agentRunStatus.data?.id ?? null;

      // Only show toast when panel is visible (prevents duplicate toasts in dual-panel mode)
      if (options?.isVisible === false) return;

      // Import toast dynamically to avoid circular deps
      import("sonner").then(({ toast }) => {
        toast.error("Previous agent run failed", {
          description: errorMessage.slice(0, 200),
          duration: 10000,
        });
      });
    }
  }, [isFailed, errorMessage, agentRunStatus.data?.id, options?.isVisible]);

  // Send message mutation
  const sendMessage = useMutation<
    SendAgentMessageResult,
    Error,
    SendMessageVariables,
    SendMessageMutationContext
  >({
    mutationFn: async ({
      content,
      attachmentIds,
      composerArtifactReferences,
      composerProjectReferences,
      composerIntegrationReferences,
      capabilityIntent,
      composerSelectionSnapshot,
      teamIntent,
      teamMessageTarget,
    }) => {
      const sendOptions =
        composerProjectReferences?.length ||
        composerIntegrationReferences?.length ||
        composerArtifactReferences?.length ||
        composerSelectionSnapshot ||
        capabilityIntent ||
        teamIntent ||
        teamMessageTarget
          ? {
              ...options?.sendOptions,
              ...(capabilityIntent ? { capabilityIntent } : {}),
              ...(teamIntent ? { teamIntent } : {}),
              ...(teamMessageTarget ? { teamMessageTarget } : {}),
              ...(composerProjectReferences?.length
                ? { composerProjectReferences }
                : {}),
              ...(composerIntegrationReferences?.length
                ? { composerIntegrationReferences }
                : {}),
              ...(composerArtifactReferences?.length
                ? { composerArtifactReferences }
                : {}),
              ...(composerSelectionSnapshot
                ? { composerSelectionSnapshot }
                : {}),
            }
          : options?.sendOptions;
      if (options?.sendOptions) {
        return chatApi.sendAgentMessage(
          contextType,
          contextId,
          content,
          attachmentIds,
          sendOptions
        );
      }

      return chatApi.sendAgentMessage(
        contextType,
        contextId,
        content,
        attachmentIds,
        sendOptions
      );
    },
    onMutate: (variables) => {
      setSending(effectiveStoreKey, true);
      if (!activeConversationId) {
        return {};
      }
      const optimisticMessage = addOptimisticUserMessageToConversationCache(
        queryClient,
        activeConversationId,
        variables.content,
        {
          metadata: serializeComposerReferencesMetadata({
            folderReferences: variables.composerFolderReferences,
            projectReferences: variables.composerProjectReferences,
            integrationReferences: variables.composerIntegrationReferences,
            artifactReferences: variables.composerArtifactReferences,
            selectionSnapshot: variables.composerSelectionSnapshot,
          }),
        }
      );
      return {
        optimisticConversationId: activeConversationId,
        optimisticMessageId: optimisticMessage.id,
      };
    },
    onSettled: () => {
      setSending(effectiveStoreKey, false);
    },
    onSuccess: (_data, _variables, mutationContext) => {
      // Invalidate active conversation to refetch messages
      if (activeConversationId && !mutationContext?.optimisticMessageId) {
        invalidateConversationDataQueries(queryClient, activeConversationId);
      }

      // Invalidate conversations list to update message counts
      queryClient.invalidateQueries({
        queryKey: chatKeys.conversationList(contextType, contextId),
      });

      // If in ideation context, also invalidate session data
      if (context.view === "ideation" && context.ideationSessionId) {
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(context.ideationSessionId),
        });
      }
    },
    onError: (_error, _variables, context) => {
      if (context?.optimisticConversationId && context.optimisticMessageId) {
        removeOptimisticMessageFromConversationCache(
          queryClient,
          context.optimisticConversationId,
          context.optimisticMessageId
        );
      }
      // Reset agent running state on error
      setAgentRunning(effectiveStoreKey, false);
    },
  });

  // Create new conversation mutation
  const createConversationMutation = useMutation<ChatConversation, Error, void>(
    {
      mutationFn: async () => {
        return chatApi.createConversation(contextType, contextId);
      },
      onSuccess: (newConversation) => {
        // Set as active conversation
        setActiveConversation(effectiveStoreKey, newConversation.id);

        // Invalidate conversations list
        queryClient.invalidateQueries({
          queryKey: chatKeys.conversationList(contextType, contextId),
        });
      },
    }
  );

  // Switch conversation
  const switchConversation = useCallback(
    (conversationId: string) => {
      setActiveConversation(effectiveStoreKey, conversationId);

      // Invalidate the conversation query to ensure fresh data is fetched
      invalidateConversationDataQueries(queryClient, conversationId);
    },
    [setActiveConversation, queryClient, effectiveStoreKey]
  );

  // Create new conversation
  const createConversation = useCallback(async () => {
    await createConversationMutation.mutateAsync();
  }, [createConversationMutation]);

  // Subscribe to agent events for real-time updates
  // Pass effectiveStoreKey so setActiveConversation writes to the correct scoped slot.
  useAgentEvents(activeConversationId, effectiveStoreKey);

  // Initialize active conversation if none is set
  // Use a ref to track initialization and prevent infinite loops
  const hasInitializedRef = useRef(false);

  useEffect(() => {
    // Skip auto-select when caller manages active conversation selection externally
    if (disableAutoSelect) return;

    // Only initialize once per context change
    if (hasInitializedRef.current) {
      return;
    }

    if (!activeConversationId && conversations.data && conversations.data.length > 0) {
      // IMPORTANT: Create a copy before sorting to avoid mutating React Query's cached data
      const sorted = [...conversations.data].sort((a, b) => {
        const aTime = a.lastMessageAt || a.createdAt;
        const bTime = b.lastMessageAt || b.createdAt;
        return new Date(bTime).getTime() - new Date(aTime).getTime();
      });
      const mostRecent = sorted[0];

      if (mostRecent) {
        hasInitializedRef.current = true;
        setActiveConversation(effectiveStoreKey, mostRecent.id);
      }
    }
  }, [activeConversationId, conversations.data, setActiveConversation, effectiveStoreKey, disableAutoSelect]);

  // Reset initialization flag when context changes
  useEffect(() => {
    hasInitializedRef.current = false;
  }, [contextType, contextId]);

  return {
    // Messages from active conversation
    messages: activeConversation,
    // All conversations for this context
    conversations,
    // Active conversation data
    activeConversation,
    // Agent run status
    agentRunStatus,
    // Mutations
    sendMessage,
    // Conversation management
    switchConversation,
    createConversation,
    // Effective store key for active conversation operations (caller-provided storeKey or derived contextKey)
    contextKey: effectiveStoreKey,
    // Context info
    contextType,
    contextId,
  };
}
