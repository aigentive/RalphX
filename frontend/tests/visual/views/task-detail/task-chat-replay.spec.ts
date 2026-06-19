import { expect, test, type Locator } from "@playwright/test";
import { setupTaskChatScenario } from "../../../fixtures/chat.fixtures";

async function visibleY(locator: Locator, label: string) {
  await expect(locator, `${label} should be visible`).toBeVisible();
  const box = await locator.boundingBox();
  expect(box, `${label} should have a layout box`).not.toBeNull();
  return box!.y;
}

function collapsedToolCallGroupToggles(root: Locator) {
  return root.getByRole("button", { name: /^Agent called \d+ tools$/ });
}

async function expandToolCallGroups(root: Locator) {
  await expect(collapsedToolCallGroupToggles(root).first()).toBeVisible({ timeout: 10000 });

  for (let index = 0; index < 20; index += 1) {
    const toggles = collapsedToolCallGroupToggles(root);
    if ((await toggles.count()) === 0) {
      return;
    }
    await toggles.first().click();
  }
}

const executionContractContextId = "task-mock-4";
const executionContractConversationId = "conv-execution-contract";
const reviewContractContextId = "task-mock-5";
const reviewContractConversationId = "conv-review-contract";
const mergeContractContextId = "task-mock-merge-incomplete";
const mergeContractConversationId = "conv-merge-contract";

const executionContractConversation = {
  id: executionContractConversationId,
  contextType: "task_execution",
  contextId: executionContractContextId,
  claudeSessionId: "claude-execution-contract",
  providerSessionId: "claude-execution-contract",
  providerHarness: "claude",
  upstreamProvider: "anthropic",
  providerProfile: null,
  title: "Execution Contract Session",
  messageCount: 0,
  lastMessageAt: null,
  createdAt: "2026-04-10T10:00:00.000Z",
  updatedAt: "2026-04-10T10:00:00.000Z",
} as const;

const executionUserMessage = {
  id: "msg-execution-user-1",
  sessionId: null,
  projectId: "project-mock-1",
  taskId: executionContractContextId,
  role: "user",
  content: "Execute the task and validate the result.",
  metadata: null,
  parentMessageId: null,
  conversationId: executionContractConversationId,
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
} as const;

const executionLiveProviderMessage = {
  id: "msg-execution-assistant-1",
  sessionId: null,
  projectId: "project-mock-1",
  taskId: executionContractContextId,
  role: "assistant",
  content: "I am reading the message renderer now.",
  metadata: null,
  parentMessageId: "msg-execution-user-1",
  conversationId: executionContractConversationId,
  toolCalls: [
    {
      id: "tool-execution-read-1",
      name: "Read",
      arguments: { file_path: "frontend/src/components/Chat/MessageItem.tsx" },
      result: "provider metadata rendering",
    },
  ],
  contentBlocks: [
    { type: "text", text: "I am reading the message renderer now." },
    {
      type: "tool_use",
      id: "tool-execution-read-1",
      name: "Read",
      arguments: { file_path: "frontend/src/components/Chat/MessageItem.tsx" },
      result: "provider metadata rendering",
    },
  ],
  sender: "worker",
  attributionSource: "native",
  providerHarness: "claude",
  providerSessionId: "claude-execution-contract",
  upstreamProvider: "anthropic",
  providerProfile: null,
  logicalModel: "claude-sonnet-4-6",
  effectiveModelId: "claude-sonnet-4-6",
  logicalEffort: "medium",
  effectiveEffort: "medium",
  inputTokens: 980,
  outputTokens: 164,
  cacheCreationTokens: 0,
  cacheReadTokens: 512,
  estimatedUsd: 0.02,
  createdAt: "2026-04-10T10:01:00.000Z",
} as const;

const executionFinalProviderMessage = {
  ...executionLiveProviderMessage,
  id: "msg-execution-assistant-final-1",
  content: "I am reading the message renderer now.\n\nThe provider metadata row is isolated to MessageItem and remains safe to adjust.",
  contentBlocks: [
    { type: "text", text: "I am reading the message renderer now." },
    {
      type: "tool_use",
      id: "tool-execution-read-1",
      name: "Read",
      arguments: { file_path: "frontend/src/components/Chat/MessageItem.tsx" },
      result: "provider metadata rendering",
    },
    {
      type: "text",
      text: "The provider metadata row is isolated to MessageItem and remains safe to adjust.",
    },
  ],
  outputTokens: 220,
  createdAt: "2026-04-10T10:02:00.000Z",
} as const;

const reviewContractConversation = {
  id: reviewContractConversationId,
  contextType: "review",
  contextId: reviewContractContextId,
  claudeSessionId: "claude-review-contract",
  providerSessionId: "claude-review-contract",
  providerHarness: "claude",
  upstreamProvider: "anthropic",
  providerProfile: null,
  title: "Review Contract Session",
  messageCount: 0,
  lastMessageAt: null,
  createdAt: "2026-04-10T10:00:00.000Z",
  updatedAt: "2026-04-10T10:00:00.000Z",
} as const;

const reviewUserMessage = {
  id: "msg-review-user-contract-1",
  sessionId: null,
  projectId: "project-mock-1",
  taskId: reviewContractContextId,
  role: "user",
  content: "Review the task and decide whether to approve it.",
  metadata: null,
  parentMessageId: null,
  conversationId: reviewContractConversationId,
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
} as const;

const reviewLiveProviderMessage = {
  id: "msg-review-assistant-contract-1",
  sessionId: null,
  projectId: "project-mock-1",
  taskId: reviewContractContextId,
  role: "assistant",
  content: "I am completing the review now.",
  metadata: null,
  parentMessageId: "msg-review-user-contract-1",
  conversationId: reviewContractConversationId,
  toolCalls: [
    {
      id: "tool-review-complete-1",
      name: "complete_review",
      arguments: {
        decision: "changes_requested",
        feedback: "Need broader contract coverage.",
        issues: [{ severity: "major", description: "Execution/review live-final parity missing." }],
      },
      result: { success: true, new_status: "reviewing", followup_session_id: "followup-review-1" },
    },
  ],
  contentBlocks: [
    { type: "text", text: "I am completing the review now." },
    {
      type: "tool_use",
      id: "tool-review-complete-1",
      name: "complete_review",
      arguments: {
        decision: "changes_requested",
        feedback: "Need broader contract coverage.",
        issues: [{ severity: "major", description: "Execution/review live-final parity missing." }],
      },
      result: { success: true, new_status: "reviewing", followup_session_id: "followup-review-1" },
    },
  ],
  sender: "reviewer",
  attributionSource: "native",
  providerHarness: "claude",
  providerSessionId: "claude-review-contract",
  upstreamProvider: "anthropic",
  providerProfile: null,
  logicalModel: "claude-sonnet-4-6",
  effectiveModelId: "claude-sonnet-4-6",
  logicalEffort: "medium",
  effectiveEffort: "medium",
  inputTokens: 1506,
  outputTokens: 203,
  cacheCreationTokens: 0,
  cacheReadTokens: 644,
  estimatedUsd: 0.03,
  createdAt: "2026-04-10T10:01:00.000Z",
} as const;

const reviewFinalProviderMessage = {
  ...reviewLiveProviderMessage,
  id: "msg-review-assistant-contract-final-1",
  content: "I am completing the review now.\n\nChanges are still required before approval.",
  contentBlocks: [
    { type: "text", text: "I am completing the review now." },
    {
      type: "tool_use",
      id: "tool-review-complete-1",
      name: "complete_review",
      arguments: {
        decision: "changes_requested",
        feedback: "Need broader contract coverage.",
        issues: [{ severity: "major", description: "Execution/review live-final parity missing." }],
      },
      result: { success: true, new_status: "reviewing", followup_session_id: "followup-review-1" },
    },
    { type: "text", text: "Changes are still required before approval." },
  ],
  outputTokens: 240,
  createdAt: "2026-04-10T10:02:00.000Z",
} as const;

const reviewCancelledProviderMessage = {
  ...reviewFinalProviderMessage,
  content:
    "I am completing the review now.\n\nChanges are still required before approval.\n\n[Agent error: user cancelled MCP tool call]",
} as const;

const mergeContractConversation = {
  id: mergeContractConversationId,
  contextType: "merge",
  contextId: mergeContractContextId,
  claudeSessionId: null,
  providerSessionId: "thread-merge-contract",
  providerHarness: "codex",
  upstreamProvider: "openai",
  providerProfile: null,
  title: "Merge Contract Session",
  messageCount: 0,
  lastMessageAt: null,
  createdAt: "2026-04-10T10:00:00.000Z",
  updatedAt: "2026-04-10T10:00:00.000Z",
} as const;

const mergeUserMessage = {
  id: "msg-merge-user-1",
  sessionId: null,
  projectId: "project-mock-1",
  taskId: mergeContractContextId,
  role: "user",
  content: "Please resolve the merge path.",
  metadata: null,
  parentMessageId: null,
  conversationId: mergeContractConversationId,
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
} as const;

const mergeLiveProviderMessage = {
  id: "msg-merge-assistant-1",
  sessionId: null,
  projectId: "project-mock-1",
  taskId: mergeContractContextId,
  role: "assistant",
  content: "I am checking the merge target now.",
  metadata: null,
  parentMessageId: "msg-merge-user-1",
  conversationId: mergeContractConversationId,
  toolCalls: [
    {
      id: "tool-merge-target-1",
      name: "ralphx::get_merge_target",
      arguments: { task_id: mergeContractContextId },
      result: { branch: "main" },
    },
  ],
  contentBlocks: [
    { type: "text", text: "I am checking the merge target now." },
    {
      type: "tool_use",
      id: "tool-merge-target-1",
      name: "ralphx::get_merge_target",
      arguments: { task_id: mergeContractContextId },
      result: { branch: "main" },
    },
  ],
  sender: null,
  attributionSource: "native",
  providerHarness: "codex",
  providerSessionId: "thread-merge-contract",
  upstreamProvider: "openai",
  providerProfile: null,
  logicalModel: "gpt-5.4",
  effectiveModelId: "gpt-5.4",
  logicalEffort: "high",
  effectiveEffort: "xhigh",
  inputTokens: 1440,
  outputTokens: 180,
  cacheCreationTokens: 20,
  cacheReadTokens: 900,
  estimatedUsd: null,
  createdAt: "2026-04-10T10:01:00.000Z",
} as const;

const mergeFinalProviderMessage = {
  ...mergeLiveProviderMessage,
  id: "msg-merge-assistant-final-1",
  content: "I am checking the merge target now.\n\nThe target is main, and the merge conflict is isolated to src/commands/gateway.ts.",
  contentBlocks: [
    { type: "text", text: "I am checking the merge target now." },
    {
      type: "tool_use",
      id: "tool-merge-target-1",
      name: "ralphx::get_merge_target",
      arguments: { task_id: mergeContractContextId },
      result: { branch: "main" },
    },
    {
      type: "text",
      text: "The target is main, and the merge conflict is isolated to src/commands/gateway.ts.",
    },
  ],
  outputTokens: 240,
  createdAt: "2026-04-10T10:02:00.000Z",
} as const;

async function seedTaskContractConversation(
  page: Page,
  scenario: "execution_db_compact" | "review_db_compact" | "merge_db_compact",
  conversation: {
    id: string;
    contextType: string;
    contextId: string;
  },
  messages: Array<Record<string, unknown>>,
) {
  await setupTaskChatScenario(page, scenario);

  await page.evaluate(async ({ conversation, seededMessages }) => {
    const mockChatApi = window.__mockChatApi;
    const queryClient = window.__queryClient;
    const chatStore = window.__chatStore;

    if (!mockChatApi || !queryClient || !chatStore) {
      throw new Error("Expected mock chat app globals to be available");
    }

    const timelineSeededMessages = seededMessages.map((message) => {
      const id = typeof message.id === "string" ? message.id : "";
      return message.role !== "user" && !id.includes("final")
        ? { ...message, timelineStatus: "streaming" }
        : message;
    });

    mockChatApi.seedConversation(conversation, timelineSeededMessages);

    queryClient.setQueryData(["chat", "conversations", conversation.contextType, conversation.contextId], [conversation]);
    queryClient.setQueryData(["chat", "conversations", conversation.id], {
      conversation,
      messages: timelineSeededMessages,
    });
    queryClient.setQueryData(["chat", "conversations", conversation.id, "timeline"], {
      pages: [
        await mockChatApi.getConversationTimelinePage(conversation.id, 40, null),
      ],
      pageParams: [null],
    });

    chatStore.getState().setActiveConversation(`${conversation.contextType}:${conversation.contextId}`, conversation.id);
  }, {
    conversation,
    seededMessages: messages,
  });

  await expect(page.locator('[data-testid="integrated-chat-panel"]')).toBeVisible();
}

async function replaceTaskContractMessages(
  page: Page,
  conversationId: string,
  messages: Array<Record<string, unknown>>,
) {
  await page.evaluate(async ({ conversationId, seededMessages }) => {
    const mockChatApi = window.__mockChatApi;
    const queryClient = window.__queryClient;

    if (!mockChatApi || !queryClient) {
      throw new Error("Expected mock chat app globals to be available");
    }

    mockChatApi.replaceMessages(conversationId, seededMessages);
    const timelinePage = await mockChatApi.getConversationTimelinePage(conversationId, 40, null);
    queryClient.setQueryData(["chat", "conversations", conversationId], {
      conversation: await mockChatApi.getConversation(conversationId).then((payload) => payload.conversation),
      messages: seededMessages,
    });
    queryClient.setQueryData(["chat", "conversations", conversationId, "timeline"], {
      pages: [timelinePage],
      pageParams: [null],
    });
  }, {
    conversationId,
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

test.describe("Task Chat Replay", () => {
  test("renders DB-derived execution replay in the task chat panel", async ({ page }) => {
    await setupTaskChatScenario(page, "execution_db_compact");

    const panel = page.locator('[data-testid="integrated-chat-panel"]');

    await expect(panel).toBeVisible();
    await expect(page.getByTestId("chat-session-provider-badge")).toHaveText(/Claude/i);
    await expect(
      panel.getByText("Execution replay sampled from a compact two-message worker conversation."),
    ).toBeVisible();
    await expandToolCallGroups(panel);
    await expect(
      panel.getByText("frontend/src/components/Chat/MessageItem.tsx"),
    ).toBeVisible();

    await page.getByTestId("chat-session-stats-button").click();
    await expect(page.getByText("980")).toBeVisible();
    await expect(page.getByText("164")).toBeVisible();
  });

  test("renders DB-derived review replay in the task chat panel", async ({ page }) => {
    await setupTaskChatScenario(page, "review_db_compact");

    const panel = page.locator('[data-testid="integrated-chat-panel"]');

    await expect(panel).toBeVisible();
    await expect(page.getByTestId("chat-session-provider-badge")).toHaveText(/Claude/i);
    await expect(
      panel.getByText("Reviewer replay sampled from a compact two-message real conversation."),
    ).toBeVisible();
    await expect(panel.getByText(/Changes Requested/i)).toBeVisible();

    await page.getByTestId("chat-session-stats-button").click();
    await expect(page.getByText("1,506")).toBeVisible();
    await expect(page.getByText("203")).toBeVisible();
  });

  test("renders DB-derived merge replay in the task chat panel", async ({ page }) => {
    await setupTaskChatScenario(page, "merge_db_compact");

    const panel = page.locator('[data-testid="integrated-chat-panel"]');

    await expect(panel).toBeVisible();
    await expect(page.getByTestId("chat-session-provider-badge")).toHaveText(/Codex/i);
    await expect(
      panel.getByText("Merge replay sampled from a compact two-message merger conversation."),
    ).toBeVisible();

    await page.getByTestId("chat-session-stats-button").click();
    await expect(page.getByText("1,244")).toBeVisible();
    await expect(page.getByText("188")).toBeVisible();
    await expect(page.getByText("gpt-5.4", { exact: true })).toBeVisible();
  });

  test("keeps live and finalized merge turns deduplicated with stable widget order", async ({ page }) => {
    await seedTaskContractConversation(
      page,
      "merge_db_compact",
      mergeContractConversation,
      [mergeUserMessage, mergeLiveProviderMessage],
    );

    await emitChatEvent(page, "agent:tool_call", {
      tool_name: "ralphx::get_merge_target",
      tool_id: "tool-merge-target-1",
      arguments: { task_id: mergeContractContextId },
      result: { branch: "main" },
      conversation_id: mergeContractConversationId,
      context_id: mergeContractContextId,
      context_type: "merge",
    });
    await emitChatEvent(page, "agent:chunk", {
      text: "I am checking the merge target now.",
      conversation_id: mergeContractConversationId,
      context_id: mergeContractContextId,
    });

    await expect(page.getByText("I am checking the merge target now.")).toHaveCount(1);
    await expect(page.locator('[data-testid="merge-widget-target"]')).toHaveCount(1);

    await emitChatEvent(page, "agent:message_created", {
      conversation_id: mergeContractConversationId,
      context_id: mergeContractContextId,
      context_type: "merge",
      role: "assistant",
      message_id: mergeFinalProviderMessage.id,
    });
    await replaceTaskContractMessages(page, mergeContractConversationId, [mergeUserMessage, mergeFinalProviderMessage]);

    await expect(page.getByText("I am checking the merge target now.")).toHaveCount(1);
    await expect(
      page.getByText("The target is main, and the merge conflict is isolated to src/commands/gateway.ts."),
    ).toHaveCount(1);
    await expect(page.locator('[data-testid="merge-widget-target"]')).toHaveCount(1);

    const firstText = page.getByText("I am checking the merge target now.");
    const widget = page.locator('[data-testid="merge-widget-target"]');
    const secondText = page.getByText(
      "The target is main, and the merge conflict is isolated to src/commands/gateway.ts.",
    );
    const firstY = await visibleY(firstText, "first merge text");
    const widgetY = await visibleY(widget, "merge widget");
    const secondY = await visibleY(secondText, "second merge text");

    expect(firstY).toBeLessThan(widgetY);
    expect(widgetY).toBeLessThan(secondY);
  });

  test("keeps live and finalized execution turns deduplicated with stable task-tool ordering", async ({ page }) => {
    await seedTaskContractConversation(
      page,
      "execution_db_compact",
      executionContractConversation,
      [executionUserMessage, executionLiveProviderMessage],
    );

    await emitChatEvent(page, "agent:tool_call", {
      tool_name: "Read",
      tool_id: "tool-execution-read-1",
      arguments: { file_path: "frontend/src/components/Chat/MessageItem.tsx" },
      result: "provider metadata rendering",
      conversation_id: executionContractConversationId,
      context_id: executionContractContextId,
      context_type: "task_execution",
    });
    await emitChatEvent(page, "agent:chunk", {
      text: "I am reading the message renderer now.",
      conversation_id: executionContractConversationId,
      context_id: executionContractContextId,
    });

    await expect(page.getByText("I am reading the message renderer now.")).toHaveCount(1);
    await expect(page.getByText("frontend/src/components/Chat/MessageItem.tsx")).toHaveCount(1);

    await emitChatEvent(page, "agent:message_created", {
      conversation_id: executionContractConversationId,
      context_id: executionContractContextId,
      context_type: "task_execution",
      role: "assistant",
      message_id: executionFinalProviderMessage.id,
    });
    await replaceTaskContractMessages(page, executionContractConversationId, [
      executionUserMessage,
      executionFinalProviderMessage,
    ]);

    await expect(page.getByText("I am reading the message renderer now.")).toHaveCount(1);
    await expect(
      page.getByText("The provider metadata row is isolated to MessageItem and remains safe to adjust."),
    ).toHaveCount(1);
    await expect(page.getByText("frontend/src/components/Chat/MessageItem.tsx")).toHaveCount(1);

    const firstText = page.getByText("I am reading the message renderer now.");
    const widget = page.getByText("frontend/src/components/Chat/MessageItem.tsx");
    const secondText = page.getByText(
      "The provider metadata row is isolated to MessageItem and remains safe to adjust.",
    );
    const firstY = await visibleY(firstText, "first execution text");
    const widgetY = await visibleY(widget, "execution widget");
    const secondY = await visibleY(secondText, "second execution text");

    expect(firstY).toBeLessThan(widgetY);
    expect(widgetY).toBeLessThan(secondY);
  });

  test("keeps live and finalized review turns deduplicated with stable widget order", async ({ page }) => {
    await seedTaskContractConversation(
      page,
      "review_db_compact",
      reviewContractConversation,
      [reviewUserMessage, reviewLiveProviderMessage],
    );

    await emitChatEvent(page, "agent:tool_call", {
      tool_name: "complete_review",
      tool_id: "tool-review-complete-1",
      arguments: {
        decision: "changes_requested",
        feedback: "Need broader contract coverage.",
        issues: [{ severity: "major", description: "Execution/review live-final parity missing." }],
      },
      result: { success: true, new_status: "reviewing", followup_session_id: "followup-review-1" },
      conversation_id: reviewContractConversationId,
      context_id: reviewContractContextId,
      context_type: "review",
    });
    await emitChatEvent(page, "agent:chunk", {
      text: "I am completing the review now.",
      conversation_id: reviewContractConversationId,
      context_id: reviewContractContextId,
    });

    await expect(page.getByText("I am completing the review now.")).toHaveCount(1);
    await expect(page.locator('[data-testid="review-widget-complete"]')).toHaveCount(1);

    await emitChatEvent(page, "agent:message_created", {
      conversation_id: reviewContractConversationId,
      context_id: reviewContractContextId,
      context_type: "review",
      role: "assistant",
      message_id: reviewFinalProviderMessage.id,
    });
    await replaceTaskContractMessages(page, reviewContractConversationId, [
      reviewUserMessage,
      reviewFinalProviderMessage,
    ]);

    await expect(page.getByText("I am completing the review now.")).toHaveCount(1);
    await expect(page.getByText("Changes are still required before approval.")).toHaveCount(1);
    await expect(page.locator('[data-testid="review-widget-complete"]')).toHaveCount(1);

    const firstText = page.getByText("I am completing the review now.");
    const widget = page.locator('[data-testid="review-widget-complete"]');
    const secondText = page.getByText("Changes are still required before approval.");
    const firstY = await visibleY(firstText, "first review text");
    const widgetY = await visibleY(widget, "review widget");
    const secondY = await visibleY(secondText, "second review text");

    expect(firstY).toBeLessThan(widgetY);
    expect(widgetY).toBeLessThan(secondY);
  });

  test("keeps persisted review widgets visible when raw content includes cancelled-tool noise", async ({ page }) => {
    await seedTaskContractConversation(
      page,
      "review_db_compact",
      reviewContractConversation,
      [reviewUserMessage, reviewCancelledProviderMessage],
    );

    await expect(page.locator('[data-testid="review-widget-complete"]')).toHaveCount(1);
    await expect(page.getByText("I am completing the review now.")).toHaveCount(1);
    await expect(page.getByText("Changes are still required before approval.")).toHaveCount(1);
    await expect(page.getByText(/\[Agent error: user cancelled MCP tool call\]/)).toHaveCount(0);
  });
});
