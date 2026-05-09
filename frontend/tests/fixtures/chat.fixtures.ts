import { expect, Page } from "@playwright/test";
import { setupApp } from "./setup.fixtures";
import {
  IDEATION_REPLAY_CONTEXTS,
  TASK_REPLAY_CONTEXTS,
  type MockChatScenarioName,
} from "@/api-mock/chat-scenarios";
import type { ChildSessionStatusResponse } from "@/api/chat";

export type ChatScenarioName = MockChatScenarioName;

type ChildSessionStatusOverride = {
  response?: ChildSessionStatusResponse;
  error?: string;
  delayMs?: number;
};

type IdeationChatScenarioName = Extract<
  ChatScenarioName,
  "ideation_db_widget_mix" | "ideation_widget_matrix"
>;
type TaskChatScenarioName = Exclude<ChatScenarioName, IdeationChatScenarioName>;

export async function seedChatScenario(page: Page, scenario: ChatScenarioName) {
  await page.evaluate(({ scenarioName }) => {
    const mockChatApi = (window as Window).__mockChatApi;
    const queryClient = (window as Window).__queryClient;

    if (!mockChatApi) {
      throw new Error("Mock chat API not available");
    }

    mockChatApi.reset();
    mockChatApi.seedScenario(scenarioName);
    queryClient?.invalidateQueries();
  }, { scenarioName: scenario });
}

export async function setChildSessionStatusOverride(
  page: Page,
  sessionId: string,
  override: ChildSessionStatusOverride
) {
  await page.evaluate(({ childSessionId, childOverride }) => {
    const mockChatApi = (window as Window).__mockChatApi;

    if (!mockChatApi) {
      throw new Error("Mock chat API not available");
    }

    mockChatApi.setChildSessionStatusOverride(childSessionId, childOverride);
  }, { childSessionId: sessionId, childOverride: override });
}

async function seedChildSessionOverrides(
  page: Page,
  overrides: Record<string, ChildSessionStatusOverride> | undefined
) {
  if (!overrides) {
    return;
  }

  for (const [sessionId, override] of Object.entries(overrides)) {
    await setChildSessionStatusOverride(page, sessionId, override);
  }
}

export async function setupIdeationChatScenario(
  page: Page,
  scenario: IdeationChatScenarioName,
  options?: {
    childSessionOverrides?: Record<string, ChildSessionStatusOverride>;
  }
) {
  const replayContext = IDEATION_REPLAY_CONTEXTS[scenario];
  await setupApp(page);
  await page.click('[data-testid="nav-ideation"]');
  await page.waitForSelector('[data-testid="ideation-view"]', { timeout: 10000 });
  await page.waitForTimeout(250);
  await seedChatScenario(page, scenario);
  await seedChildSessionOverrides(page, options?.childSessionOverrides);
  await page.evaluate(async (replayContext) => {
    const chatStore = (window as Window).__chatStore;
    const ideationStore = (window as Window).__ideationStore;
    const mockChatApi = (window as Window).__mockChatApi;
    const queryClient = (window as Window).__queryClient;
    const { conversationId, contextType, contextId } = replayContext;

    if (!ideationStore) {
      throw new Error("Ideation store not available");
    }
    if (!mockChatApi) {
      throw new Error("Mock chat API not available");
    }
    if (!queryClient) {
      throw new Error("Query client not available");
    }

    ideationStore.getState().selectSession({
      id: contextId,
      projectId: "project-mock-1",
      title: contextId === "session-widget-matrix" ? "Widget Matrix Session" : "Demo Ideation Session",
      titleSource: null,
      status: "active",
      planArtifactId: null,
      seedTaskId: null,
      parentSessionId: null,
      teamMode: null,
      teamConfig: null,
      createdAt: "2026-03-11T21:51:34.589194Z",
      updatedAt: "2026-04-10T08:15:00.000000Z",
      archivedAt: null,
      convertedAt: null,
      verificationStatus: "unverified",
      verificationInProgress: false,
      gapScore: null,
      sessionPurpose: "general",
      acceptanceStatus: null,
    });

    const conversations = await mockChatApi.listConversations(contextType, contextId);
    const conversation = await mockChatApi.getConversation(conversationId);
    const seedConversationCache = (
      conversationsPayload: unknown,
      conversationPayload: typeof conversation,
    ) => {
      queryClient.setQueryData(
        ["chat", "conversations", contextType, contextId],
        conversationsPayload,
      );
      queryClient.setQueryData(
        ["chat", "conversations", conversationId],
        conversationPayload,
      );
      queryClient.setQueryData(
        ["chat", "conversations", conversationId, "history"],
        {
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
        },
      );
    };

    seedConversationCache(conversations, conversation);

    chatStore?.getState().setActiveConversation(`session:${contextId}`, conversationId);
  }, replayContext);
  await page.waitForFunction((replayContext) => {
    const chatStore = (window as Window).__chatStore;
    return (
      chatStore?.getState().activeConversationIds?.[`session:${replayContext.contextId}`] ===
      replayContext.conversationId
    );
  }, replayContext);
  await expect(page.locator('[data-testid="conversation-panel"]')).toBeVisible();
  await expect(page.locator('[data-testid="integrated-chat-messages"]')).toBeVisible();
  await expect(page.locator('[data-testid="chat-session-chips"]')).toBeVisible();
}

export async function setupTaskChatScenario(page: Page, scenario: TaskChatScenarioName) {
  const { contextType, contextId, conversationId } = TASK_REPLAY_CONTEXTS[scenario];

  await setupApp(page);
  await seedChatScenario(page, scenario);
  await page.evaluate(({ taskId }) => {
    const uiStore = (window as Window).__uiStore as {
      getState(): {
        setCurrentView(view: string): void;
        setSelectedTaskId(taskId: string | null): void;
      };
    } | undefined;

    if (!uiStore) {
      throw new Error("UI store not available");
    }

    uiStore.getState().setCurrentView("kanban");
    uiStore.getState().setSelectedTaskId(taskId);
  }, { taskId: contextId });
  await page.waitForSelector('[data-testid="task-detail-overlay"]', { timeout: 10000 });
  await page.waitForSelector('[data-testid="integrated-chat-panel"]', { timeout: 10000 });

  await page.evaluate(async ({ currentContextType, currentContextId, currentConversationId }) => {
    const chatStore = (window as Window).__chatStore;
    const mockChatApi = (window as Window).__mockChatApi;
    const queryClient = (window as Window).__queryClient;

    if (!mockChatApi) {
      throw new Error("Mock chat API not available");
    }
    if (!queryClient) {
      throw new Error("Query client not available");
    }

    const conversations = await mockChatApi.listConversations(
      currentContextType,
      currentContextId,
    );
    const conversation = await mockChatApi.getConversation(currentConversationId);
    const seedConversationCache = (
      conversationsPayload: unknown,
      conversationPayload: typeof conversation,
    ) => {
      queryClient.setQueryData(
        ["chat", "conversations", currentContextType, currentContextId],
        conversationsPayload,
      );
      queryClient.setQueryData(
        ["chat", "conversations", currentConversationId],
        conversationPayload,
      );
      queryClient.setQueryData(
        ["chat", "conversations", currentConversationId, "history"],
        {
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
        },
      );
    };

    seedConversationCache(conversations, conversation);

    chatStore?.getState().setActiveConversation(
      `${currentContextType}:${currentContextId}`,
      currentConversationId,
    );
  }, {
    currentContextType: contextType,
    currentContextId: contextId,
    currentConversationId: conversationId,
  });

  await page.waitForFunction(({ storeKey, currentConversationId }) => {
    const chatStore = (window as Window).__chatStore;
    return chatStore?.getState().activeConversationIds?.[storeKey] === currentConversationId;
  }, {
    storeKey: `${contextType}:${contextId}`,
    currentConversationId: conversationId,
  });

  await expect(page.locator('[data-testid="integrated-chat-panel"]')).toBeVisible();
  await expect(page.locator('[data-testid="chat-session-chips"]')).toBeVisible();
}
