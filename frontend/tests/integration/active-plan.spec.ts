import { expect, test, type Page } from "@playwright/test";

import type { ChatMessageResponse } from "@/api/chat";
import type {
  AgentConversationMode,
  ChatConversation,
} from "@/types/chat-conversation";
import type { Task } from "@/types/task";
import { setupApp } from "../fixtures/setup.fixtures";

const PROJECT_ID = "project-mock-1";
const CONVERSATION_ID = "active-plan-agents-conversation";
const SESSION_ID = "active-plan-session";
const PLAN_ARTIFACT_ID = "active-plan-artifact";

function makeConversation(): ChatConversation {
  return {
    id: CONVERSATION_ID,
    contextType: "project",
    contextId: PROJECT_ID,
    claudeSessionId: null,
    providerSessionId: `thread-${CONVERSATION_ID}`,
    providerHarness: "codex",
    upstreamProvider: "openai",
    providerProfile: null,
    agentMode: "ideation",
    automationId: null,
    automationRunId: null,
    coordinationMode: "solo",
    title: "Active Plan Agents Tasks coverage",
    messageCount: 1,
    lastMessageAt: "2026-07-20T10:01:00.000Z",
    createdAt: "2026-07-20T10:00:00.000Z",
    updatedAt: "2026-07-20T10:01:00.000Z",
    archivedAt: null,
  };
}

function makeMessage(): ChatMessageResponse {
  return {
    id: `${CONVERSATION_ID}-message`,
    sessionId: null,
    projectId: PROJECT_ID,
    taskId: null,
    role: "assistant",
    content: "The accepted plan is ready for task review.",
    metadata: null,
    parentMessageId: null,
    conversationId: CONVERSATION_ID,
    toolCalls: null,
    contentBlocks: null,
    sender: null,
    attributionSource: "provider",
    providerHarness: "codex",
    providerSessionId: `thread-${CONVERSATION_ID}`,
    upstreamProvider: "openai",
    providerProfile: null,
    logicalModel: "gpt-5.4",
    effectiveModelId: "gpt-5.4",
    logicalEffort: "medium",
    effectiveEffort: "medium",
    inputTokens: 120,
    outputTokens: 40,
    cacheCreationTokens: 0,
    cacheReadTokens: 0,
    estimatedUsd: null,
    createdAt: "2026-07-20T10:01:00.000Z",
  };
}

async function seedAgentsTaskScenario(page: Page) {
  await page.evaluate(
    async ({ conversation, message, projectId, sessionId, planArtifactId }) => {
      const queryClient = window.__queryClient;
      const mockStore = window.__mockStore as
        | { tasks: Map<string, Task> }
        | undefined;
      if (!queryClient || !mockStore) {
        throw new Error("Expected mock store and query client to be available");
      }

      window.__mockChatApi?.reset();
      mockStore.tasks.clear();

      const {
        mockGetAgentConversationWorkspace,
        mockGetConversation,
        mockStartAgentConversation,
        seedMockAgentConversationWorkspace,
        seedMockConversation,
      } = await import("/src/api-mock/chat");
      const { mockIdeationApi } = await import("/src/api-mock/ideation");
      const { createMockTask } = await import("/src/test/mock-data");

      seedMockConversation(conversation, [message]);
      await mockStartAgentConversation({
        projectId,
        content: "Seed the accepted Agents Tasks workspace",
        conversationId: conversation.id,
        providerHarness: "codex",
        modelId: "gpt-5.4",
        mode: "ideation" as AgentConversationMode,
        base: {
          kind: "current_branch",
          ref: "main",
          displayName: "Current branch (main)",
        },
      });

      const now = "2026-07-20T10:00:00.000Z";
      mockIdeationApi.sessions.seedWithData({
        session: {
          id: sessionId,
          projectId,
          title: "Accepted active plan",
          titleSource: null,
          status: "accepted",
          planArtifactId,
          seedTaskId: null,
          parentSessionId: null,
          createdAt: now,
          updatedAt: now,
          archivedAt: null,
          convertedAt: "2026-07-20T10:02:00.000Z",
          verificationStatus: "verified",
          verificationInProgress: false,
          gapScore: null,
          sessionPurpose: "general",
          acceptanceStatus: "accepted",
        },
        proposals: [],
        messages: [],
      });

      const workspace = await mockGetAgentConversationWorkspace(conversation.id);
      if (!workspace) {
        throw new Error("Expected seeded Agents workspace");
      }
      const linkedWorkspace = {
        ...workspace,
        linkedIdeationSessionId: sessionId,
        linkedPlanBranchId: null,
      };
      seedMockAgentConversationWorkspace(linkedWorkspace);

      mockStore.tasks.set(
        "active-plan-task-alpha",
        createMockTask({
          id: "active-plan-task-alpha",
          projectId,
          title: "Active plan task Alpha",
          internalStatus: "backlog",
          ideationSessionId: sessionId,
          planArtifactId: sessionId,
        }),
      );
      mockStore.tasks.set(
        "active-plan-task-beta",
        createMockTask({
          id: "active-plan-task-beta",
          projectId,
          title: "Other plan task Beta",
          internalStatus: "ready",
          ideationSessionId: "other-plan-session",
          planArtifactId: "other-plan-session",
        }),
      );

      queryClient.setQueryData(
        ["chat", "conversations", conversation.id],
        await mockGetConversation(conversation.id),
      );
      queryClient.setQueryData(
        ["agents", "conversation-workspace", conversation.id],
        linkedWorkspace,
      );
      queryClient.setQueryData(
        ["ideation", "sessions", "detail", sessionId, "with-data"],
        await mockIdeationApi.sessions.getWithData(sessionId),
      );
      queryClient.setQueryData(
        ["tasks", "session-history", projectId, sessionId],
        { hasHistory: true, taskCount: 1 },
      );
      queryClient.setQueryData(["agents", "artifact", planArtifactId], {
        id: planArtifactId,
        type: "design_doc",
        name: "Accepted active plan",
        content: { type: "inline", text: "# Accepted active plan" },
        metadata: {
          createdAt: now,
          createdBy: "active-plan-test",
          version: 1,
        },
        derivedFrom: [],
        bucketId: undefined,
      });
      await queryClient.invalidateQueries({
        queryKey: ["agents", "sidebar-conversations"],
      });
    },
    {
      conversation: makeConversation(),
      message: makeMessage(),
      projectId: PROJECT_ID,
      sessionId: SESSION_ID,
      planArtifactId: PLAN_ARTIFACT_ID,
    },
  );
}

async function updateActivePlan(page: Page, sessionId: string | null) {
  await page.evaluate(
    async ({ projectId, targetSessionId }) => {
      const { planApi } = await import("/src/api/plan");
      const planStore = (window as unknown as {
        __planStore?: {
          getState(): {
            loadActivePlan(projectId: string): Promise<void>;
          };
        };
      }).__planStore;

      if (!planStore) {
        throw new Error("Plan store not available - ensure the app bootstrap completed");
      }

      if (targetSessionId === null) {
        await planApi.clearActivePlan(projectId);
      } else {
        await planApi.setActivePlan(projectId, targetSessionId, "quick_switcher");
      }
      await planStore.getState().loadActivePlan(projectId);
    },
    { projectId: PROJECT_ID, targetSessionId: sessionId },
  );
}

async function hydrateIdeationArtifactCache(page: Page) {
  await page.evaluate(async (sessionId) => {
    const queryClient = window.__queryClient;
    if (!queryClient) {
      throw new Error("Expected query client to be available");
    }

    const { mockIdeationApi } = await import("/src/api-mock/ideation");
    queryClient.setQueryData(
      ["ideation", "sessions", "detail", sessionId, "with-data"],
      await mockIdeationApi.sessions.getWithData(sessionId),
    );
  }, SESSION_ID);
}

async function openAgentsTasksArtifact(page: Page) {
  await page.getByTestId("nav-agents").click();
  await expect(page.getByTestId("agents-view")).toBeVisible();

  const conversationRow = page.getByTestId(`agents-session-${CONVERSATION_ID}`);
  await expect(conversationRow).toBeVisible();
  await conversationRow.getByRole("button").first().click();
  await page.evaluate(async (conversationId) => {
    const queryClient = window.__queryClient;
    if (!queryClient) {
      throw new Error("Expected query client to be available");
    }
    const { mockGetAgentConversationWorkspace } = await import("/src/api-mock/chat");
    const workspace = await mockGetAgentConversationWorkspace(conversationId);
    if (!workspace) {
      throw new Error("Expected seeded Agents workspace");
    }
    queryClient.setQueryData(
      ["agents", "conversation-workspace", conversationId],
      workspace,
    );
  }, CONVERSATION_ID);
  await page.evaluate(async ({ projectId, conversationId }) => {
    const { useAgentSessionStore } = await import("/src/stores/agentSessionStore");
    useAgentSessionStore.getState().setRuntimeForConversation(conversationId, projectId, {
      provider: "codex",
      modelId: "gpt-5.4",
    });
  }, { projectId: PROJECT_ID, conversationId: CONVERSATION_ID });

  await page
    .getByTestId("integrated-chat-header")
    .getByRole("button", { name: "Open artifacts" })
    .click();
  await expect(page.getByTestId("agents-artifact-pane")).toBeVisible();
  await hydrateIdeationArtifactCache(page);

  const tasksTab = page.getByTestId("agents-artifact-tab-tasks");
  await expect(tasksTab).toBeVisible();
  await tasksTab.click();
  await expect(page.getByTestId("agents-artifact-content-tasks")).toBeVisible();
}

test.describe("Active Plan in the Agents Tasks artifact", () => {
  test.beforeEach(async ({ page }) => {
    await setupApp(page);
    await seedAgentsTaskScenario(page);
    await updateActivePlan(page, SESSION_ID);
  });

  test("renders only active-plan tasks in the live Graph tab", async ({ page }) => {
    await openAgentsTasksArtifact(page);

    const pane = page.getByTestId("agents-artifact-pane");
    await pane.getByRole("button", { name: "Graph" }).click();
    await expect(page.getByTestId("task-graph-view")).toBeVisible();

    const alphaNode = page
      .getByTestId("task-node")
      .filter({ hasText: "Active plan task Alpha" });
    const betaNode = page
      .getByTestId("task-node")
      .filter({ hasText: "Other plan task Beta" });

    await expect(alphaNode).toBeVisible();
    await expect(betaNode).toHaveCount(0);
  });

  test("renders only active-plan tasks in the live Kanban tab", async ({ page }) => {
    await openAgentsTasksArtifact(page);

    const pane = page.getByTestId("agents-artifact-pane");
    await pane.getByRole("button", { name: "Kanban" }).click();
    await expect(page.getByTestId("task-board")).toBeVisible();

    await expect(page.getByTestId("task-card-active-plan-task-alpha")).toBeVisible();
    await expect(page.getByTestId("task-card-active-plan-task-beta")).toHaveCount(0);
  });
});
