/**
 * Mock Chat API
 *
 * Mirrors the interface of src/api/chat.ts with mock implementations.
 */

import type {
  ChatConversation,
  ContextType,
} from "@/types/chat-conversation";
import { normalizeConversationProviderMetadata } from "@/types/chat-conversation";
import type {
  ChatMessageResponse,
  ChatTimelineItemResponse,
  ChildSessionStatusResponse,
  ConversationListPageResponse,
  ConversationStatsResponse,
  ConversationTimelinePageResponse,
  AgentConversationWorkspace,
  AgentConversationWorkspacePublicationEvent,
  ArchiveConversationResult,
  AgentSidebarConversationGroupsResponse,
  AgentSidebarConversationsInput,
  AgentSidebarAttentionLane,
  AgentSidebarPublicationState,
  AgentSidebarSort,
  AgentConversationRuntimeStatus,
  PrecomputeAgentConversationWorkspacePrDescriptionResult,
  PublishAgentConversationWorkspaceResult,
  QueuedMessageResponse,
  SendAgentMessageResult,
  SetAgentConversationWorkspacePrSupervisionInput,
  StartAgentConversationInput,
  StartAgentConversationResult,
  SwitchAgentConversationModeInput,
  SwitchAgentConversationModeResult,
  UpdateAgentConversationCoordinationModeInput,
  SendAgentMessageOptions,
} from "@/api/chat";
import { generateTestUuid } from "@/test/mock-data";
import { buildFallbackConversationStats } from "@/lib/chat/conversation-stats";
import {
  cloneMockChatMessage,
  getMockChatScenario,
  listMockChatScenarios,
  type MockChatScenarioName,
} from "./chat-scenarios";

// ============================================================================
// Mock State
// ============================================================================

const mockConversations: Map<string, ChatConversation> = new Map();
const mockMutedConversations: Set<string> = new Set();
const mockMessages: Map<string, ChatMessageResponse[]> = new Map();
const mockQueuedMessages: Map<string, QueuedMessageResponse[]> = new Map();
const mockWorkspaces: Map<string, AgentConversationWorkspace> = new Map();
const mockWorkspacePublicationEvents: Map<
  string,
  AgentConversationWorkspacePublicationEvent[]
> = new Map();
const mockChildSessionStatuses: Map<string, ChildSessionStatusResponse> = new Map();
const mockChildSessionStatusOverrides: Map<string, MockChildSessionStatusOverride> = new Map();

type MockChildSessionStatusOverride = {
  response?: ChildSessionStatusResponse;
  error?: string;
  delayMs?: number;
};

type MockContentBlock = NonNullable<ChatMessageResponse["contentBlocks"]>[number];

export interface MockChatController {
  reset(): void;
  seedScenario(name: MockChatScenarioName): void;
  seedConversation(
    conversation: ChatConversation,
    messages: ChatMessageResponse[]
  ): void;
  replaceMessages(
    conversationId: string,
    messages: ChatMessageResponse[]
  ): void;
  listScenarios(): MockChatScenarioName[];
  getChildSessionStatus(sessionId: string): Promise<ChildSessionStatusResponse>;
  setChildSessionStatusOverride(
    sessionId: string,
    override: MockChildSessionStatusOverride
  ): void;
  clearChildSessionStatusOverrides(): void;
  listConversations(
    contextType: ContextType,
    contextId: string,
    includeArchived?: boolean,
    archivedOnly?: boolean
  ): Promise<ChatConversation[]>;
  listConversationsPage(
    contextType: ContextType,
    contextId: string,
    limit: number,
    offset?: number,
    includeArchived?: boolean,
    search?: string,
    archivedOnly?: boolean
  ): Promise<ConversationListPageResponse>;
  listAgentSidebarConversations(
    input: AgentSidebarConversationsInput
  ): Promise<AgentSidebarConversationGroupsResponse>;
  setAgentConversationMuted?(
    conversationId: string,
    muted: boolean,
  ): Promise<void>;
  getConversation(
    conversationId: string
  ): Promise<{ conversation: ChatConversation; messages: ChatMessageResponse[] }>;
  getConversationSummary(conversationId: string): Promise<ChatConversation | null>;
  getConversationTimelinePage(
    conversationId: string,
    limit: number,
    beforeSequence?: number | null
  ): Promise<ConversationTimelinePageResponse>;
  getConversationStats(
    conversationId: string
  ): Promise<ConversationStatsResponse | null>;
  seedAgentConversationWorkspace(workspace: AgentConversationWorkspace): void;
}

export function resetMockChatState(): void {
  mockConversations.clear();
  mockMutedConversations.clear();
  mockMessages.clear();
  mockQueuedMessages.clear();
  mockWorkspaces.clear();
  mockWorkspacePublicationEvents.clear();
  mockChildSessionStatuses.clear();
  mockChildSessionStatusOverrides.clear();
}

export function seedMockChatScenario(name: MockChatScenarioName): void {
  const scenario = getMockChatScenario(name);
  resetMockChatState();

  for (const conversation of scenario.conversations) {
    mockConversations.set(conversation.id, conversation);
  }

  for (const [conversationId, messages] of Object.entries(scenario.messages)) {
    mockMessages.set(
      conversationId,
      messages.map((message) => cloneMockChatMessage(message))
    );
  }

  for (const [key, queued] of Object.entries(scenario.queuedMessages ?? {})) {
    mockQueuedMessages.set(key, [...queued]);
  }

  for (const [sessionId, status] of Object.entries(scenario.childSessionStatuses ?? {})) {
    mockChildSessionStatuses.set(sessionId, { ...status });
  }
}

function cloneConversation(conversation: ChatConversation): ChatConversation {
  return { ...conversation };
}

function refreshConversationMessageStats(conversationId: string): void {
  const conversation = mockConversations.get(conversationId);
  if (!conversation) {
    return;
  }

  const messages = mockMessages.get(conversationId) ?? [];
  const lastMessageAt =
    messages.length > 0
      ? messages[messages.length - 1]?.createdAt ?? conversation.lastMessageAt
      : null;

  mockConversations.set(conversationId, {
    ...conversation,
    messageCount: messages.length,
    lastMessageAt,
    updatedAt: lastMessageAt ?? conversation.updatedAt,
  });
}

export function seedMockConversation(
  conversation: ChatConversation,
  messages: ChatMessageResponse[]
): void {
  mockConversations.set(conversation.id, cloneConversation(conversation));
  mockMessages.set(
    conversation.id,
    messages.map((message) => cloneMockChatMessage(message))
  );
  refreshConversationMessageStats(conversation.id);
}

export function replaceMockConversationMessages(
  conversationId: string,
  messages: ChatMessageResponse[]
): void {
  mockMessages.set(
    conversationId,
    messages.map((message) => cloneMockChatMessage(message))
  );
  refreshConversationMessageStats(conversationId);
}

export function seedMockAgentConversationWorkspace(
  workspace: AgentConversationWorkspace
): void {
  mockWorkspaces.set(workspace.conversationId, { ...workspace });
}

function exposeMockChatController(): void {
  if (typeof window === "undefined") {
    return;
  }

  window.__mockChatApi = {
    reset: resetMockChatState,
    seedScenario: seedMockChatScenario,
    seedConversation: seedMockConversation,
    replaceMessages: replaceMockConversationMessages,
    listScenarios: listMockChatScenarios,
    getChildSessionStatus: mockGetChildSessionStatus,
    setChildSessionStatusOverride: mockSetChildSessionStatusOverride,
    clearChildSessionStatusOverrides: mockClearChildSessionStatusOverrides,
    listConversations: mockListConversations,
    listConversationsPage: mockListConversationsPage,
    listAgentSidebarConversations: mockListAgentSidebarConversations,
    setAgentConversationMuted: mockSetAgentConversationMuted,
    getConversation: mockGetConversation,
    getConversationSummary: mockGetConversationSummary,
    getConversationTimelinePage: mockGetConversationTimelinePage,
    getConversationStats: mockGetConversationStats,
    seedAgentConversationWorkspace: seedMockAgentConversationWorkspace,
  };
}

exposeMockChatController();

// ============================================================================
// Mock Chat API Functions
// ============================================================================

export async function mockListConversations(
  contextType: ContextType,
  contextId: string,
  includeArchived = false,
  archivedOnly = false
): Promise<ChatConversation[]> {
  return Array.from(mockConversations.values()).filter(
    (c) =>
      c.contextType === contextType &&
      c.contextId === contextId &&
      (archivedOnly
        ? Boolean(c.archivedAt)
        : includeArchived || !c.archivedAt)
  );
}

export async function mockListConversationsPage(
  contextType: ContextType,
  contextId: string,
  limit: number,
  offset = 0,
  includeArchived = false,
  search?: string,
  archivedOnly = false
): Promise<ConversationListPageResponse> {
  const normalizedSearch = search?.trim().toLowerCase();
  const conversations = (await mockListConversations(
    contextType,
    contextId,
    includeArchived,
    archivedOnly
  ))
    .filter((conversation) => {
      if (!normalizedSearch) {
        return true;
      }
      return (conversation.title ?? "Untitled agent")
        .toLowerCase()
        .includes(normalizedSearch);
    })
    .sort(
      (left, right) =>
        new Date(right.createdAt).getTime() - new Date(left.createdAt).getTime()
    );
  const pagedConversations = conversations.slice(offset, offset + limit);

  return {
    conversations: pagedConversations,
    limit,
    offset,
    total: conversations.length,
    hasMore: offset + pagedConversations.length < conversations.length,
  };
}

const MOCK_PUBLICATION_STATES: AgentSidebarPublicationState[] = [
  "active",
  "draft",
  "merged",
  "closed",
  "uncommitted",
  "unpushed",
];
const MOCK_INBOX_LANES: AgentSidebarAttentionLane[] = [
  "needs",
  "working",
  "stale",
  "done",
];
const MOCK_INBOX_STALE_AFTER_MS = 7 * 24 * 60 * 60 * 1000;
const MOCK_INBOX_WORKING_SUPERVISION_STATUSES = new Set([
  "fixing",
  "publishing",
  "waiting",
  "waiting_for_checks",
  "monitoring",
]);
const MOCK_INBOX_FIXING_SUPERVISION_STATUSES = new Set([
  "fixing",
  "publishing",
]);
const MOCK_INBOX_WAITING_SUPERVISION_STATUSES = new Set([
  "waiting",
  "waiting_for_checks",
]);

export async function mockListAgentSidebarConversations(
  input: AgentSidebarConversationsInput
): Promise<AgentSidebarConversationGroupsResponse> {
  const projectIds = Array.from(
    new Set(input.projectIds.map((projectId) => projectId.trim()).filter(Boolean))
  );
  const publicationStates = input.publicationStates ?? MOCK_PUBLICATION_STATES;
  const normalizedSearch = input.search?.trim().toLowerCase();
  const includeArchived = input.includeArchived ?? false;
  const archivedOnly = input.archivedOnly ?? false;
  const pinnedConversationIds = new Set(input.pinnedConversationIds ?? []);
  const priorityConversationIds = new Set(input.priorityConversationIds ?? []);

  const rows = projectIds
    .flatMap((projectId) =>
      Array.from(mockConversations.values())
        .filter(
          (conversation) =>
            conversation.contextType === "project" &&
            conversation.contextId === projectId &&
            (archivedOnly
              ? Boolean(conversation.archivedAt)
              : includeArchived || !conversation.archivedAt)
        )
        .filter((conversation) => {
          if (!normalizedSearch) return true;
          return (conversation.title ?? "Untitled agent")
            .toLowerCase()
            .includes(normalizedSearch);
        })
        .map((conversation) => {
          const workspace = mockWorkspaces.get(conversation.id) ?? null;
          const publicationState = getMockPublicationState(workspace);
          const attentionLane = getMockInboxAttentionLane(
            conversation,
            workspace,
            publicationState
          );
          const isMuted = mockMutedConversations.has(conversation.id);
          return {
            conversation,
            workspace,
            refKind:
              workspace?.publicationPrNumber != null
                ? ("pull-request" as const)
                : ("branch" as const),
            refLabel:
              workspace?.publicationPrNumber != null
                ? `PR #${workspace.publicationPrNumber}`
                : workspace?.baseRef || "master",
            publicationState,
            publicationLabel: getMockPublicationLabel(workspace, publicationState),
            attentionLane:
              isMuted && attentionLane === "needs" ? "stale" : attentionLane,
            parkedDelegateCount: 0,
            actionVerb: getMockInboxActionVerb(workspace, publicationState),
            isMuted,
          };
        })
        .filter((row) => publicationStates.includes(row.publicationState))
    )
    .sort((left, right) =>
      compareMockSidebarRows(
        left,
        right,
        input.sort ?? "latest",
        pinnedConversationIds,
        priorityConversationIds
      )
    );

  const groupBy = input.groupBy ?? "project";
  const limit = input.limitPerGroup ?? 6;
  const offsets = input.offsets ?? {};

  if (groupBy === "publication") {
    return {
      groups: publicationStates.map((state) =>
        buildMockSidebarGroup(
          state,
          getMockPublicationGroupLabel(state),
          rows.filter((row) => row.publicationState === state),
          offsets[state] ?? 0,
          limit
        )
      ),
    };
  }

  if (groupBy === "inbox") {
    return {
      groups: MOCK_INBOX_LANES.map((lane) =>
        buildMockSidebarGroup(
          lane,
          getMockInboxGroupLabel(lane),
          rows.filter((row) => row.attentionLane === lane),
          offsets[lane] ?? 0,
          limit
        )
      ),
    };
  }

  return {
    groups: projectIds.map((projectId) =>
      buildMockSidebarGroup(
        projectId,
        projectId,
        rows.filter((row) => row.conversation.contextId === projectId),
        offsets[projectId] ?? 0,
        limit
      )
    ),
  };
}

function compareMockSidebarRows(
  left: AgentSidebarConversationGroupsResponse["groups"][number]["rows"][number],
  right: AgentSidebarConversationGroupsResponse["groups"][number]["rows"][number],
  sort: AgentSidebarSort,
  pinnedConversationIds: Set<string>,
  priorityConversationIds: Set<string>
): number {
  const pinnedDelta =
    Number(pinnedConversationIds.has(right.conversation.id)) -
    Number(pinnedConversationIds.has(left.conversation.id));
  if (pinnedDelta !== 0) return pinnedDelta;
  const priorityDelta =
    Number(priorityConversationIds.has(right.conversation.id)) -
    Number(priorityConversationIds.has(left.conversation.id));
  if (priorityDelta !== 0) return priorityDelta;

  if (sort === "az" || sort === "za") {
    const leftTitle = (left.conversation.title ?? "Untitled agent").toLowerCase();
    const rightTitle = (right.conversation.title ?? "Untitled agent").toLowerCase();
    const titleDelta = leftTitle.localeCompare(rightTitle);
    if (titleDelta !== 0) {
      return sort === "az" ? titleDelta : -titleDelta;
    }
  }

  return (
    new Date(right.conversation.createdAt).getTime() -
    new Date(left.conversation.createdAt).getTime()
  );
}

function buildMockSidebarGroup(
  key: string,
  label: string,
  rows: AgentSidebarConversationGroupsResponse["groups"][number]["rows"],
  offset: number,
  limit: number
): AgentSidebarConversationGroupsResponse["groups"][number] {
  const pagedRows = rows.slice(offset, offset + limit);
  return {
    key,
    label,
    total: rows.length,
    offset,
    limit,
    hasMore: offset + pagedRows.length < rows.length,
    rows: pagedRows,
  };
}

function getMockPublicationState(
  workspace: AgentConversationWorkspace | null
): AgentSidebarPublicationState {
  const prStatus = workspace?.publicationPrStatus?.trim().toLowerCase();
  const pushStatus = workspace?.publicationPushStatus?.trim().toLowerCase();

  if (prStatus === "merged") return "merged";
  if (prStatus === "closed") return "closed";
  if (pushStatus === "needs_agent") return "uncommitted";
  if (
    pushStatus === "pending" ||
    pushStatus === "failed" ||
    pushStatus === "description_failed"
  ) {
    return "unpushed";
  }
  if (prStatus === "draft") return "draft";
  return "active";
}

function getMockPublicationLabel(
  workspace: AgentConversationWorkspace | null,
  state: AgentSidebarPublicationState
): string | null {
  if (state === "active" || state === "uncommitted" || state === "unpushed") {
    const supervisionLabel = getMockSupervisionPublicationLabel(workspace);
    if (supervisionLabel) {
      return supervisionLabel;
    }
  }
  return state === "active" ? null : getMockPublicationGroupLabel(state).toLowerCase();
}

function getMockInboxAttentionLane(
  conversation: ChatConversation,
  workspace: AgentConversationWorkspace | null,
  publicationState: AgentSidebarPublicationState
): AgentSidebarAttentionLane {
  if (
    conversation.archivedAt ||
    publicationState === "merged" ||
    publicationState === "closed"
  ) {
    return "done";
  }

  const supervisionStatus = workspace?.prSupervisionStatus?.trim().toLowerCase();
  if (
    supervisionStatus &&
    MOCK_INBOX_WORKING_SUPERVISION_STATUSES.has(supervisionStatus)
  ) {
    return "working";
  }

  const lastActivityAt = conversation.lastMessageAt ?? conversation.updatedAt;
  if (Date.now() - new Date(lastActivityAt).getTime() > MOCK_INBOX_STALE_AFTER_MS) {
    return "stale";
  }

  return "needs";
}

function getMockInboxGroupLabel(lane: AgentSidebarAttentionLane): string {
  switch (lane) {
    case "needs":
      return "Needs you";
    case "working":
      return "Working";
    case "stale":
      return "Stale";
    case "done":
      return "Done";
  }
}

function getMockInboxActionVerb(
  workspace: AgentConversationWorkspace | null,
  publicationState: AgentSidebarPublicationState
): string {
  const supervisionStatus = workspace?.prSupervisionStatus?.trim().toLowerCase();

  if (publicationState === "merged") return "Merged";
  if (publicationState === "closed") return "Closed";
  if (
    supervisionStatus &&
    MOCK_INBOX_FIXING_SUPERVISION_STATUSES.has(supervisionStatus)
  ) {
    return "Fixing";
  }
  if (
    supervisionStatus &&
    MOCK_INBOX_WAITING_SUPERVISION_STATUSES.has(supervisionStatus)
  ) {
    return "Waiting for checks";
  }
  if (supervisionStatus === "monitoring" && workspace?.prAutoMergeCurrent === true) {
    return "Auto-merging";
  }
  if (supervisionStatus === "blocked") return "Unblock";
  if (publicationState === "uncommitted") return "Commit changes";
  if (publicationState === "unpushed") return "Push changes";
  if (publicationState === "draft") return "Publish";
  if (publicationState === "active" && workspace?.publicationPrNumber != null) {
    return "Review";
  }
  return "Continue";
}

function getMockPublicationGroupLabel(state: AgentSidebarPublicationState): string {
  switch (state) {
    case "active":
      return "Active";
    case "draft":
      return "Draft";
    case "merged":
      return "Merged";
    case "closed":
      return "Closed";
    case "uncommitted":
      return "Uncommitted";
    case "unpushed":
      return "Unpushed";
  }
}

function getMockSupervisionPublicationLabel(
  workspace: AgentConversationWorkspace | null
): string | null {
  const status = workspace?.prSupervisionStatus?.trim().toLowerCase();
  if (status === "fixing" || status === "publishing") return "fixing";
  if (status === "blocked") return "blocked";
  if (status === "waiting" || status === "waiting_for_checks") return "waiting";
  if (status === "monitoring" && workspace?.prAutoMergeCurrent === true) {
    return "auto-merge";
  }
  return null;
}

export async function mockGetConversation(
  conversationId: string
): Promise<{ conversation: ChatConversation; messages: ChatMessageResponse[] }> {
  const conversation = mockConversations.get(conversationId);
  if (!conversation) {
    // Return a new empty conversation
    const newConversation: ChatConversation = {
      id: conversationId,
      contextType: "project",
      contextId: "mock-project",
      ...normalizeConversationProviderMetadata({}),
      coordinationMode: "solo",
      title: null,
      messageCount: 0,
      lastMessageAt: null,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      archivedAt: null,
    };
    return { conversation: newConversation, messages: [] };
  }
  return {
    conversation,
    messages: mockMessages.get(conversationId) ?? [],
  };
}

export async function mockGetConversationSummary(
  conversationId: string
): Promise<ChatConversation | null> {
  return (await mockGetConversation(conversationId)).conversation;
}

function normalizeMockContentBlocks(message: ChatMessageResponse): MockContentBlock[] {
  if (Array.isArray(message.contentBlocks) && message.contentBlocks.length > 0) {
    return message.contentBlocks;
  }

  if (message.content.trim().length === 0) {
    return [];
  }

  return [{ type: "text", text: message.content }];
}

function mockTimelineToolCallFromBlock(block: Record<string, unknown>, index: number) {
  const id = typeof block.id === "string" ? block.id : `tool-${index}`;
  const name = typeof block.name === "string" ? block.name : "unknown";
  const toolCall: NonNullable<ChatTimelineItemResponse["toolCall"]> = {
    id,
    name,
    arguments: block.arguments ?? block.input ?? {},
  };
  if ("result" in block) {
    toolCall.result = block.result;
  }
  const parentToolUseId = block.parentToolUseId ?? block.parent_tool_use_id;
  if (typeof parentToolUseId === "string") {
    toolCall.parentToolUseId = parentToolUseId;
  }
  if (typeof block.error === "string") {
    toolCall.error = block.error;
  }
  return toolCall;
}

function mockTimelineItemsForMessages(
  conversationId: string,
  messages: ChatMessageResponse[]
): ChatTimelineItemResponse[] {
  let sequence = 0;
  return messages.flatMap((message) => {
    const blocks = normalizeMockContentBlocks(message);
    return blocks.map((block, blockIndex) => {
      sequence += 1;
      const blockRecord =
        block != null && typeof block === "object"
          ? (block as unknown as Record<string, unknown>)
          : {};
      const isToolCall = blockRecord.type === "tool_use";
      const status =
        message.timelineStatus ??
        (message.id.includes("live") ? "streaming" : "finalized");
      const carriesParentUsage = blockIndex === 0 && message.role !== "user";
      const toolCall = isToolCall
        ? mockTimelineToolCallFromBlock(blockRecord, blockIndex)
        : null;
      const text =
        typeof blockRecord.text === "string"
          ? blockRecord.text
          : isToolCall
            ? ""
            : message.content;
      const contentBlocks: MockContentBlock[] = [block];
      const blockIdentity = isToolCall && toolCall?.id
        ? toolCall.id
        : String(blockIndex);
      const asMessage: ChatMessageResponse = {
        ...message,
        id: `block:${message.id}:${blockIdentity}`,
        content: text,
        parentMessageId: message.id,
        conversationId,
        toolCalls: toolCall ? [toolCall] : null,
        contentBlocks,
        inputTokens: carriesParentUsage ? message.inputTokens ?? null : null,
        outputTokens: carriesParentUsage ? message.outputTokens ?? null : null,
        cacheCreationTokens: carriesParentUsage ? message.cacheCreationTokens ?? null : null,
        cacheReadTokens: carriesParentUsage ? message.cacheReadTokens ?? null : null,
        estimatedUsd: carriesParentUsage ? message.estimatedUsd ?? null : null,
        effectiveModelId: carriesParentUsage ? message.effectiveModelId ?? null : null,
        logicalModel: carriesParentUsage ? message.logicalModel ?? null : null,
        effectiveEffort: carriesParentUsage ? message.effectiveEffort ?? null : null,
        logicalEffort: carriesParentUsage ? message.logicalEffort ?? null : null,
        timelineStatus: status,
        timelineKind: isToolCall ? "tool_use" : "text",
        timelineSequence: sequence,
        timelineBlockIndex: blockIndex,
      };

      return {
        id: asMessage.id,
        conversationId,
        messageId: message.id,
        runId: null,
        sequence,
        blockIndex,
        role: message.role,
        kind: asMessage.timelineKind ?? "text",
        status,
        content: text,
        contentBlocks,
        toolCall,
        metadata: message.metadata,
        providerHarness: message.providerHarness ?? null,
        providerSessionId: message.providerSessionId ?? null,
        upstreamProvider: message.upstreamProvider ?? null,
        providerProfile: message.providerProfile ?? null,
        logicalModel: carriesParentUsage ? message.logicalModel ?? null : null,
        effectiveModelId: carriesParentUsage ? message.effectiveModelId ?? null : null,
        logicalEffort: carriesParentUsage ? message.logicalEffort ?? null : null,
        effectiveEffort: carriesParentUsage ? message.effectiveEffort ?? null : null,
        inputTokens: carriesParentUsage ? message.inputTokens ?? null : null,
        outputTokens: carriesParentUsage ? message.outputTokens ?? null : null,
        cacheCreationTokens: carriesParentUsage ? message.cacheCreationTokens ?? null : null,
        cacheReadTokens: carriesParentUsage ? message.cacheReadTokens ?? null : null,
        estimatedUsd: carriesParentUsage ? message.estimatedUsd ?? null : null,
        createdAt: message.createdAt,
        updatedAt: message.createdAt,
        finalizedAt: status === "streaming" ? null : message.createdAt,
        asMessage,
      };
    });
  });
}

export async function mockGetConversationTimelinePage(
  conversationId: string,
  limit: number,
  beforeSequence: number | null = null
): Promise<ConversationTimelinePageResponse> {
  const { conversation, messages } = await mockGetConversation(conversationId);
  const allItems = mockTimelineItemsForMessages(conversationId, messages);
  const eligibleItems =
    beforeSequence == null
      ? allItems
      : allItems.filter((item) => item.sequence < beforeSequence);
  const start = Math.max(0, eligibleItems.length - limit);
  const items = eligibleItems.slice(start);

  return {
    conversation,
    items,
    messages: items.map((item) => item.asMessage),
    limit,
    beforeSequence,
    totalItemCount: allItems.length,
    hasOlder: start > 0,
    oldestLoadedSequence: items[0]?.sequence ?? null,
    newestLoadedSequence: items[items.length - 1]?.sequence ?? null,
  };
}

export async function mockGetConversationStats(
  conversationId: string
): Promise<ConversationStatsResponse | null> {
  const conversation = mockConversations.get(conversationId);
  if (!conversation) {
    return null;
  }

  return buildFallbackConversationStats(
    conversation,
    mockMessages.get(conversationId) ?? []
  );
}

export async function mockCreateConversation(
  contextType: ContextType,
  contextId?: string | null,
  title?: string
): Promise<ChatConversation> {
  const id = generateTestUuid();
  const conversation: ChatConversation = {
    id,
    contextType,
    contextId: contextId ?? id,
    ...normalizeConversationProviderMetadata({}),
    coordinationMode: "solo",
    title: title?.trim() || null,
    messageCount: 0,
    lastMessageAt: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    archivedAt: null,
  };
  mockConversations.set(conversation.id, conversation);
  return conversation;
}

export async function mockUpdateConversationTitle(
  conversationId: string,
  title: string
): Promise<ChatConversation> {
  const conversation = mockConversations.get(conversationId);
  if (!conversation) {
    throw new Error(`Conversation ${conversationId} not found`);
  }
  const updated = {
    ...conversation,
    title: title.trim(),
    updatedAt: new Date().toISOString(),
  };
  mockConversations.set(conversationId, updated);
  return cloneConversation(updated);
}

export async function mockArchiveConversation(
  conversationId: string,
  _options: { closePullRequest: boolean }
): Promise<ArchiveConversationResult> {
  const conversation = mockConversations.get(conversationId);
  if (!conversation) {
    throw new Error(`Conversation ${conversationId} not found`);
  }
  const updated = {
    ...conversation,
    archivedAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
  mockConversations.set(conversationId, updated);
  return {
    conversation: cloneConversation(updated),
    cleanup: {
      runtimeShutdownSucceeded: true,
      cleanupClaim: "claimed",
      localCleanup: "cleaned",
      message: null,
    },
  };
}

export async function mockRestoreConversation(
  conversationId: string
): Promise<ChatConversation> {
  const conversation = mockConversations.get(conversationId);
  if (!conversation) {
    throw new Error(`Conversation ${conversationId} not found`);
  }
  const updated = {
    ...conversation,
    archivedAt: null,
    updatedAt: new Date().toISOString(),
  };
  mockConversations.set(conversationId, updated);
  return cloneConversation(updated);
}

export async function mockSetAgentConversationMuted(
  conversationId: string,
  muted: boolean,
): Promise<void> {
  if (!mockConversations.has(conversationId)) {
    throw new Error(`Conversation ${conversationId} not found`);
  }

  if (muted) {
    mockMutedConversations.add(conversationId);
  } else {
    mockMutedConversations.delete(conversationId);
  }
}

function cloneChildSessionStatus(
  status: ChildSessionStatusResponse
): ChildSessionStatusResponse {
  return {
    ...status,
    agent_state: { ...status.agent_state },
    recent_messages: status.recent_messages.map((message) => ({ ...message })),
  };
}

function mockSetChildSessionStatusOverride(
  sessionId: string,
  override: MockChildSessionStatusOverride
): void {
  mockChildSessionStatusOverrides.set(sessionId, override);
}

function mockClearChildSessionStatusOverrides(): void {
  mockChildSessionStatusOverrides.clear();
}

export async function mockGetChildSessionStatus(
  sessionId: string
): Promise<ChildSessionStatusResponse> {
  const override = mockChildSessionStatusOverrides.get(sessionId);
  const delayMs = override?.delayMs ?? 0;

  if (delayMs > 0) {
    await new Promise((resolve) => globalThis.setTimeout(resolve, delayMs));
  }

  if (override?.error) {
    throw new Error(override.error);
  }

  const response = override?.response ?? mockChildSessionStatuses.get(sessionId);
  if (!response) {
    throw new Error(`No mock child session status seeded for ${sessionId}`);
  }

  return cloneChildSessionStatus(response);
}

export async function mockGetAgentRunStatus(
  _conversationId: string
): Promise<null> {
  // No agent runs in mock mode
  return null;
}

type MockConversationRuntimeInput = Pick<
  StartAgentConversationInput,
  "providerHarness" | "modelId" | "logicalEffort"
>;

function applyMockConversationRuntime(
  conversation: ChatConversation,
  input: MockConversationRuntimeInput,
): ChatConversation {
  return {
    ...conversation,
    ...(input.providerHarness !== undefined
      ? { providerHarness: input.providerHarness }
      : {}),
    ...(input.modelId
      ? { logicalModel: input.modelId, effectiveModelId: input.modelId }
      : {}),
    ...(input.logicalEffort
      ? {
          logicalEffort: input.logicalEffort,
          effectiveEffort: input.logicalEffort,
        }
      : {}),
  };
}

export async function mockSendAgentMessage(
  contextType: ContextType,
  contextId: string,
  _content: string,
  _attachmentIds?: string[],
  _target?: string,
  options?: SendAgentMessageOptions,
): Promise<SendAgentMessageResult> {
  // Find or create conversation
  let conversation = options?.conversationId
    ? mockConversations.get(options.conversationId)
    : Array.from(mockConversations.values()).find(
        (c) => c.contextType === contextType && c.contextId === contextId
      );

  const isNew = !conversation;
  if (!conversation) {
    conversation = await mockCreateConversation(contextType, contextId);
  }
  if (options) {
    conversation = {
      ...applyMockConversationRuntime(conversation, options),
      coordinationMode:
        options.capabilityIntent?.coordinationMode ??
        options.teamIntent?.coordinationMode ??
        conversation.coordinationMode,
      updatedAt: new Date().toISOString(),
    };
    mockConversations.set(conversation.id, conversation);
  }

  return {
    conversationId: conversation.id,
    agentRunId: generateTestUuid(),
    isNewConversation: isNew,
    wasQueued: false,
    queuedAsPending: false,
  };
}

export async function mockStartAgentConversation(
  input: StartAgentConversationInput
): Promise<StartAgentConversationResult> {
  const contextType = input.projectId ? "project" : "standalone";
  const contextId = input.projectId ?? null;
  const conversation = input.conversationId
    ? mockConversations.get(input.conversationId) ??
      (await mockCreateConversation(contextType, contextId))
    : await mockCreateConversation(contextType, contextId);
  const mode = input.mode ?? "edit";
  const modeConversation: ChatConversation = {
    ...applyMockConversationRuntime(conversation, input),
    agentMode: mode,
    coordinationMode:
      input.capabilityIntent?.coordinationMode ??
      input.teamIntent?.coordinationMode ??
      conversation.coordinationMode,
    updatedAt: new Date().toISOString(),
  };
  mockConversations.set(conversation.id, modeConversation);
  const sendResult: SendAgentMessageResult = {
    conversationId: conversation.id,
    agentRunId: generateTestUuid(),
    isNewConversation: !input.conversationId,
    wasQueued: false,
    queuedAsPending: false,
  };

  const workspace = input.projectId
    ? createMockWorkspace(modeConversation, input.projectId, mode, input.base)
    : null;
  if (workspace) {
    mockWorkspaces.set(conversation.id, workspace);
  }

  return {
    conversation: modeConversation,
    workspace,
    sendResult,
  };
}

export async function mockSwitchAgentConversationMode(
  input: SwitchAgentConversationModeInput
): Promise<SwitchAgentConversationModeResult> {
  const conversation = mockConversations.get(input.conversationId);
  if (!conversation) {
    throw new Error(`No mock conversation seeded for ${input.conversationId}`);
  }
  const updatedConversation: ChatConversation = {
    ...conversation,
    agentMode: input.mode,
    providerSessionId: null,
    providerHarness: null,
    claudeSessionId: null,
    updatedAt: new Date().toISOString(),
  };
  mockConversations.set(input.conversationId, updatedConversation);

  let workspace = mockWorkspaces.get(input.conversationId) ?? null;
  workspace = workspace
    ? { ...workspace, mode: input.mode, updatedAt: updatedConversation.updatedAt }
    : createMockWorkspace(
        updatedConversation,
        updatedConversation.contextId,
        input.mode,
        input.base
      );
  mockWorkspaces.set(input.conversationId, workspace);

  return {
    conversation: updatedConversation,
    workspace,
  };
}

export async function mockUpdateAgentConversationCoordinationMode(
  input: UpdateAgentConversationCoordinationModeInput
): Promise<ChatConversation> {
  const conversation = mockConversations.get(input.conversationId);
  if (!conversation) {
    throw new Error(`No mock conversation seeded for ${input.conversationId}`);
  }
  const updatedConversation: ChatConversation = {
    ...conversation,
    coordinationMode: input.coordinationMode,
    updatedAt: new Date().toISOString(),
  };
  mockConversations.set(input.conversationId, updatedConversation);
  return cloneConversation(updatedConversation);
}

function createMockWorkspace(
  conversation: ChatConversation,
  projectId: string,
  mode: Exclude<StartAgentConversationInput["mode"], undefined>,
  base: StartAgentConversationInput["base"]
): AgentConversationWorkspace {
  return {
    conversationId: conversation.id,
    projectId,
    mode,
    branchMode: base?.branchMode ?? "isolated",
    baseRefKind: base?.kind ?? "project_default",
    baseRef: base?.ref ?? "main",
    baseDisplayName: base?.displayName ?? null,
    baseCommit: null,
    branchName: `ralphx/mock/agent-${conversation.id.slice(0, 8)}`,
    worktreePath: `/tmp/ralphx/mock/${conversation.id}`,
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: null,
    publicationMetadataAttemptId: null,
    publicationMetadataPhase: null,
    publicationMetadataState: null,
    status: "active",
    createdAt: conversation.createdAt,
    updatedAt: conversation.updatedAt,
  };
}

export async function mockGetAgentConversationWorkspace(
  conversationId: string
): Promise<AgentConversationWorkspace | null> {
  return mockWorkspaces.get(conversationId) ?? null;
}

export async function mockListAgentConversationWorkspacesByProject(
  projectId: string
): Promise<AgentConversationWorkspace[]> {
  return Array.from(mockWorkspaces.values()).filter(
    (workspace) => workspace.projectId === projectId
  );
}

export async function mockListAgentConversationWorkspacePublicationEvents(
  conversationId: string
): Promise<AgentConversationWorkspacePublicationEvent[]> {
  return mockWorkspacePublicationEvents.get(conversationId) ?? [];
}

export async function mockPublishAgentConversationWorkspace(
  conversationId: string
): Promise<PublishAgentConversationWorkspaceResult> {
  const workspace = mockWorkspaces.get(conversationId);
  if (!workspace) {
    throw new Error(`No mock workspace seeded for ${conversationId}`);
  }
  const published: AgentConversationWorkspace = {
    ...workspace,
    publicationPrNumber: workspace.publicationPrNumber ?? 42,
    publicationPrUrl:
      workspace.publicationPrUrl ?? "https://github.com/mock/project/pull/42",
    publicationPrStatus: workspace.publicationPrStatus ?? "draft",
    publicationPushStatus: "pushed",
    updatedAt: new Date().toISOString(),
  };
  mockWorkspaces.set(conversationId, published);
  mockWorkspacePublicationEvents.set(conversationId, [
    ...(mockWorkspacePublicationEvents.get(conversationId) ?? []),
    {
      id: `event-${mockWorkspacePublicationEvents.get(conversationId)?.length ?? 0}`,
      conversationId,
      step: "published",
      status: "succeeded",
      summary: "Draft pull request is ready",
      classification: null,
      attemptId: null,
      createdAt: new Date().toISOString(),
    },
  ]);
  return {
    workspace: published,
    commitSha: "mockcommit",
    pushed: true,
    createdPr: workspace.publicationPrNumber == null,
    prNumber: published.publicationPrNumber,
    prUrl: published.publicationPrUrl,
  };
}

export async function mockReconcileAgentConversationWorkspacePublication(
  _conversationId: string
): Promise<void> {
  return undefined;
}

export async function mockSetAgentConversationWorkspacePrSupervision(
  conversationId: string,
  input: SetAgentConversationWorkspacePrSupervisionInput
): Promise<AgentConversationWorkspace> {
  const workspace = mockWorkspaces.get(conversationId);
  if (!workspace) {
    throw new Error(`No mock workspace seeded for ${conversationId}`);
  }
  const updated: AgentConversationWorkspace = {
    ...workspace,
    prAutofixEnabled: input.autoFixEnabled,
    prAutoMergeDesired: input.autoMergeDesired,
    prAutoMergeMethod: input.autoMergeMethod ?? workspace.prAutoMergeMethod ?? "squash",
    prSupervisionStatus:
      input.autoFixEnabled || input.autoMergeDesired ? "monitoring" : "disabled",
    prSupervisionSummary:
      input.autoFixEnabled || input.autoMergeDesired
        ? "RalphX PR supervision is enabled."
        : null,
    prSupervisionUpdatedAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
  mockWorkspaces.set(conversationId, updated);
  return updated;
}

export async function mockPrecomputeAgentConversationWorkspacePrDescription(
  conversationId: string
): Promise<PrecomputeAgentConversationWorkspacePrDescriptionResult> {
  return {
    conversationId,
    status: mockWorkspaces.has(conversationId) ? "ready" : "skipped",
    cacheStatus: mockWorkspaces.has(conversationId) ? "miss" : null,
    reason: mockWorkspaces.has(conversationId) ? null : "missing_workspace",
  };
}

export async function mockGetQueuedAgentMessages(
  contextType: ContextType,
  contextId: string
): Promise<QueuedMessageResponse[]> {
  const key = `${contextType}:${contextId}`;
  return mockQueuedMessages.get(key) ?? [];
}

export async function mockDeleteQueuedAgentMessage(
  contextType: ContextType,
  contextId: string,
  messageId: string
): Promise<boolean> {
  const key = `${contextType}:${contextId}`;
  const existing = mockQueuedMessages.get(key) ?? [];
  const filtered = existing.filter((m) => m.id !== messageId);
  mockQueuedMessages.set(key, filtered);
  return existing.length !== filtered.length;
}

export async function mockSendQueuedAgentMessageNow(
  contextType: ContextType,
  contextId: string,
  messageId: string
): Promise<SendAgentMessageResult> {
  const key = `${contextType}:${contextId}`;
  const existing = mockQueuedMessages.get(key) ?? [];
  const selected = existing.find((message) => message.id === messageId);
  mockQueuedMessages.set(
    key,
    existing.filter((message) => message.id !== messageId)
  );
  return {
    conversationId: contextId,
    agentRunId: `mock-run-${messageId}`,
    isNewConversation: false,
    wasQueued: false,
    queuedAsPending: false,
    queuedMessageId: selected ? null : undefined,
  };
}

export async function mockIsChatServiceAvailable(): Promise<boolean> {
  // Chat is not available in mock mode
  return false;
}

export async function mockStopAgent(
  _contextType: ContextType,
  _contextId: string
): Promise<boolean> {
  // No agent to stop in mock mode
  return false;
}

export async function mockIsAgentRunning(
  _contextType: ContextType,
  _contextId: string
): Promise<boolean> {
  // No agents running in mock mode
  return false;
}

export async function mockGetAgentRunningStates(
  _contextType: ContextType,
  contextIds: string[]
): Promise<Record<string, { isRunning: boolean; agentStatus: "idle" }>> {
  return Object.fromEntries(
    contextIds.map((contextId) => [
      contextId,
      { isRunning: false, agentStatus: "idle" },
    ])
  );
}

export async function mockGetAgentConversationRuntimeStatuses(
  conversationIds: string[],
): Promise<Record<string, AgentConversationRuntimeStatus>> {
  return Object.fromEntries(
    conversationIds.map((conversationId) => [
      conversationId,
      {
        conversationId,
        isRunning: false,
        agentStatus: "idle",
        primarySource: null,
        summaryLabel: null,
        items: [],
      },
    ]),
  );
}

export async function mockGetBulkWorkspacePublicationStates(
  conversationIds: string[]
): Promise<Record<string, { publication_state: string; publication_label: string | null }>> {
  return Object.fromEntries(
    conversationIds.map((id) => [id, { publication_state: "active", publication_label: null }])
  );
}

// ============================================================================
// Mock Chat API Object
// ============================================================================

export const mockChatApi = {
  reset: resetMockChatState,
  seedScenario: seedMockChatScenario,
  seedConversation: seedMockConversation,
  replaceMessages: replaceMockConversationMessages,
  listConversations: mockListConversations,
  listConversationsPage: mockListConversationsPage,
  getConversation: mockGetConversation,
  getConversationSummary: mockGetConversationSummary,
  getConversationTimelinePage: mockGetConversationTimelinePage,
  createConversation: mockCreateConversation,
  updateConversationTitle: mockUpdateConversationTitle,
  archiveConversation: mockArchiveConversation,
  restoreConversation: mockRestoreConversation,
  setAgentConversationMuted: mockSetAgentConversationMuted,
  getChildSessionStatus: mockGetChildSessionStatus,
  getAgentRunStatus: mockGetAgentRunStatus,
  getAgentConversationWorkspace: mockGetAgentConversationWorkspace,
  listAgentConversationWorkspacesByProject:
    mockListAgentConversationWorkspacesByProject,
  listAgentSidebarConversations: mockListAgentSidebarConversations,
  listAgentConversationWorkspacePublicationEvents:
    mockListAgentConversationWorkspacePublicationEvents,
  precomputeAgentConversationWorkspacePrDescription:
    mockPrecomputeAgentConversationWorkspacePrDescription,
  reconcileAgentConversationWorkspacePublication:
    mockReconcileAgentConversationWorkspacePublication,
  publishAgentConversationWorkspace: mockPublishAgentConversationWorkspace,
  setAgentConversationWorkspacePrSupervision:
    mockSetAgentConversationWorkspacePrSupervision,
  startAgentConversation: mockStartAgentConversation,
  switchAgentConversationMode: mockSwitchAgentConversationMode,
  updateAgentConversationCoordinationMode:
    mockUpdateAgentConversationCoordinationMode,
  sendAgentMessage: mockSendAgentMessage,
  getQueuedAgentMessages: mockGetQueuedAgentMessages,
  deleteQueuedAgentMessage: mockDeleteQueuedAgentMessage,
  sendQueuedAgentMessageNow: mockSendQueuedAgentMessageNow,
  isChatServiceAvailable: mockIsChatServiceAvailable,
  stopAgent: mockStopAgent,
  isAgentRunning: mockIsAgentRunning,
  getAgentRunningStates: mockGetAgentRunningStates,
  getAgentConversationRuntimeStatuses: mockGetAgentConversationRuntimeStatuses,
  getBulkWorkspacePublicationStates: mockGetBulkWorkspacePublicationStates,
} as const;
