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
  ChildSessionStatusResponse,
  ConversationListPageResponse,
  ConversationStatsResponse,
  AgentConversationWorkspace,
  AgentConversationWorkspacePublicationEvent,
  PublishAgentConversationWorkspaceResult,
  QueuedMessageResponse,
  SendAgentMessageResult,
  StartAgentConversationInput,
  StartAgentConversationResult,
  SwitchAgentConversationModeInput,
  SwitchAgentConversationModeResult,
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
  getConversation(
    conversationId: string
  ): Promise<{ conversation: ChatConversation; messages: ChatMessageResponse[] }>;
  getConversationStats(
    conversationId: string
  ): Promise<ConversationStatsResponse | null>;
  seedAgentConversationWorkspace(workspace: AgentConversationWorkspace): void;
}

export function resetMockChatState(): void {
  mockConversations.clear();
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
    getConversation: mockGetConversation,
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
  contextId: string,
  title?: string
): Promise<ChatConversation> {
  const conversation: ChatConversation = {
    id: generateTestUuid(),
    contextType,
    contextId,
    ...normalizeConversationProviderMetadata({}),
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
  conversationId: string
): Promise<ChatConversation> {
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
  return cloneConversation(updated);
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

export async function mockSendAgentMessage(
  contextType: ContextType,
  contextId: string,
  _content: string
): Promise<SendAgentMessageResult> {
  // Find or create conversation
  let conversation = Array.from(mockConversations.values()).find(
    (c) => c.contextType === contextType && c.contextId === contextId
  );

  const isNew = !conversation;
  if (!conversation) {
    conversation = await mockCreateConversation(contextType, contextId);
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
  const conversation = input.conversationId
    ? mockConversations.get(input.conversationId) ??
      (await mockCreateConversation("project", input.projectId))
    : await mockCreateConversation("project", input.projectId);
  const mode = input.mode ?? "edit";
  const modeConversation: ChatConversation = {
    ...conversation,
    agentMode: mode,
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

  const workspace = createMockWorkspace(modeConversation, input.projectId, mode, input.base);
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
  createConversation: mockCreateConversation,
  updateConversationTitle: mockUpdateConversationTitle,
  archiveConversation: mockArchiveConversation,
  restoreConversation: mockRestoreConversation,
  getChildSessionStatus: mockGetChildSessionStatus,
  getAgentRunStatus: mockGetAgentRunStatus,
  getAgentConversationWorkspace: mockGetAgentConversationWorkspace,
  listAgentConversationWorkspacesByProject:
    mockListAgentConversationWorkspacesByProject,
  listAgentConversationWorkspacePublicationEvents:
    mockListAgentConversationWorkspacePublicationEvents,
  publishAgentConversationWorkspace: mockPublishAgentConversationWorkspace,
  startAgentConversation: mockStartAgentConversation,
  switchAgentConversationMode: mockSwitchAgentConversationMode,
  sendAgentMessage: mockSendAgentMessage,
  getQueuedAgentMessages: mockGetQueuedAgentMessages,
  deleteQueuedAgentMessage: mockDeleteQueuedAgentMessage,
  isChatServiceAvailable: mockIsChatServiceAvailable,
  stopAgent: mockStopAgent,
  isAgentRunning: mockIsAgentRunning,
} as const;
