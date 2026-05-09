import { expect, test, type Page } from "@playwright/test";
import { setupApp } from "../../../fixtures/setup.fixtures";

const contextId = "session-chat-contract";
const conversationId = "conv-chat-contract";

const baseConversation = {
  id: conversationId,
  contextType: "ideation",
  contextId,
  claudeSessionId: null,
  providerSessionId: "thread-chat-contract",
  providerHarness: "codex",
  upstreamProvider: "openai",
  providerProfile: null,
  title: "Contract Session",
  messageCount: 0,
  lastMessageAt: null,
  createdAt: "2026-04-10T10:00:00.000Z",
  updatedAt: "2026-04-10T10:00:00.000Z",
} as const;

const userMessage = {
  id: "msg-user-1",
  sessionId: contextId,
  projectId: "project-mock-1",
  taskId: null,
  role: "user",
  content: "Hello there, this is a test message",
  metadata: null,
  parentMessageId: null,
  conversationId,
  toolCalls: null,
  contentBlocks: null,
  sender: null,
  attributionSource: "native",
  providerHarness: null,
  providerSessionId: null,
  upstreamProvider: null,
  providerProfile: null,
  logicalModel: null,
  effectiveModelId: null,
  logicalEffort: null,
  effectiveEffort: null,
  inputTokens: null,
  outputTokens: null,
  cacheCreationTokens: null,
  cacheReadTokens: null,
  estimatedUsd: null,
  createdAt: "2026-04-10T10:00:00.000Z",
};

const liveProviderMessage = {
  id: "msg-provider-live-1",
  sessionId: contextId,
  projectId: "project-mock-1",
  taskId: null,
  role: "orchestrator",
  content: "I am preparing the plan now.",
  metadata: null,
  parentMessageId: "msg-user-1",
  conversationId,
  toolCalls: [
    {
      id: "tool-session-plan-1",
      name: "ralphx::get_session_plan",
      arguments: { session_id: contextId },
      result: { status: "ok" },
    },
  ],
  contentBlocks: [
    { type: "text", text: "I am preparing the plan now." },
    {
      type: "tool_use",
      id: "tool-session-plan-1",
      name: "ralphx::get_session_plan",
      arguments: { session_id: contextId },
      result: { status: "ok" },
    },
  ],
  sender: null,
  attributionSource: "native",
  providerHarness: "codex",
  providerSessionId: "thread-chat-contract",
  upstreamProvider: "openai",
  providerProfile: null,
  logicalModel: "gpt-5.4",
  effectiveModelId: "gpt-5.4",
  logicalEffort: "xhigh",
  effectiveEffort: "xhigh",
  inputTokens: 1250,
  outputTokens: 140,
  cacheCreationTokens: 80,
  cacheReadTokens: 900,
  estimatedUsd: null,
  createdAt: "2026-04-10T10:01:00.000Z",
};

const liveProviderMessageWithUsage = {
  ...liveProviderMessage,
  inputTokens: 76286,
  outputTokens: 12148,
  cacheCreationTokens: 12000,
  cacheReadTokens: 37920,
};

const finalizedProviderMessage = {
  ...liveProviderMessage,
  id: "msg-provider-final-1",
  content: "I am preparing the plan now.\n\nHere is the final plan summary.",
  contentBlocks: [
    { type: "text", text: "I am preparing the plan now." },
    {
      type: "tool_use",
      id: "tool-session-plan-1",
      name: "ralphx::get_session_plan",
      arguments: { session_id: contextId },
      result: { status: "ok" },
    },
    { type: "text", text: "Here is the final plan summary." },
  ],
  outputTokens: 210,
  createdAt: "2026-04-10T10:02:00.000Z",
};

const erroredProviderMessage = {
  ...finalizedProviderMessage,
  content:
    "I am preparing the plan now.\n\nHere is the final plan summary.\n\n[Agent error: user cancelled MCP tool call]",
};

async function seedIdeationConversation(
  page: Page,
  messages: Array<typeof userMessage | typeof liveProviderMessage>
) {
  await setupApp(page);
  await page.click('[data-testid="nav-ideation"]');
  await page.waitForSelector('[data-testid="ideation-view"]', { timeout: 10000 });

  await page.evaluate(async ({ conversation, seededMessages, sessionId }) => {
    const mockChatApi = window.__mockChatApi;
    const queryClient = window.__queryClient;
    const chatStore = window.__chatStore;
    const ideationStore = window.__ideationStore;

    if (!mockChatApi || !queryClient || !chatStore || !ideationStore) {
      throw new Error("Expected mock chat app globals to be available");
    }

    mockChatApi.reset();
    mockChatApi.seedConversation(conversation, seededMessages);

    ideationStore.getState().selectSession({
      id: sessionId,
      projectId: "project-mock-1",
      title: "Contract Session",
      titleSource: null,
      status: "active",
      planArtifactId: null,
      seedTaskId: null,
      parentSessionId: null,
      teamMode: null,
      teamConfig: null,
      createdAt: "2026-04-10T10:00:00.000Z",
      updatedAt: "2026-04-10T10:00:00.000Z",
      archivedAt: null,
      convertedAt: null,
      verificationStatus: "unverified",
      verificationInProgress: false,
      gapScore: null,
      sessionPurpose: "general",
      acceptanceStatus: null,
    });

    const conversations = await mockChatApi.listConversations("ideation", sessionId);
    const conversationPayload = await mockChatApi.getConversation(conversation.id);

    queryClient.setQueryData(["chat", "conversations", "ideation", sessionId], conversations);
    queryClient.setQueryData(["chat", "conversations", conversation.id], conversationPayload);
    queryClient.setQueryData(["chat", "conversations", conversation.id, "history"], {
      pages: [
        {
          conversation: conversationPayload.conversation,
          messages: conversationPayload.messages,
          limit: 40,
          offset: 0,
          totalMessageCount: conversationPayload.messages.length,
          hasOlder: false,
        },
      ],
      pageParams: [0],
    });
    chatStore.getState().setActiveConversation(`session:${sessionId}`, conversation.id);
  }, {
    conversation: baseConversation,
    seededMessages: messages,
    sessionId: contextId,
  });

  await page.waitForFunction(({ expectedSessionId, expectedConversationId }) => {
    return window.__chatStore?.getState().activeConversationIds?.[`session:${expectedSessionId}`] === expectedConversationId;
  }, {
    expectedSessionId: contextId,
    expectedConversationId: conversationId,
  });

  await expect(page.locator('[data-testid="conversation-panel"]')).toBeVisible();
  await expect(page.locator('[data-testid="integrated-chat-messages"]')).toBeVisible();
}

async function replaceConversationMessages(
  page: Page,
  messages: Array<typeof userMessage | typeof liveProviderMessage>
) {
  await page.evaluate(async ({ currentConversationId, seededMessages }) => {
    const mockChatApi = window.__mockChatApi;
    const queryClient = window.__queryClient;

    if (!mockChatApi || !queryClient) {
      throw new Error("Expected mock chat app globals to be available");
    }

    mockChatApi.replaceMessages(currentConversationId, seededMessages);
    const conversationPayload = await mockChatApi.getConversation(currentConversationId);
    queryClient.setQueryData(["chat", "conversations", currentConversationId], conversationPayload);
    queryClient.setQueryData(["chat", "conversations", currentConversationId, "history"], {
      pages: [
        {
          conversation: conversationPayload.conversation,
          messages: conversationPayload.messages,
          limit: 40,
          offset: 0,
          totalMessageCount: conversationPayload.messages.length,
          hasOlder: false,
        },
      ],
      pageParams: [0],
    });
  }, {
    currentConversationId: conversationId,
    seededMessages: messages,
  });
}

async function emitChatEvent(page: Page, event: string, payload: unknown) {
  await page.evaluate(async ({ eventName, eventPayload }) => {
    const eventBus = window.__eventBus;
    if (!eventBus) {
      throw new Error("Expected event bus to be available");
    }

    await eventBus.emit(eventName, eventPayload);
  }, { eventName: event, eventPayload: payload });
}

test.describe("Chat Contract", () => {
  test("keeps the live ideation turn deduplicated while streaming over a persisted orchestrator row", async ({ page }) => {
    await seedIdeationConversation(page, [userMessage, liveProviderMessage]);

    await emitChatEvent(page, "agent:tool_call", {
      tool_name: "ralphx::get_session_plan",
      tool_id: "tool-session-plan-1",
      arguments: { session_id: contextId },
      result: { status: "ok" },
      conversation_id: conversationId,
      context_id: contextId,
      context_type: "ideation",
    });
    await emitChatEvent(page, "agent:chunk", {
      text: "I am preparing the plan now.",
      conversation_id: conversationId,
      context_id: contextId,
    });

    await expect(page.getByText("I am preparing the plan now.")).toHaveCount(1);
    await expect(page.locator('[data-testid="ideation-widget-get-session-plan"]')).toHaveCount(1);

    await page.getByTestId("chat-session-stats-button").click();
    await expect(page.getByText("Conversation stats")).toBeVisible();
    await expect(page.getByText("1,250")).toBeVisible();
    await expect(page.getByText("140")).toBeVisible();
    await expect(page.getByText("980")).toBeVisible();
  });

  test("swaps the live footer for the finalized orchestrator row without duplicates and preserves block order", async ({ page }) => {
    await seedIdeationConversation(page, [userMessage, liveProviderMessage]);

    await emitChatEvent(page, "agent:tool_call", {
      tool_name: "ralphx::get_session_plan",
      tool_id: "tool-session-plan-1",
      arguments: { session_id: contextId },
      result: { status: "ok" },
      conversation_id: conversationId,
      context_id: contextId,
      context_type: "ideation",
    });
    await emitChatEvent(page, "agent:chunk", {
      text: "I am preparing the plan now.",
      conversation_id: conversationId,
      context_id: contextId,
    });

    await emitChatEvent(page, "agent:message_created", {
      conversation_id: conversationId,
      context_id: contextId,
      context_type: "ideation",
      role: "orchestrator",
      message_id: finalizedProviderMessage.id,
    });
    await replaceConversationMessages(page, [userMessage, finalizedProviderMessage]);

    await expect(page.getByText("I am preparing the plan now.")).toHaveCount(1);
    await expect(page.getByText("Here is the final plan summary.")).toHaveCount(1);
    await expect(page.locator('[data-testid="ideation-widget-get-session-plan"]')).toHaveCount(1);

    const firstText = page.getByText("I am preparing the plan now.");
    const widget = page.locator('[data-testid="ideation-widget-get-session-plan"]');
    const secondText = page.getByText("Here is the final plan summary.");
    const firstBox = await firstText.boundingBox();
    const widgetBox = await widget.boundingBox();
    const secondBox = await secondText.boundingBox();

    expect(firstBox?.y).toBeLessThan(widgetBox?.y ?? Number.POSITIVE_INFINITY);
    expect(widgetBox?.y).toBeLessThan(secondBox?.y ?? Number.POSITIVE_INFINITY);
  });

  test("bridges the message_created-to-refetch gap without leaking the stale provider row", async ({ page }) => {
    await seedIdeationConversation(page, [userMessage, liveProviderMessage]);

    await emitChatEvent(page, "agent:tool_call", {
      tool_name: "ralphx::get_session_plan",
      tool_id: "tool-session-plan-1",
      arguments: { session_id: contextId },
      result: { status: "ok" },
      conversation_id: conversationId,
      context_id: contextId,
      context_type: "ideation",
    });
    await emitChatEvent(page, "agent:chunk", {
      text: "I am preparing the plan now.",
      conversation_id: conversationId,
      context_id: contextId,
    });

    await expect(page.getByText("I am preparing the plan now.")).toHaveCount(1);
    await expect(page.locator('[data-testid="ideation-widget-get-session-plan"]')).toHaveCount(1);

    await emitChatEvent(page, "agent:message_created", {
      conversation_id: conversationId,
      context_id: contextId,
      context_type: "ideation",
      role: "orchestrator",
      message_id: finalizedProviderMessage.id,
    });

    await expect(page.getByText("Hello there, this is a test message")).toBeVisible();
    await expect(page.getByText("I am preparing the plan now.")).toHaveCount(0);
    await expect(page.locator('[data-testid="ideation-widget-get-session-plan"]')).toHaveCount(0);

    await replaceConversationMessages(page, [userMessage, finalizedProviderMessage]);

    await expect(page.getByText("I am preparing the plan now.")).toHaveCount(1);
    await expect(page.getByText("Here is the final plan summary.")).toHaveCount(1);
    await expect(page.locator('[data-testid="ideation-widget-get-session-plan"]')).toHaveCount(1);
  });

  test("keeps the persisted live snapshot visible when a turn completes without a finalized replacement row", async ({ page }) => {
    await seedIdeationConversation(page, [userMessage, liveProviderMessage]);

    await emitChatEvent(page, "agent:tool_call", {
      tool_name: "ralphx::get_session_plan",
      tool_id: "tool-session-plan-1",
      arguments: { session_id: contextId },
      result: { status: "ok" },
      conversation_id: conversationId,
      context_id: contextId,
      context_type: "ideation",
    });
    await emitChatEvent(page, "agent:chunk", {
      text: "I am preparing the plan now.",
      conversation_id: conversationId,
      context_id: contextId,
      context_type: "ideation",
    });

    await expect(page.getByText("I am preparing the plan now.")).toHaveCount(1);
    await expect(page.locator('[data-testid="ideation-widget-get-session-plan"]')).toHaveCount(1);

    await emitChatEvent(page, "agent:turn_completed", {
      conversation_id: conversationId,
      context_id: contextId,
      context_type: "ideation",
    });

    await expect(page.getByText("I am preparing the plan now.")).toHaveCount(1);
    await expect(page.locator('[data-testid="ideation-widget-get-session-plan"]')).toHaveCount(1);
  });

  test("keeps finalized widgets visible when appended raw content diverges from persisted content blocks", async ({ page }) => {
    await seedIdeationConversation(page, [userMessage, erroredProviderMessage]);

    await expect(page.locator('[data-testid="ideation-widget-get-session-plan"]')).toHaveCount(1);
    await expect(page.getByText("Here is the final plan summary.")).toHaveCount(1);
    await expect(page.getByText("I am preparing the plan now.")).toHaveCount(1);
  });

  test("hydrates conversation stats during a live turn before finalization", async ({ page }) => {
    await seedIdeationConversation(page, [
      userMessage,
      {
        ...liveProviderMessage,
        inputTokens: null,
        outputTokens: null,
        cacheCreationTokens: null,
        cacheReadTokens: null,
      },
    ]);

    const statsButton = page.getByTestId("chat-session-stats-button");
    await expect(statsButton).toBeVisible();
    await statsButton.click();
    await expect(page.getByText("Aggregated from none.")).toBeVisible();
    await expect(page.getByText("Unavailable", { exact: true })).toBeVisible();

    await replaceConversationMessages(page, [userMessage, liveProviderMessageWithUsage]);
    await emitChatEvent(page, "agent:usage_updated", {
      conversation_id: conversationId,
      context_id: contextId,
      context_type: "ideation",
    });

    await expect(page.getByText("Aggregated from messages.")).toBeVisible();
    await expect(page.getByText("76.3k")).toBeVisible();
    await expect(page.getByText("12.1k")).toBeVisible();
    await expect(page.getByText("49.9k")).toBeVisible();
  });
});
