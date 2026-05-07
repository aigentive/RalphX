import { expect, test, type Page } from "@playwright/test";

import { setupApp } from "../../../fixtures/setup.fixtures";
import type { ChatMessageResponse } from "@/api/chat";
import type {
  AgentConversationMode,
  ChatConversation,
} from "@/types/chat-conversation";

const projectId = "project-mock-1";
const baseRef = {
  kind: "current_branch" as const,
  ref: "feature/agent-screen",
  displayName: "Current branch (feature/agent-screen)",
};

const editConversationId = "conv-agent-edit-visual";
const ideationConversationId = "conv-agent-ideation-visual";
const archivedConversationId = "conv-agent-archived-visual";

function makeConversation({
  id,
  title,
  mode,
  createdAt,
  archivedAt = null,
}: {
  id: string;
  title: string;
  mode: AgentConversationMode;
  createdAt: string;
  archivedAt?: string | null;
}): ChatConversation {
  return {
    id,
    contextType: "project",
    contextId: projectId,
    claudeSessionId: null,
    providerSessionId: `thread-${id}`,
    providerHarness: "codex",
    upstreamProvider: "openai",
    providerProfile: null,
    agentMode: mode,
    title,
    messageCount: 0,
    lastMessageAt: null,
    createdAt,
    updatedAt: createdAt,
    archivedAt,
  };
}

function makeMessage(
  conversationId: string,
  id: string,
  role: "user" | "assistant",
  content: string,
  createdAt: string,
  contentBlocks: ChatMessageResponse["contentBlocks"] = null,
): ChatMessageResponse {
  return {
    id,
    sessionId: null,
    projectId,
    taskId: null,
    role,
    content,
    metadata: null,
    parentMessageId: null,
    conversationId,
    toolCalls: null,
    contentBlocks,
    sender: null,
    attributionSource: role === "assistant" ? "provider" : "native",
    providerHarness: role === "assistant" ? "codex" : null,
    providerSessionId: role === "assistant" ? `thread-${conversationId}` : null,
    upstreamProvider: role === "assistant" ? "openai" : null,
    providerProfile: null,
    logicalModel: role === "assistant" ? "gpt-5.4" : null,
    effectiveModelId: role === "assistant" ? "gpt-5.4" : null,
    logicalEffort: role === "assistant" ? "medium" : null,
    effectiveEffort: role === "assistant" ? "medium" : null,
    inputTokens: role === "assistant" ? 4200 : null,
    outputTokens: role === "assistant" ? 380 : null,
    cacheCreationTokens: role === "assistant" ? 120 : null,
    cacheReadTokens: role === "assistant" ? 900 : null,
    estimatedUsd: null,
    createdAt,
  };
}

function seededMessages(conversationId: string): ChatMessageResponse[] {
  return [
    makeMessage(
      conversationId,
      `${conversationId}-user-1`,
      "user",
      "Please update the Agents workspace controls and make sure the commit flow is reviewable.",
      "2026-04-25T18:01:00.000Z",
    ),
    makeMessage(
      conversationId,
      `${conversationId}-assistant-1`,
      "assistant",
      "I am tightening the Agents view flow, checking the workspace diff surface, and keeping the composer responsive in split layouts.",
      "2026-04-25T18:02:00.000Z",
      [
        {
          type: "text",
          text: "I am tightening the Agents view flow and checking the workspace diff surface.",
        },
        {
          type: "tool_use",
          id: `${conversationId}-tool-read`,
          name: "read",
          arguments: {
            file_path: "frontend/src/components/agents/AgentsView.tsx",
          },
          result: {
            success: true,
            lines: 180,
          },
        },
      ],
    ),
  ];
}

async function setupAgentsView(page: Page) {
  await setupApp(page);
  await page.click('[data-testid="nav-agents"]');
  await expect(page.getByTestId("agents-view")).toBeVisible();
}

async function seedConversationWithWorkspace(
  page: Page,
  conversation: ChatConversation,
  messages: ChatMessageResponse[],
  mode: AgentConversationMode,
) {
  await page.evaluate(
    async ({ seededConversation, seededConversationMessages, seededMode, seededProjectId, seededBaseRef }) => {
      const queryClient = window.__queryClient;

      if (!queryClient) {
        throw new Error("Expected mock chat globals to be available");
      }

      const {
        mockGetAgentConversationWorkspace,
        mockGetConversation,
        seedMockConversation,
        seedMockAgentConversationWorkspace,
        mockStartAgentConversation,
      } = await import(
        "/src/api-mock/chat"
      );
      const { mockIdeationApi } = await import("/src/api-mock/ideation");

      seedMockConversation(seededConversation, seededConversationMessages);

      if (!seededConversation.archivedAt) {
        await mockStartAgentConversation({
          projectId: seededProjectId,
          content: "Seed visual workspace",
          conversationId: seededConversation.id,
          providerHarness: "codex",
          modelId: "gpt-5.4",
          mode: seededMode,
          base: seededBaseRef,
        });
      }

      const linkedIdeationSessionId =
        seededMode === "ideation"
          ? `${seededConversation.id}-ideation-session`
          : null;
      const linkedPlanArtifactId =
        seededMode === "ideation"
          ? `${seededConversation.id}-plan-artifact`
          : null;
      if (linkedIdeationSessionId && linkedPlanArtifactId) {
        const now = "2026-04-25T17:30:00.000Z";
        mockIdeationApi.sessions.seedWithData({
          session: {
            id: linkedIdeationSessionId,
            projectId: seededProjectId,
            title: "Plan Agents workspace flow",
            titleSource: null,
            status: "active",
            planArtifactId: linkedPlanArtifactId,
            seedTaskId: null,
            parentSessionId: null,
            teamMode: null,
            teamConfig: null,
            createdAt: now,
            updatedAt: now,
            archivedAt: null,
            convertedAt: "2026-04-25T18:10:00.000Z",
            verificationStatus: "verified",
            verificationInProgress: false,
            gapScore: null,
            sessionPurpose: "general",
            acceptanceStatus: "accepted",
          },
          proposals: [
            {
              id: `${seededConversation.id}-proposal`,
              sessionId: linkedIdeationSessionId,
              title: "Refine Agents workspace flow",
              description: "Keep planning artifacts visible without exposing publish controls.",
              category: "feature",
              steps: ["Review artifact tabs", "Confirm task handoff"],
              acceptanceCriteria: ["Ideation tabs are available"],
              suggestedPriority: "medium",
              priorityScore: 50,
              priorityReason: "Visual test fixture",
              estimatedComplexity: "medium",
              userPriority: null,
              userModified: false,
              status: "accepted",
              createdTaskId: null,
              planArtifactId: linkedPlanArtifactId,
              planVersionAtCreation: 1,
              sortOrder: 0,
              createdAt: now,
              updatedAt: now,
            },
          ],
          messages: [],
        });
        queryClient.setQueryData(
          ["agents", "artifact", linkedPlanArtifactId],
          {
            id: linkedPlanArtifactId,
            type: "design_doc",
            name: "Agent Plan",
            content: {
              type: "inline",
              text: "# Agent Plan\n\nTighten the Agents workspace and keep artifact routing clear.",
            },
            metadata: {
              createdAt: now,
              createdBy: "visual-fixture",
              version: 1,
            },
            derivedFrom: [],
            bucketId: undefined,
          },
        );
      }

      const hydratedConversation = await mockGetConversation(seededConversation.id);
      queryClient.setQueryData(
        ["chat", "conversations", seededConversation.id],
        hydratedConversation,
      );

      const workspace = await mockGetAgentConversationWorkspace(seededConversation.id);
      const hydratedWorkspace =
        workspace && linkedIdeationSessionId
          ? {
              ...workspace,
              linkedIdeationSessionId,
              linkedPlanBranchId: null,
            }
          : workspace;
      queryClient.setQueryData(
        ["agents", "conversation-workspace", seededConversation.id],
        hydratedWorkspace,
      );
      if (hydratedWorkspace) {
        seedMockAgentConversationWorkspace(hydratedWorkspace);
      }
    },
    {
      seededConversation: conversation,
      seededConversationMessages: messages,
      seededMode: mode,
      seededProjectId: projectId,
      seededBaseRef: baseRef,
    },
  );
}

async function selectAgentConversation(
  page: Page,
  conversationId: string,
) {
  const row = page.getByTestId(`agents-session-${conversationId}`);
  await expect(row).toBeVisible();
  await row.getByRole("button").first().click();

  await page.evaluate(
    async ({ selectedProjectId, selectedConversationId }) => {
      const { useAgentSessionStore } = await import("/src/stores/agentSessionStore");
      const store = useAgentSessionStore.getState();

      store.setRuntimeForConversation(selectedConversationId, selectedProjectId, {
        provider: "codex",
        modelId: "gpt-5.4",
      });
    },
    {
      selectedProjectId: projectId,
      selectedConversationId: conversationId,
    },
  );
}

async function seedPublishHistory(page: Page, conversationId: string) {
  await page.evaluate(async (targetConversationId) => {
    const queryClient = window.__queryClient;
    if (!queryClient) {
      throw new Error("Expected query client to be available");
    }
    const {
      mockGetAgentConversationWorkspace,
      mockListAgentConversationWorkspacePublicationEvents,
      mockPublishAgentConversationWorkspace,
    } = await import("/src/api-mock/chat");

    const result = await mockPublishAgentConversationWorkspace(targetConversationId);
    const workspace =
      result.workspace ?? await mockGetAgentConversationWorkspace(targetConversationId);
    const events = await mockListAgentConversationWorkspacePublicationEvents(
      targetConversationId,
    );

    queryClient.setQueryData(
      ["agents", "conversation-workspace-publication-events", targetConversationId],
      events,
    );
    queryClient.setQueryData(
      ["agents", "conversation-workspace", targetConversationId],
      workspace,
    );
  }, conversationId);
}

async function hydrateIdeationArtifactCache(page: Page, conversationId: string) {
  await page.evaluate(async (targetConversationId) => {
    const queryClient = window.__queryClient;
    if (!queryClient) {
      throw new Error("Expected query client to be available");
    }

    const { mockIdeationApi } = await import("/src/api-mock/ideation");
    const sessionId = `${targetConversationId}-ideation-session`;
    const sessionData = await mockIdeationApi.sessions.getWithData(sessionId);

    queryClient.setQueryData(
      ["ideation", "sessions", "detail", sessionId, "with-data"],
      sessionData,
    );
  }, conversationId);
}

async function seedAgentsScenario(page: Page) {
  await page.evaluate(() => {
    window.__mockChatApi?.reset();
  });

  await seedConversationWithWorkspace(
    page,
    makeConversation({
      id: editConversationId,
      title: "Update Agents workspace flow",
      mode: "edit",
      createdAt: "2026-04-25T18:00:00.000Z",
    }),
    seededMessages(editConversationId),
    "edit",
  );
  await seedConversationWithWorkspace(
    page,
    makeConversation({
      id: ideationConversationId,
      title: "Plan Agents workspace flow",
      mode: "ideation",
      createdAt: "2026-04-25T17:30:00.000Z",
    }),
    seededMessages(ideationConversationId),
    "ideation",
  );
  await seedConversationWithWorkspace(
    page,
    makeConversation({
      id: archivedConversationId,
      title: "Archived workspace investigation",
      mode: "edit",
      createdAt: "2026-04-24T09:00:00.000Z",
      archivedAt: "2026-04-25T16:00:00.000Z",
    }),
    seededMessages(archivedConversationId),
    "edit",
  );

  await page.evaluate(async () => {
    const queryClient = window.__queryClient;
    if (!queryClient) {
      throw new Error("Expected query client to be available");
    }
    const { mockListConversationsPage } = await import("/src/api-mock/chat");
    const activePage = await mockListConversationsPage(
      "project",
      "project-mock-1",
      6,
      0,
      false,
      undefined,
      false,
    );
    const archivedPage = await mockListConversationsPage(
      "project",
      "project-mock-1",
      6,
      0,
      true,
      undefined,
      true,
    );
    const toAgentConversation = (conversation: (typeof activePage.conversations)[number]) => ({
      ...conversation,
      projectId: conversation.contextId,
      ideationSessionId: null,
    });

    queryClient.setQueryData(
      [
        "agents",
        "project-conversations",
        "project-mock-1",
        "archived",
        false,
        "search",
        "",
      ],
      {
        pages: [
          {
            ...activePage,
            conversations: activePage.conversations.map(toAgentConversation),
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData(
      [
        "agents",
        "project-conversations",
        "project-mock-1",
        "archived",
        true,
        "search",
        "",
      ],
      {
        pages: [
          {
            ...archivedPage,
            conversations: archivedPage.conversations.map(toAgentConversation),
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData(
      ["agents", "project-conversations", "project-mock-1", "archived-count"],
      {
        ...archivedPage,
        limit: 1,
        conversations: archivedPage.conversations.slice(0, 1),
      },
    );
  });
}

async function seedGitAuthRepairIssue(page: Page) {
  await page.evaluate(() => {
    window.__mockGhAuthStatus = true;
    window.__mockGitAuthDiagnostics = {
      fetchUrl: "https://github.com/mock/project.git",
      pushUrl: "git@github.com:mock/project.git",
      fetchKind: "HTTPS",
      pushKind: "SSH",
      mixedAuthModes: true,
      canSwitchToSsh: true,
      suggestedSshUrl: "git@github.com:mock/project.git",
    };
  });
}

test.describe("Agents View", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.addInitScript(() => {
      window.localStorage.clear();
    });
  });

  test("starter composer and action menu match visual contract", async ({ page }) => {
    await setupAgentsView(page);
    await expect(page.getByTestId("agents-start-composer")).toBeVisible();

    await page.getByTestId("agent-composer-actions-menu").click();
    await expect(page.getByTestId("agents-start-mode-edit")).toBeVisible();
    await expect(page.getByTestId("agents-start-mode-chat")).toBeVisible();
    await expect(page.getByTestId("agents-start-mode-ideation")).toBeVisible();
    await expect(page.getByText("Build, change, and review code in a branch.")).toBeVisible();

    await expect(page).toHaveScreenshot("agents-start-composer-actions.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("edit workspace with commit publish pane matches visual contract", async ({ page }) => {
    await setupAgentsView(page);
    await seedAgentsScenario(page);
    await selectAgentConversation(page, editConversationId);

    await expect(page.getByTestId(`agents-session-${editConversationId}`)).toBeVisible();
    await expect(page.getByTestId("integrated-chat-messages")).toBeVisible();
    await expect(page.getByTestId("agents-publish-workspace")).toBeVisible();
    await page.getByTestId("agents-publish-workspace").click();
    await expect(page.getByTestId("agents-publish-pane")).toBeVisible();
    await expect(page.getByTestId("agents-review-changes")).toBeEnabled();

    await seedPublishHistory(page, editConversationId);
    await expect(page.getByTestId("agents-publish-events")).toBeVisible();
    await page.getByTestId("agents-publish-history-toggle").click();
    await expect(page.getByTestId("agents-publish-event-published")).toBeVisible();

    await expect(page).toHaveScreenshot("agents-edit-publish-pane.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("commit publish pane shows git auth repair actions", async ({ page }) => {
    await setupAgentsView(page);
    await seedGitAuthRepairIssue(page);
    await seedAgentsScenario(page);
    await selectAgentConversation(page, editConversationId);

    await expect(page.getByTestId("agents-publish-workspace")).toBeVisible();
    await page.getByTestId("agents-publish-workspace").click();
    await expect(page.getByTestId("agents-publish-pane")).toBeVisible();
    await expect(page.getByTestId("git-auth-repair-panel")).toBeVisible();
    await expect(page.getByTestId("git-auth-switch-ssh")).toBeVisible();
    await expect(page.getByTestId("git-auth-setup-gh")).toBeVisible();

    await expect(page).toHaveScreenshot("agents-publish-git-auth-repair.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("ideation workspace shows only ideation artifacts", async ({ page }) => {
    await setupAgentsView(page);
    await seedAgentsScenario(page);
    await selectAgentConversation(page, ideationConversationId);

    await expect(page.getByTestId(`agents-session-${ideationConversationId}`)).toBeVisible();
    await expect(page.getByTestId("integrated-chat-messages")).toBeVisible();
    await page
      .getByTestId("integrated-chat-header")
      .getByRole("button", { name: "Open artifacts" })
      .click();
    await expect(page.getByTestId("agents-artifact-pane")).toBeVisible();
    await hydrateIdeationArtifactCache(page, ideationConversationId);
    await expect(page.getByTestId("agents-artifact-tab-plan")).toBeVisible();
    await expect(page.getByTestId("agents-artifact-tab-verification")).toBeVisible();
    await expect(page.getByTestId("agents-artifact-tab-proposal")).toBeVisible();
    await expect(page.getByTestId("agents-artifact-tab-tasks")).toBeVisible();
    await expect(page.getByTestId("agents-artifact-tab-publish")).toHaveCount(0);

    await expect(page).toHaveScreenshot("agents-ideation-artifacts.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("v27 sidebar tree and static recent block match visual contract", async ({ page }) => {
    await setupAgentsView(page);
    await seedAgentsScenario(page);

    await expect(page.getByTestId("agents-show-all-projects-pill")).toBeVisible();
    await expect(page.getByTestId("agents-project-sort-pill")).toBeVisible();
    await expect(page.getByTestId("agents-show-archived-pill")).toBeVisible();
    // Static "Recent" block is now hidden ("Coming soon") on the polished sidebar — present in DOM but aria-hidden + display:none.
    await expect(page.getByTestId("agents-static-recent")).toHaveAttribute("aria-hidden", "true");
    await expect(page.getByTestId(`agents-session-${editConversationId}`)).toBeVisible();
    await expect(page.getByTestId(`agents-session-${archivedConversationId}`)).toHaveCount(0);

    await expect(page).toHaveScreenshot("agents-v27-sidebar-recent.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("split-pane composer collapses secondary send text in compact containers", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await setupAgentsView(page);
    await seedAgentsScenario(page);
    await selectAgentConversation(page, editConversationId);
    await expect(page.getByTestId("agents-publish-workspace")).toBeVisible();
    await page.getByTestId("agents-publish-workspace").click();
    await expect(page.getByTestId("agents-publish-pane")).toBeVisible();

    const submitButton = page.getByTestId("agents-conversation-submit");
    await expect(submitButton).toBeVisible();
    await expect(submitButton.locator(".agent-composer-action-label")).toBeHidden();

    await expect(page).toHaveScreenshot("agents-compact-split-composer.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });
});
