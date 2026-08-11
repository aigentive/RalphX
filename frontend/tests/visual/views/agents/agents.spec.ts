import { expect, test, type Page } from "@playwright/test";

import { seedAutomationRuntimeVisualState } from "../../../fixtures/agents-automation-runtime.fixtures";
import {
  dismissProviderCliUpdateToasts,
  setupApp,
} from "../../../fixtures/setup.fixtures";
import { revealAgentInboxConversation } from "../../../helpers/agents-inbox.helpers";
import {
  AgentsPublishPage,
  type WorkspaceReviewVisualState,
} from "../../../pages/views/agents-publish.page";
import { AgentsRuntimePage } from "../../../pages/views/agents-runtime.page";
import type { StateTransition } from "@/api/tasks";
import type { ChatMessageResponse } from "@/api/chat";
import type {
  AgentConversationMode,
  ChatConversation,
} from "@/types/chat-conversation";
import type { InternalStatus, Task } from "@/types/task";
import type { TaskStep } from "@/types/task-step";

const projectId = "project-mock-1";
const baseRef = {
  kind: "current_branch" as const,
  ref: "feature/agent-screen",
  displayName: "Current branch (feature/agent-screen)",
};

const editConversationId = "conv-agent-edit-visual";
const ideationConversationId = "conv-agent-ideation-visual";
const automationConversationId = "conv-agent-automation-visual";
const archivedConversationId = "conv-agent-archived-visual";
const stablePublishEventCreatedAt = "2026-05-13T05:20:00";

const taskDetailVisualTaskIds = {
  executing: "agents-task-detail-executing",
  reviewing: "agents-task-detail-reviewing",
  reviewPassed: "agents-task-detail-review-passed",
  approved: "agents-task-detail-approved",
  merged: "agents-task-detail-merged",
} as const;

const taskDetailVisualStates: Array<{
  id: string;
  status: InternalStatus;
  title: string;
  detailTestId: string;
}> = [
  {
    id: taskDetailVisualTaskIds.executing,
    status: "executing",
    title: "Executing task detail parity",
    detailTestId: "execution-task-detail",
  },
  {
    id: taskDetailVisualTaskIds.reviewing,
    status: "reviewing",
    title: "Reviewing task detail parity",
    detailTestId: "reviewing-task-detail",
  },
  {
    id: taskDetailVisualTaskIds.reviewPassed,
    status: "review_passed",
    title: "Review passed task detail parity",
    detailTestId: "human-review-task-detail",
  },
  {
    id: taskDetailVisualTaskIds.approved,
    status: "approved",
    title: "Approved task detail parity",
    detailTestId: "completed-task-detail",
  },
  {
    id: taskDetailVisualTaskIds.merged,
    status: "merged",
    title: "Merged task detail parity",
    detailTestId: "merged-task-detail",
  },
];

function makePagedDiffRows() {
  return [
    { kind: "hunk_header", header: "@@ -1,5 +1,7 @@" },
    {
      kind: "line",
      line: {
        kind: "context",
        content: "import { cn } from \"@/lib/utils\";",
        old_line_num: 1,
        new_line_num: 1,
      },
    },
    {
      kind: "line",
      line: {
        kind: "deletion",
        content: "const stickyGutter = true;",
        old_line_num: 2,
        new_line_num: null,
      },
    },
    {
      kind: "line",
      line: {
        kind: "addition",
        content: "const stickyGutter = false;",
        old_line_num: null,
        new_line_num: 2,
      },
    },
    {
      kind: "line",
      line: {
        kind: "addition",
        content: "const inlineRowsArePaged = true;",
        old_line_num: null,
        new_line_num: 3,
      },
    },
    {
      kind: "line",
      line: {
        kind: "context",
        content: "export function renderWorkspaceDiff() {",
        old_line_num: 4,
        new_line_num: 5,
      },
    },
    {
      kind: "line",
      line: {
        kind: "addition",
        content: "  return \"bounded page window\";",
        old_line_num: null,
        new_line_num: 6,
      },
    },
    {
      kind: "line",
      line: {
        kind: "context",
        content: "}",
        old_line_num: 5,
        new_line_num: 7,
      },
    },
  ];
}

async function installPagedDiffRoute(page: Page) {
  await page.route("**/api/agent-workspaces/**/file-diff-page**", async (route) => {
    const url = new URL(route.request().url());
    const filePath = url.searchParams.get("path") ?? "mock-file.ts";
    const offset = Math.max(0, Number(url.searchParams.get("offset") ?? "0"));
    const limit = Math.max(1, Number(url.searchParams.get("limit") ?? "200"));
    const rows = makePagedDiffRows();
    const pageRows = rows.slice(offset, offset + limit);
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        file_path: filePath,
        language: filePath.endsWith(".rs")
          ? "rust"
          : filePath.endsWith(".yaml") || filePath.endsWith(".yml")
            ? "yaml"
            : filePath.endsWith(".tsx")
              ? "tsx"
              : "text",
        rows: pageRows,
        offset,
        limit,
        next_offset: offset + limit < rows.length ? offset + limit : null,
        total_rows: rows.length,
        old_total_lines: 5,
        new_total_lines: 7,
        is_binary: false,
      }),
    });
  });
}

function makeConversation({
  id,
  title,
  mode,
  createdAt,
  archivedAt = null,
  automationId = null,
}: {
  id: string;
  title: string;
  mode: AgentConversationMode;
  createdAt: string;
  archivedAt?: string | null;
  automationId?: string | null;
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
    automationId,
    automationRunId: null,
    coordinationMode: "solo",
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
  await installPagedDiffRoute(page);
  await setupApp(page);
  await page.click('[data-testid="nav-agents"]');
  await expect(page.getByTestId("agents-view")).toBeVisible();
}

async function enableStarterCapabilityFixture(page: Page) {
  await page.evaluate(() => {
    const queryClient = window.__queryClient;
    if (!queryClient) {
      throw new Error("Expected query client for capability fixture");
    }
    const current =
      queryClient.getQueryData<Record<string, boolean>>(["featureFlags"]) ?? {};
    queryClient.setQueryData(["featureFlags"], {
      ...current,
      agentConversationTeam: true,
    });
  });
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
            artifactRole: "overview",
            planContractVersion: 2,
            blueprint: {
              id: `${seededConversation.id}-plan-blueprint-artifact`,
              type: "design_doc",
              name: "Agent Plan Blueprint",
              content: {
                type: "inline",
                text: "# Blueprint\n\nImplement the Agents workspace plan.",
              },
              metadata: {
                createdAt: now,
                createdBy: "visual-fixture",
                version: 1,
              },
              derivedFrom: [],
              bucketId: undefined,
              artifactRole: "blueprint",
            },
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
  const row = await revealAgentInboxConversation(page, conversationId);
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
  const published = await page.evaluate(async (targetConversationId) => {
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

    return { events, workspace };
  }, conversationId);

  await seedPublishedPrToolbarHealth(page, conversationId, published.workspace);
  await page.evaluate(
    ({ targetConversationId, events, workspace }) => {
      const queryClient = window.__queryClient;
      if (!queryClient) {
        throw new Error("Expected query client to be available");
      }
      queryClient.setQueryData(
        [
          "agents",
          "conversation-workspace-publication-events",
          targetConversationId,
        ],
        events,
      );
      queryClient.setQueryData(
        ["agents", "conversation-workspace", targetConversationId],
        workspace,
      );
    },
    {
      targetConversationId: conversationId,
      events: published.events,
      workspace: published.workspace,
    },
  );
}

async function seedPublishedPrToolbarHealth(
  page: Page,
  conversationId: string,
  publishedWorkspace?: {
    projectId: string;
    publicationPrNumber: number | null;
    publicationPrUrl: string | null;
    branchName: string;
  },
) {
  await page.evaluate(
    async ({ targetConversationId, publishedWorkspace }) => {
      const queryClient = window.__queryClient;
      if (!queryClient) {
        throw new Error("Expected query client to be available");
      }
      const workspace =
        publishedWorkspace ??
        queryClient.getQueryData<{
          projectId: string;
          publicationPrNumber: number | null;
          publicationPrUrl: string | null;
          branchName: string;
        }>(["agents", "conversation-workspace", targetConversationId]);
      if (!workspace?.publicationPrNumber) {
        throw new Error("Expected a published PR workspace");
      }
      const { prKeys } = await import("/src/hooks/usePullRequestDetail");
      const selector = {
        projectId: workspace.projectId,
        prNumber: workspace.publicationPrNumber,
      };
      queryClient.setQueryData(
        prKeys.detail(selector),
        {
          state: "loaded",
          origin: "ownedOutbound",
          description: {
            number: workspace.publicationPrNumber,
            title: "Persistent Agents workspace toolbar",
            body: "Keep workspace identity and pull request health visible across every artifact tab.",
            author: "ralphx",
            createdAt: "2026-05-13T05:20:00Z",
            url: workspace.publicationPrUrl,
            state: "open",
            isDraft: false,
            headRefName: workspace.branchName,
            baseRefName: "main",
          },
          checks: [
            {
              name: "Frontend tests",
              status: "completed",
              conclusion: "success",
              detailsUrl: null,
            },
            {
              name: "Native UI",
              status: "in_progress",
              conclusion: null,
              detailsUrl: null,
            },
          ],
          reviewSummary: {
            reviewDecision: "APPROVED",
            latestChangesRequestedAuthor: null,
            latestChangesRequestedBody: null,
            latestChangesRequestedSubmittedAt: null,
            latestChangesRequestedComments: [],
          },
          issueComments: [],
          reviewThread: [],
          rxConversations: [],
          linkedTickets: [],
          sourcesUnavailable: [],
        },
        { updatedAt: Date.now() + 60 * 60 * 1000 },
    );
    },
    { targetConversationId: conversationId, publishedWorkspace },
  );
}

async function hydratePublishHistoryCache(page: Page, conversationId: string) {
  await page.evaluate(async (targetConversationId) => {
    const queryClient = window.__queryClient;
    if (!queryClient) {
      throw new Error("Expected query client to be available");
    }
    const { mockListAgentConversationWorkspacePublicationEvents } =
      await import("/src/api-mock/chat");
    const events =
      await mockListAgentConversationWorkspacePublicationEvents(
        targetConversationId,
      );
    queryClient.setQueryData(
      ["agents", "conversation-workspace-publication-events", targetConversationId],
      events,
    );
  }, conversationId);
}

async function stabilizePublishHistoryTimestamps(page: Page, conversationId: string) {
  await page.evaluate(
    ({ targetConversationId, createdAt }) => {
      const queryClient = window.__queryClient;
      if (!queryClient) {
        throw new Error("Expected query client to be available");
      }
      const queryKey = [
        "agents",
        "conversation-workspace-publication-events",
        targetConversationId,
      ];
      const events = queryClient.getQueryData(queryKey);
      if (!Array.isArray(events)) {
        return;
      }
      queryClient.setQueryData(
        queryKey,
        events.map((event) => ({ ...event, createdAt })),
      );
    },
    {
      targetConversationId: conversationId,
      createdAt: stablePublishEventCreatedAt,
    },
  );
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
    const planArtifactId = sessionData?.session.planArtifactId;
    const planArtifact = planArtifactId
      ? queryClient.getQueryData(["agents", "artifact", planArtifactId])
      : null;

    queryClient.setQueryData(
      ["ideation", "sessions", "detail", sessionId, "with-data"],
      sessionData,
    );
    if (planArtifactId && planArtifact) {
      queryClient.setQueryData(
        ["agents", "session-plan", sessionId, planArtifactId],
        planArtifact,
      );
    }
  }, conversationId);
}

function taskDetailVisualTransitions(taskId: string): StateTransition[] {
  return [
    {
      fromStatus: "ready",
      toStatus: "executing",
      trigger: "agent",
      timestamp: "2026-07-07T10:00:00.000Z",
      conversationId: "agents-task-detail-exec-1",
      agentRunId: "run-exec-1",
    },
    {
      fromStatus: "executing",
      toStatus: "reviewing",
      trigger: "system",
      timestamp: "2026-07-07T10:10:00.000Z",
      conversationId: "agents-task-detail-review-1",
      agentRunId: "run-review-1",
    },
    {
      fromStatus: "reviewing",
      toStatus: "revision_needed",
      trigger: "system",
      timestamp: "2026-07-07T10:18:00.000Z",
    },
    {
      fromStatus: "revision_needed",
      toStatus: "re_executing",
      trigger: "agent",
      timestamp: "2026-07-07T10:22:00.000Z",
      conversationId: "agents-task-detail-exec-2",
      agentRunId: "run-exec-2",
    },
    {
      fromStatus: "re_executing",
      toStatus: "reviewing",
      trigger: "system",
      timestamp: "2026-07-07T10:30:00.000Z",
      conversationId: "agents-task-detail-review-2",
      agentRunId: "run-review-2",
    },
    {
      fromStatus: "reviewing",
      toStatus: "review_passed",
      trigger: "system",
      timestamp: "2026-07-07T10:38:00.000Z",
      conversationId: "agents-task-detail-review-2",
      agentRunId: "run-review-2",
    },
    {
      fromStatus: "review_passed",
      toStatus: "approved",
      trigger: "system",
      timestamp: "2026-07-07T10:42:00.000Z",
    },
    {
      fromStatus: "approved",
      toStatus: "pending_merge",
      trigger: "system",
      timestamp: "2026-07-07T10:45:00.000Z",
    },
    {
      fromStatus: "pending_merge",
      toStatus: "merged",
      trigger: "system",
      timestamp: "2026-07-07T10:50:00.000Z",
      conversationId: "agents-task-detail-merge-1",
      agentRunId: "run-merge-1",
      contextType: "merge",
    },
  ].map((transition) => ({ ...transition, taskId })) as StateTransition[];
}

async function seedAgentsTaskDetailVisualState(page: Page) {
  await page.evaluate(
    async ({
      targetConversationId,
      seededProjectId,
      taskStates,
      mergedTaskId,
      mergedTransitions,
    }) => {
      const queryClient = window.__queryClient;
      const mockStore = window.__mockStore as
        | {
            tasks: Map<string, Task>;
            taskSteps: Map<string, TaskStep[]>;
          }
        | undefined;

      if (!queryClient || !mockStore) {
        throw new Error("Expected mock store and query client to be available");
      }

      const { createMockTask, generateTestUuid } = await import("/src/test/mock-data");
      const { AGENT_WORKER } = await import("/src/constants/agents");
      const { seedMockConversation } = await import("/src/api-mock/chat");
      const { infiniteTaskKeys } = await import("/src/hooks/useInfiniteTasksQuery");
      const linkedSessionId = `${targetConversationId}-ideation-session`;
      const linkedExecutionPlanId = `${linkedSessionId}-execution-plan`;
      const now = "2026-07-07T10:55:00.000Z";
      const seededTasks: Task[] = [];

      for (const visualTask of taskStates) {
        const task = createMockTask({
          id: visualTask.id,
          projectId: seededProjectId,
          title: visualTask.title,
          description:
            "Agents detail parity fixture with long-enough copy to reveal cramped two-column regressions in the right panel.",
          category: "feature",
          internalStatus: visualTask.status,
          priority: 2,
          ideationSessionId: linkedSessionId,
          executionPlanId: linkedExecutionPlanId,
          planArtifactId: linkedSessionId,
          taskBranch: `task/${visualTask.id}`,
          startedAt: "2026-07-07T10:00:00.000Z",
          completedAt:
            visualTask.status === "approved" || visualTask.status === "merged"
              ? "2026-07-07T10:55:00.000Z"
              : null,
          mergeCommitSha:
            visualTask.status === "merged" ? "abc123def4567890" : null,
        });
        mockStore.tasks.set(visualTask.id, task);
        seededTasks.push(task);
        mockStore.taskSteps.set(visualTask.id, [
          {
            id: generateTestUuid(),
            taskId: visualTask.id,
            title: "Map current state",
            description: "Review the state-specific body.",
            status: "completed",
            sortOrder: 0,
            dependsOn: null,
            createdBy: AGENT_WORKER,
            completionNote: "Mapped",
            createdAt: now,
            updatedAt: now,
            startedAt: now,
            completedAt: now,
          },
          {
            id: generateTestUuid(),
            taskId: visualTask.id,
            title: "Verify shell order",
            description: "Confirm summary, stage body, evidence, and context order.",
            status:
              visualTask.status === "executing" || visualTask.status === "reviewing"
                ? "in_progress"
                : "completed",
            sortOrder: 1,
            dependsOn: null,
            createdBy: AGENT_WORKER,
            completionNote:
              visualTask.status === "executing" || visualTask.status === "reviewing"
                ? null
                : "Verified",
            createdAt: now,
            updatedAt: now,
            startedAt: now,
            completedAt:
              visualTask.status === "executing" || visualTask.status === "reviewing"
                ? null
                : now,
          },
        ]);
      }

      for (const runtimeConversation of [
        {
          id: "agents-task-detail-exec-1",
          contextType: "task_execution" as const,
          label: "Execution attempt 1",
        },
        {
          id: "agents-task-detail-review-1",
          contextType: "review" as const,
          label: "Review attempt 1",
        },
        {
          id: "agents-task-detail-exec-2",
          contextType: "task_execution" as const,
          label: "Execution attempt 2",
        },
        {
          id: "agents-task-detail-review-2",
          contextType: "review" as const,
          label: "Review attempt 2",
        },
        {
          id: "agents-task-detail-merge-1",
          contextType: "merge" as const,
          label: "Merge attempt 1",
        },
      ]) {
        seedMockConversation(
          {
            id: runtimeConversation.id,
            contextType: runtimeConversation.contextType,
            contextId: mergedTaskId,
            claudeSessionId: null,
            providerSessionId: `thread-${runtimeConversation.id}`,
            providerHarness: "codex",
            upstreamProvider: "openai",
            providerProfile: null,
            agentMode: "edit",
            title: runtimeConversation.label,
            messageCount: 0,
            lastMessageAt: null,
            createdAt: now,
            updatedAt: now,
            archivedAt: null,
          } as ChatConversation,
          [
            {
              id: `${runtimeConversation.id}-assistant`,
              sessionId: null,
              projectId: seededProjectId,
              taskId: mergedTaskId,
              role: "assistant",
              content: `${runtimeConversation.label} transcript for Agents detail parity.`,
              metadata: null,
              parentMessageId: null,
              conversationId: runtimeConversation.id,
              toolCalls: null,
              contentBlocks: null,
              sender: null,
              attributionSource: "provider",
              providerHarness: "codex",
              providerSessionId: `thread-${runtimeConversation.id}`,
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
              createdAt: now,
            } as ChatMessageResponse,
          ],
        );
      }

      queryClient.setQueryData(
        ["stateTransitions", mergedTaskId],
        mergedTransitions,
      );
      queryClient.setQueryData(
        ["tasks", "session-history", seededProjectId, linkedSessionId],
        { hasHistory: true, taskCount: seededTasks.length },
      );
      queryClient.setQueryData(["tasks", "list", seededProjectId], (existing: Task[] | undefined) => {
        const withoutSeeded = (existing ?? []).filter(
          (task) => !taskStates.some((visualTask) => visualTask.id === task.id),
        );
        return [...seededTasks, ...withoutSeeded];
      });

      const sessionQueryKey = [
        "ideation",
        "sessions",
        "detail",
        linkedSessionId,
        "with-data",
      ];
      queryClient.setQueryData(sessionQueryKey, (existing: { proposals?: unknown[] } | undefined) => {
        if (!existing) return existing;
        const existingProposals = existing.proposals ?? [];
        return {
          ...existing,
          proposals: [
            ...existingProposals,
            ...seededTasks.map((task, index) => ({
              id: `${task.id}-proposal`,
              sessionId: linkedSessionId,
              title: task.title,
              description: task.description,
              category: task.category,
              steps: ["Open task detail", "Verify one-column layout"],
              acceptanceCriteria: ["Task detail remains one-column"],
              suggestedPriority: "medium",
              priorityScore: 50,
              priorityReason: "Visual test fixture",
              estimatedComplexity: "medium",
              userPriority: null,
              userModified: false,
              status: "accepted",
              createdTaskId: task.id,
              planArtifactId: task.planArtifactId,
              planVersionAtCreation: 1,
              sortOrder: index + 10,
              createdAt: now,
              updatedAt: now,
            })),
          ],
        };
      });

      const activeColumns =
        queryClient.getQueryData<Array<{ mapsTo: InternalStatus; groups?: Array<{ statuses: InternalStatus[] }> }>>([
          "workflows",
          "activeColumns",
        ]) ?? [];
      const cacheColumn = (
        statuses: InternalStatus[],
        ideationSessionId: string | null | undefined,
      ) => {
        const tasks = seededTasks.filter((task) =>
          statuses.includes(task.internalStatus),
        );
        queryClient.setQueryData(
          infiniteTaskKeys.list({
            projectId: seededProjectId,
            statuses,
            includeArchived: false,
            ideationSessionId,
            executionPlanId: null,
          }),
          {
            pages: [
              {
                tasks,
                total: tasks.length,
                offset: 0,
                hasMore: false,
              },
            ],
            pageParams: [0],
          },
        );
      };
      for (const column of activeColumns) {
        const statuses =
          column.groups && column.groups.length > 0
            ? Array.from(new Set(column.groups.flatMap((group) => group.statuses)))
            : [column.mapsTo];
        cacheColumn(statuses, linkedSessionId);
        cacheColumn(statuses, null);
        cacheColumn(statuses, undefined);
      }
      for (const statuses of [
        ["executing"],
        ["reviewing"],
        ["review_passed"],
        ["approved"],
        ["merged"],
        ["executing", "re_executing", "qa_refining", "qa_testing", "qa_passed", "qa_failed"],
        ["pending_review", "reviewing", "review_passed", "revision_needed", "escalated"],
        ["approved", "merged"],
        [
          "pending_merge",
          "merging",
          "waiting_on_pr",
          "merge_incomplete",
          "merge_conflict",
          "merged",
        ],
      ] as InternalStatus[][]) {
        cacheColumn(statuses, linkedSessionId);
        cacheColumn(statuses, null);
        cacheColumn(statuses, undefined);
      }
    },
    {
      targetConversationId: ideationConversationId,
      seededProjectId: projectId,
      taskStates: taskDetailVisualStates,
      mergedTaskId: taskDetailVisualTaskIds.merged,
      mergedTransitions: taskDetailVisualTransitions(taskDetailVisualTaskIds.merged),
    },
  );
}

async function expectAgentsTaskDetailOneColumn(page: Page, detailTestId: string) {
  const shell = page.getByTestId(detailTestId);
  await expect(shell).toBeVisible();
  await expect(shell).not.toHaveClass(/grid/);
  const summary = page.getByTestId("task-detail-summary");
  const stageBody = page.getByTestId("task-detail-stage-body");
  const frame = page.getByTestId("task-detail-content-frame");
  await expect(summary).toBeVisible();
  await expect(stageBody).toBeVisible();

  const [summaryBox, stageBodyBox] = await Promise.all([
    summary.boundingBox(),
    stageBody.boundingBox(),
  ]);
  expect(summaryBox).not.toBeNull();
  expect(stageBodyBox).not.toBeNull();
  expect(stageBodyBox!.y).toBeGreaterThan(summaryBox!.y);

  const horizontalOverflow = await frame.evaluate(
    (element) => element.scrollWidth - element.clientWidth,
  );
  expect(horizontalOverflow).toBeLessThanOrEqual(2);
}

async function expectPublishVisualAtWidths(
  page: Page,
  publishPage: AgentsPublishPage,
  snapshotName: string,
) {
  const standardViewport = page.viewportSize() ?? { width: 1280, height: 720 };
  await publishPage.expectNoPaneOverflow();
  await expect(page).toHaveScreenshot(`${snapshotName}.png`, {
    fullPage: false,
    maxDiffPixelRatio: 0.01,
  });

  await page.setViewportSize({ width: 960, height: standardViewport.height });
  await publishPage.expectNoPaneOverflow();
  await expect(page).toHaveScreenshot(`${snapshotName}-constrained.png`, {
    fullPage: false,
    maxDiffPixelRatio: 0.01,
  });
  await page.setViewportSize(standardViewport);
}

const workspaceReviewVisualLabels: Record<WorkspaceReviewVisualState, string> = {
  running: "Reviewing",
  blocking: "Review blocking",
  passed: "Review passed",
};

async function seedAgentsScenario(
  page: Page,
  options: { includeAutomation?: boolean } = {},
) {
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
  if (options.includeAutomation) {
    await seedConversationWithWorkspace(
      page,
      makeConversation({
        id: automationConversationId,
        title: "Release readiness automation",
        mode: "automation",
        automationId: "automation-visual-1",
        createdAt: "2026-04-25T17:45:00.000Z",
      }),
      seededMessages(automationConversationId),
      "automation",
    );
  }
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
    const {
      mockListAgentSidebarConversations,
      mockListConversationsPage,
    } = await import("/src/api-mock/chat");
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

    const publicationStates = [
      "active",
      "draft",
      "merged",
      "closed",
      "uncommitted",
      "unpushed",
    ];
    const sidebarGroupPageSize = 8;
    const activeSidebarResponse = await mockListAgentSidebarConversations({
      projectIds: ["project-mock-1"],
      includeArchived: false,
      archivedOnly: false,
      publicationStates,
      groupBy: "project",
      limitPerGroup: sidebarGroupPageSize,
      offsets: { "project-mock-1": 0 },
      pinnedConversationIds: [],
      priorityConversationIds: [],
    });
    const archivedSidebarResponse = await mockListAgentSidebarConversations({
      projectIds: ["project-mock-1"],
      includeArchived: true,
      archivedOnly: true,
      publicationStates,
      groupBy: "project",
      limitPerGroup: sidebarGroupPageSize,
      offsets: { "project-mock-1": 0 },
      pinnedConversationIds: [],
      priorityConversationIds: [],
    });
    const activeSidebarGroup = activeSidebarResponse.groups.find(
      (group) => group.key === "project-mock-1",
    );
    const archivedSidebarGroup = archivedSidebarResponse.groups.find(
      (group) => group.key === "project-mock-1",
    );
    if (!activeSidebarGroup || !archivedSidebarGroup) {
      throw new Error("Expected seeded sidebar groups");
    }
    queryClient.setQueryData(
      [
        "agents",
        "sidebar-conversations",
        "project",
        "project-mock-1",
        "archived",
        false,
        "search",
        "",
        "states",
        publicationStates,
        "pinned",
        [],
        "priority",
        [],
        "page-size",
        sidebarGroupPageSize,
        "initial-limit",
        sidebarGroupPageSize,
      ],
      {
        pages: [activeSidebarGroup],
        pageParams: [0],
      },
    );
    queryClient.setQueryData(
      [
        "agents",
        "sidebar-conversations",
        "project",
        "project-mock-1",
        "archived",
        true,
        "search",
        "",
        "states",
        publicationStates,
        "pinned",
        [],
        "priority",
        [],
        "page-size",
        sidebarGroupPageSize,
        "initial-limit",
        sidebarGroupPageSize,
      ],
      {
        pages: [archivedSidebarGroup],
        pageParams: [0],
      },
    );
    await queryClient.refetchQueries({
      queryKey: ["agents", "sidebar-conversations", "inbox"],
      type: "active",
    });
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
      delete window.__mockGitAuthDiagnostics;
      delete window.__mockGhAuthStatus;
    });
    await dismissProviderCliUpdateToasts(page);
  });

  test("starter composer mode and action menus are separated", async ({ page }) => {
    await setupAgentsView(page);
    await expect(page.getByTestId("agents-start-composer")).toBeVisible();

    // Workflow modes live on the Mode chip popover only.
    await page.getByTestId("agents-start-mode-chip").click();
    await expect(page.getByTestId("agents-start-mode-edit")).toBeVisible();
    await expect(page.getByTestId("agents-start-mode-chat")).toBeVisible();
    await expect(page.getByTestId("agents-start-mode-plan")).toBeVisible();
    await expect(page.getByTestId("agents-start-mode-automation")).toHaveCount(0);
    await expect(page.getByTestId("agents-start-mode-ideation")).toHaveCount(0);
    await expect(page.getByText("Draft and refine a plan before execution.")).toBeVisible();
    await expect(page.getByText("Build, change, and review code in a branch.")).toBeVisible();
    await page.getByRole("button", { name: "Show more modes" }).click();
    await expect(page.getByTestId("agents-start-mode-automation")).toBeVisible();
    await expect(page.getByTestId("agents-start-mode-ideation")).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("agents-start-mode-edit")).toHaveCount(0);

    // The "+" action menu keeps everything except mode switching.
    await page.getByTestId("agent-composer-actions-menu").click();
    await expect(page.getByTestId("agents-start-mode-edit")).toHaveCount(0);

    await expect(page).toHaveScreenshot("agents-start-composer-actions.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("starter runtime selector exposes the unified runtime and capabilities menu", async ({
    page,
  }) => {
    await setupAgentsView(page);
    await enableStarterCapabilityFixture(page);
    await expect(page.getByTestId("agents-start-composer")).toBeVisible();

    await page.getByTestId("agent-composer-runtime-pill").click();
    await expect(page.getByTestId("agent-composer-runtime-menu")).toBeVisible();
    await expect(page.getByRole("slider", { name: "Effort" })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Provider,/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Model,/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Effort,/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Capabilities,/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Speed,/ })).toBeVisible();
    await expect(page).toHaveScreenshot("agents-runtime-selector-unified.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });

    await page.getByRole("button", { name: /^Provider,/ }).hover();
    const modelMenuRow = page.getByRole("button", { name: /^Model,/ });
    await modelMenuRow.hover();
    await expect(modelMenuRow).toHaveAttribute("aria-expanded", "true");
    await expect(page.getByTestId("agent-composer-runtime-model-submenu")).toBeVisible();
    await expect(page.getByTestId("agent-composer-runtime-menu")).toBeVisible();
    await expect(page).toHaveScreenshot("agents-runtime-selector-models-cascade.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });

    await page.getByRole("button", { name: /^Effort,/ }).hover();
    await expect(page.getByTestId("agent-composer-runtime-effort-submenu")).toBeVisible();
    await expect(page.getByTestId("agent-composer-runtime-menu")).toBeVisible();
    await expect(page).toHaveScreenshot("agents-runtime-selector-effort-cascade.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });

    await page.getByRole("button", { name: /^Capabilities,/ }).hover();
    await expect(
      page.getByTestId("agent-composer-runtime-capability-submenu"),
    ).toBeVisible();
    await expect(page).toHaveScreenshot(
      "agents-runtime-selector-capabilities-cascade.png",
      {
        fullPage: false,
        maxDiffPixelRatio: 0.01,
      },
    );

    await page.getByRole("button", { name: /^Speed,/ }).hover();
    await expect(page.getByTestId("agent-composer-runtime-speed-submenu")).toBeVisible();
    await expect(page).toHaveScreenshot("agents-runtime-selector-speed-cascade.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("edit workspace with commit publish pane matches visual contract", async ({ page }) => {
    await setupAgentsView(page);
    await seedAgentsScenario(page);
    await selectAgentConversation(page, editConversationId);
    const publishPage = new AgentsPublishPage(page);

    await expect(page.getByTestId(`agents-session-${editConversationId}`)).toBeVisible();
    await expect(page.getByTestId("integrated-chat-messages")).toBeVisible();
    await expect(page.getByTestId("agents-publish-workspace")).toBeVisible();
    await seedPublishHistory(page, editConversationId);
    await publishPage.openFromHeader();
    await expect(page.getByTestId("agents-review-changes")).toBeEnabled();

    await hydratePublishHistoryCache(page, editConversationId);
    // The publish event log now lives in the lazy-mounted History tab, so the
    // tab has to be activated before its content exists in the DOM.
    await publishPage.selectHistory();
    await expect(
      publishPage.historyContent.getByTestId("agents-publish-events"),
    ).toBeVisible();
    // The timeline renders directly inside the History tab (no expand toggle).
    await expect(
      publishPage.historyContent.getByTestId("agents-publish-event-published"),
    ).toBeVisible();
    await stabilizePublishHistoryTimestamps(page, editConversationId);
    await expect(
      publishPage.historyContent.getByText("Published · May 13, 5:20 AM"),
    ).toBeVisible();
    await expect(page.getByTestId("agents-workspace-toolbar")).toBeVisible();
    await expect(page.getByTestId("pr-status-strip")).toBeVisible();
    await publishPage.expectCompactPrStatus("1 passed");
    await publishPage.expectCompactPrStatus("1 pending");
    await publishPage.expectPrimaryActionContained("agents-publish-confirm");

    await expect(page).toHaveScreenshot("agents-edit-publish-pane.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });

    const standardViewport = page.viewportSize() ?? { width: 1280, height: 720 };
    await page.setViewportSize({ width: 960, height: standardViewport.height });
    await publishPage.expectNoPaneOverflow();
    await expect(page).toHaveScreenshot(
      "agents-edit-publish-pane-constrained.png",
      {
        fullPage: false,
        maxDiffPixelRatio: 0.01,
      },
    );
    await page.setViewportSize(standardViewport);

    await page.getByTestId("agents-artifact-tab-pr").click();
    const prContent = page.getByTestId("agents-artifact-content-pr");
    await expect(prContent).toBeVisible();
    await expect(
      prContent.getByRole("heading", {
        name: "Persistent Agents workspace toolbar",
      }),
    ).toBeVisible();
    await expect(
      prContent.getByText("Could not load pull request details."),
    ).toHaveCount(0);
    await expect(prContent.getByTestId("pr-status-strip")).toHaveCount(0);
    await expect(page.getByTestId("pr-status-strip")).toBeVisible();
    await expect(page.getByTestId("agents-workspace-toolbar")).toBeVisible();
    const prQueryState = await page.evaluate(async (seededProjectId) => {
      const queryClient = window.__queryClient;
      if (!queryClient) {
        throw new Error("Expected query client to be available");
      }
      const { prKeys } = await import("/src/hooks/usePullRequestDetail");
      const state = queryClient.getQueryState(
        prKeys.detail({ projectId: seededProjectId, prNumber: 42 }),
      );
      return {
        fetchStatus: state?.fetchStatus,
        isInvalidated: state?.isInvalidated,
        status: state?.status,
      };
    }, projectId);
    expect(prQueryState).toEqual({
      fetchStatus: "idle",
      isInvalidated: false,
      status: "success",
    });
    await page.waitForTimeout(1_000);
    await expect(
      prContent.getByText("Could not load pull request details."),
    ).toHaveCount(0);
    await expect(page.getByText("PR health unavailable")).toHaveCount(0);
    await expect(page).toHaveScreenshot("agents-pr-tab-workspace-toolbar.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("dirty Changes and Workspace Review states match the visual contract", async ({
    page,
  }) => {
    await setupAgentsView(page);
    await seedAgentsScenario(page);
    await selectAgentConversation(page, editConversationId);
    const publishPage = new AgentsPublishPage(page);

    await publishPage.installPagedDiffRoute();
    await publishPage.openFromHeader();
    await publishPage.selectChanges();
    await publishPage.expectDiffRowsLoaded();
    await expect(page.getByTestId("agents-review-changes")).toBeEnabled();
    await publishPage.expectPrimaryActionContained("agents-publish-confirm");
    await expectPublishVisualAtWidths(
      page,
      publishPage,
      "agents-edit-publish-dirty-changes",
    );

    for (const reviewState of [
      "running",
      "blocking",
      "passed",
    ] as const) {
      await publishPage.seedWorkspaceReviewState(
        editConversationId,
        reviewState,
      );
      await publishPage.selectReview();
      await expect(
        publishPage.reviewContent.getByText(
          workspaceReviewVisualLabels[reviewState],
          { exact: true },
        ),
      ).toBeVisible();
      await expect(
        publishPage.reviewContent.getByTestId("agents-review-auto-review-fix"),
      ).toBeVisible();
      await publishPage.expectPrimaryActionContained(
        reviewState === "running"
          ? "agents-publish-reviewing"
          : reviewState === "blocking"
            ? "agents-publish-review-required"
            : "agents-publish-confirm",
      );
      if (reviewState !== "running") {
        await expect(
          publishPage.reviewContent.getByRole("heading", {
            name: reviewState === "blocking"
              ? "Blocking findings"
              : "Workspace Review",
          }),
        ).toBeVisible();
      }
      if (reviewState === "passed") {
        await expect(
          publishPage.reviewContent.getByRole("button", { name: "Run again" }),
        ).toBeVisible();
        await expect(
          publishPage.reviewContent.getByTestId("agents-review-open-publish"),
        ).toHaveCount(0);
      }
      await expectPublishVisualAtWidths(
        page,
        publishPage,
        `agents-edit-publish-review-${reviewState}`,
      );
    }
  });

  test("commit publish pane retains its direct git auth repair actions", async ({ page }) => {
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
    await expect(page.getByTestId("git-auth-startup-toast")).toHaveCount(0);

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
    await expect(page.getByTestId("agents-artifact-tab-verification")).toHaveCount(0);
    await expect(page.getByTestId("agents-artifact-tab-proposal")).toHaveCount(0);
    await expect(page.getByTestId("plan-overview-tab")).toBeVisible();
    await expect(page.getByTestId("plan-blueprint-tab")).toBeVisible();
    await expect(page.getByTestId("plan-proposals-tab")).toBeVisible();
    await expect(page.getByTestId("agents-artifact-tab-tasks")).toHaveCount(0);
    await expect(page.getByTestId("agents-artifact-tab-publish")).toHaveCount(0);
    await expect(
      page
        .getByTestId("agents-artifact-content-plan")
        .getByText("Tighten the Agents workspace and keep artifact routing clear."),
    ).toBeVisible();

    await expect(page).toHaveScreenshot("agents-ideation-artifacts.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("Agents task detail keeps key stages one-column across split widths", async ({ page }) => {
    await setupAgentsView(page);
    await seedAgentsScenario(page);
    await selectAgentConversation(page, ideationConversationId);

    await page
      .getByTestId("integrated-chat-header")
      .getByRole("button", { name: "Open artifacts" })
      .click();
    await expect(page.getByTestId("agents-artifact-pane")).toBeVisible();
    await hydrateIdeationArtifactCache(page, ideationConversationId);
    await seedAgentsTaskDetailVisualState(page);
    await page.getByTestId("agents-artifact-tab-tasks").click();
    await expect(page.getByTestId("agents-artifact-content-tasks")).toBeVisible();
    await page
      .getByTestId("agents-artifact-pane")
      .getByRole("button", { name: "Kanban" })
      .click();

    for (const viewport of [
      { width: 1120, height: 860 },
      { width: 1580, height: 920 },
    ]) {
      await page.setViewportSize(viewport);

      for (const visualState of taskDetailVisualStates) {
        const taskCard = page.getByTestId(`task-card-${visualState.id}`);
        await taskCard.scrollIntoViewIfNeeded();
        await taskCard.evaluate((element) => {
          element.dispatchEvent(
            new MouseEvent("click", {
              bubbles: true,
              cancelable: true,
              view: window,
            }),
          );
        });
        await expect(page.getByTestId("task-detail-overlay")).toBeVisible();
        await expectAgentsTaskDetailOneColumn(page, visualState.detailTestId);
        await page.getByTestId("task-overlay-back").click();
        await expect(page.getByTestId("task-detail-overlay")).toHaveCount(0);
      }
    }

    await page
      .getByTestId(`task-card-${taskDetailVisualTaskIds.merged}`)
      .evaluate((element) => {
        element.dispatchEvent(
          new MouseEvent("click", {
            bubbles: true,
            cancelable: true,
            view: window,
          }),
        );
      });
    await expectAgentsTaskDetailOneColumn(page, "merged-task-detail");
    await page.getByTestId("task-history-dropdown-trigger").click();
    await page
      .getByTestId("task-history-dropdown-item-reviewing-2026-07-07T10:30:00.000Z")
      .click();
    await expect(page.getByTestId("history-mode-banner")).toContainText(
      "Review attempt 2",
    );
    await expect(page.getByTestId("history-mode-banner")).toContainText(
      "Main chat is showing this runtime transcript",
    );
    await expectAgentsTaskDetailOneColumn(page, "reviewing-task-detail");
  });

  test("automation runs render as Runtime tray items", async ({ page }) => {
    await setupAgentsView(page);
    await seedAgentsScenario(page, { includeAutomation: true });
    await seedAutomationRuntimeVisualState(page, {
      automationId: "automation-visual-1",
      conversationId: automationConversationId,
      projectId,
    });
    await selectAgentConversation(page, automationConversationId);

    const runtime = new AgentsRuntimePage(page);
    await runtime.openRuntimeRuns();
    await expect(runtime.standaloneRunsWidget).toHaveCount(0);
    await expect(runtime.mainGroup).toBeVisible();
    await expect(runtime.runRow("automation-visual-1-run-2")).toContainText(
      "Awaiting plan approval",
    );
    await expect(runtime.runRow("automation-visual-1-run-1")).toContainText("Merged");

    await expect(page).toHaveScreenshot("agents-automation-runtime-runs.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });

  test("v27 sidebar tree and static recent block match visual contract", async ({ page }) => {
    await setupAgentsView(page);
    await seedAgentsScenario(page);

    await expect(page.getByTestId("agents-filters-trigger")).toBeVisible();
    await expect(page.getByTestId("agents-sort-trigger")).toBeVisible();
    await page.getByTestId("agents-filters-trigger").click();
    await expect(page.getByTestId("agents-filter-archived")).toBeVisible();
    await page.getByTestId("agents-filter-projects-section-trigger").click();
    await expect(page.getByTestId("agents-filter-all-projects")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("agents-filter-popover")).toHaveCount(0);
    // Static "Recent" block is now hidden ("Coming soon") on the polished sidebar — present in DOM but aria-hidden + display:none.
    await expect(page.getByTestId("agents-static-recent")).toHaveAttribute("aria-hidden", "true");
    await revealAgentInboxConversation(page, editConversationId);
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
    await seedPublishHistory(page, editConversationId);
    await page.getByTestId("agents-publish-workspace").click();
    await expect(page.getByTestId("agents-publish-pane")).toBeVisible();

    const toolbar = page.getByTestId("agents-workspace-toolbar");
    await expect(toolbar).toBeVisible();
    const statusStrip = page.getByTestId("pr-status-strip");
    await expect(statusStrip).toBeVisible();
    await expect(statusStrip.getByLabel("Approved")).toBeVisible();
    await expect(statusStrip).not.toContainText("Approved");
    await expect(toolbar.getByLabel("Workspace sync: Pushed")).toBeVisible();
    await expect(toolbar).not.toContainText("Pushed");
    await expect(toolbar.getByLabel(/merges into/)).toBeVisible();
    await expect(page.getByTestId("agents-workspace-mode-status")).toBeVisible();
    const hasHorizontalOverflow = await toolbar.evaluate(
      (element) => element.scrollWidth > element.clientWidth,
    );
    expect(hasHorizontalOverflow).toBe(false);

    const submitButton = page.getByTestId("agents-conversation-submit");
    await expect(submitButton).toBeVisible();
    await expect(submitButton.locator(".agent-composer-action-label")).toBeHidden();

    await expect(page).toHaveScreenshot("agents-compact-split-composer.png", {
      fullPage: false,
      maxDiffPixelRatio: 0.01,
    });
  });
});
