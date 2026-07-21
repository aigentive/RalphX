import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentTaskListSummary, AgentTaskSummary } from "@/api/agent-tasks";
import type {
  AgentConversationRuntimeIndexRow,
  AgentConversationWorkspace,
} from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";

import type { AgentsChatFocus } from "./agentChatFocus";
import { AgentsComposerWorkspaceChangesCard } from "./AgentsComposerWorkspaceChangesCard";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { agentConversationRuntimeIndexKeys } from "./useAgentConversationRuntimeIndex";
import { conversationWorkspaceFixture } from "./agentsTestFixtures";

const apiMocks = vi.hoisted(() => ({
  getRuntimeIndexMock: vi.fn(),
  listAgentTasksMock: vi.fn(),
  listAgentTaskListsMock: vi.fn(),
  listAgentTasksForListMock: vi.fn(),
}));

vi.mock("@/api/agent-tasks", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/agent-tasks")>();
  return {
    ...actual,
    agentTaskApi: {
      ...actual.agentTaskApi,
      listAgentTasks: (...args: unknown[]) =>
        apiMocks.listAgentTasksMock(...args),
      listAgentTaskLists: (...args: unknown[]) =>
        apiMocks.listAgentTaskListsMock(...args),
      listAgentTasksForList: (...args: unknown[]) =>
        apiMocks.listAgentTasksForListMock(...args),
    },
  };
});

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      getAgentConversationRuntimeIndex: (...args: unknown[]) =>
        apiMocks.getRuntimeIndexMock(...args),
    },
  };
});

interface RenderCardOptions {
  conversationId?: string;
  withChanges?: boolean;
  runtimeRows?: AgentConversationRuntimeIndexRow[];
  currentFocus?: AgentsChatFocus;
  taskLedgerContext?: { contextType: string; contextId: string } | null;
  isAgentGenerating?: boolean;
  workspaceOverrides?: Partial<AgentConversationWorkspace>;
}

function renderCard({
  conversationId = "conversation-1",
  withChanges = false,
  runtimeRows = [],
  currentFocus = { type: "workspace" },
  taskLedgerContext = null,
  isAgentGenerating = false,
  workspaceOverrides = {},
}: RenderCardOptions = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  queryClient.setQueryData(agentConversationRuntimeIndexKeys.detail(conversationId), {
    conversationId,
    rows: runtimeRows,
  });
  if (withChanges) {
    queryClient.setQueryData(agentWorkspaceKeys.changeSummary(conversationId), {
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 1, additions: 2, deletions: 1 },
    });
  }

  const viewCallbacks = {
    onViewWorkspace: vi.fn(),
    onViewIdeation: vi.fn(),
    onViewWorkspaceReview: vi.fn(),
    onViewVerification: vi.fn(),
    onViewTaskRuntime: vi.fn(),
    onOpenFile: vi.fn(),
    onPreloadPublishPane: vi.fn(),
  };

  const renderElement = (options: RenderCardOptions = {}) => {
    const nextConversationId = options.conversationId ?? conversationId;
    return (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <AgentsComposerWorkspaceChangesCard
            conversationId={nextConversationId}
            projectId="project-1"
            workspace={
              nextConversationId
                ? conversationWorkspaceFixture({
                    conversationId: nextConversationId,
                    ...workspaceOverrides,
                    ...options.workspaceOverrides,
                  })
                : null
            }
            isFocusedChildChat={false}
            currentFocus={options.currentFocus ?? currentFocus}
            taskLedgerContext={
              options.taskLedgerContext !== undefined
                ? options.taskLedgerContext
                : taskLedgerContext
            }
            isAgentGenerating={
              options.isAgentGenerating !== undefined
                ? options.isAgentGenerating
                : isAgentGenerating
            }
            onViewWorkspace={viewCallbacks.onViewWorkspace}
            onViewIdeation={viewCallbacks.onViewIdeation}
            onViewWorkspaceReview={viewCallbacks.onViewWorkspaceReview}
            onViewVerification={viewCallbacks.onViewVerification}
            onViewTaskRuntime={viewCallbacks.onViewTaskRuntime}
            onOpenFile={viewCallbacks.onOpenFile}
            onPreloadPublishPane={viewCallbacks.onPreloadPublishPane}
          />
        </TooltipProvider>
      </QueryClientProvider>
    );
  };

  const result = render(renderElement());

  return {
    ...result,
    queryClient,
    viewCallbacks,
    rerenderCard: (options: RenderCardOptions = {}) =>
      result.rerender(renderElement(options)),
  };
}

function runtimeRow(
  overrides: Partial<AgentConversationRuntimeIndexRow> = {},
): AgentConversationRuntimeIndexRow {
  return {
    id: "workspace:conversation-1",
    group: "main",
    kind: "workspace",
    lifecycle: "running",
    statusLabel: "Running",
    title: "Workspace chat",
    mode: "agent",
    orderIndex: 0,
    orderStartedAt: "2026-07-06T00:00:00.000Z",
    completedAt: null,
    conversationId: "conversation-1",
    contextType: "project",
    contextId: "conversation-1",
    taskId: null,
    agentRunId: "run-1",
    parentSessionId: null,
    childSessionId: null,
    providerHarness: "codex",
    providerSessionId: "session-1",
    errorMessage: null,
    ...overrides,
  };
}

function agentTask(overrides: Partial<AgentTaskSummary> = {}): AgentTaskSummary {
  return {
    taskId: "task-1",
    taskNumber: 1,
    title: "Wire runtime tab",
    state: "active",
    ownerAgent: "ralphx-general-worker",
    blockedBy: [],
    blocks: [],
    availability: "ready",
    updatedAt: "2026-07-06T00:00:00.000Z",
    ...overrides,
  };
}

function agentTaskList(
  overrides: Partial<AgentTaskListSummary> = {},
): AgentTaskListSummary {
  return {
    listId: "list-1",
    listSequence: 1,
    taskCount: 1,
    openCount: 0,
    activeCount: 1,
    doneCount: 0,
    droppedCount: 0,
    createdAt: "2026-07-06T00:00:00.000Z",
    updatedAt: "2026-07-06T00:00:00.000Z",
    ...overrides,
  };
}

describe("AgentsComposerWorkspaceChangesCard", () => {
  beforeEach(() => {
    apiMocks.getRuntimeIndexMock.mockReset();
    apiMocks.getRuntimeIndexMock.mockImplementation((conversationId: string) =>
      Promise.resolve({ conversationId, rows: [] }),
    );
    apiMocks.listAgentTasksMock.mockReset();
    apiMocks.listAgentTasksMock.mockResolvedValue([]);
    apiMocks.listAgentTaskListsMock.mockReset();
    apiMocks.listAgentTaskListsMock.mockResolvedValue([]);
    apiMocks.listAgentTasksForListMock.mockReset();
    apiMocks.listAgentTasksForListMock.mockResolvedValue([]);
  });

  it("renders nothing without any composer context", () => {
    renderCard({ conversationId: "" });

    expect(
      screen.queryByTestId("agents-composer-context-tray"),
    ).not.toBeInTheDocument();
  });

  it("closes the active changes panel when the change summary becomes empty", async () => {
    const { queryClient } = renderCard({ withChanges: true });

    const changesToggle = await screen.findByTestId(
      "diff-filter-trigger",
      undefined,
      { timeout: 3_000 },
    );
    fireEvent.click(changesToggle);
    expect(screen.getByTestId("agents-composer-context-tray-body")).toBeInTheDocument();

    act(() => {
      queryClient.setQueryData(agentWorkspaceKeys.changeSummary("conversation-1"), {
        supportsWorktreeModes: true,
        staged: { fileCount: 0, additions: 0, deletions: 0 },
        unstaged: { fileCount: 0, additions: 0, deletions: 0 },
      });
    });

    await waitFor(() =>
      expect(
        screen.queryByTestId("agents-composer-context-tray-body"),
      ).not.toBeInTheDocument(),
    );
  });

  it.each(["merged", "closed"] as const)(
    "suppresses live workspace changes for a cold %s terminal workspace",
    async (publicationPrStatus) => {
      renderCard({
        withChanges: true,
        workspaceOverrides: {
          publicationPrNumber: 42,
          publicationPrStatus,
          publicationPushStatus: "pushed",
        },
      });

      expect(
        await screen.findByTestId("agents-composer-runtimes-toggle"),
      ).toBeInTheDocument();
      expect(
        screen.queryByTestId("diff-filter-trigger"),
      ).not.toBeInTheDocument();
      expect(screen.queryByText("Workspace changes")).not.toBeInTheDocument();
    },
  );

  it("hides an open changes panel synchronously when the workspace becomes terminal", async () => {
    const { rerenderCard } = renderCard({ withChanges: true });
    fireEvent.click(await screen.findByTestId("diff-filter-trigger"));
    expect(
      screen.getByTestId("agents-composer-workspace-changes-list"),
    ).toBeInTheDocument();

    rerenderCard({
      workspaceOverrides: {
        publicationPrNumber: 42,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      },
    });

    expect(screen.queryByTestId("diff-filter-trigger")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-composer-workspace-changes-list"),
    ).not.toBeInTheDocument();
  });

  it("opens an empty runtime index without rendering runtime groups", async () => {
    renderCard();

    fireEvent.click(await screen.findByTestId("agents-composer-runtimes-toggle"));

    await waitFor(() =>
      expect(screen.queryByText("Loading runtimes...")).not.toBeInTheDocument(),
    );
    expect(screen.getByTestId("agents-composer-runtimes-list")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-composer-runtimes-group-main"),
    ).not.toBeInTheDocument();
  });

  it("renders runtime fallback labels and non-clickable runtime rows", async () => {
    renderCard({
      runtimeRows: [
        runtimeRow({
          id: "delegation:1",
          kind: "delegation",
          lifecycle: "planned",
          statusLabel: "Planned",
          title: "Delegated reviewer",
          mode: null,
          contextType: null,
          contextId: null,
          providerHarness: null,
        }),
        runtimeRow({
          id: "workspace-review:1",
          kind: "workspace_review",
          lifecycle: "blocked",
          statusLabel: "Blocked",
          title: "Review plan",
          mode: "plan",
          conversationId: null,
          contextId: null,
        }),
      ],
    });

    fireEvent.click(await screen.findByTestId("agents-composer-runtimes-toggle"));

    expect(await screen.findByText("Delegated reviewer")).toBeInTheDocument();
    expect(screen.getByText("delegation")).toBeInTheDocument();
    expect(screen.getByText("Planned")).toBeInTheDocument();
    expect(screen.getByText("Plan mode")).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-composer-runtime-row-delegation").tagName,
    ).toBe("DIV");
  });

  it("marks ideation and verification runtime rows as current", async () => {
    const ideation = renderCard({
      currentFocus: { type: "ideation", sessionId: "ideation-1" },
      runtimeRows: [
        runtimeRow({
          id: "ideation:ideation-1",
          group: "ideation_verification",
          kind: "ideation",
          title: "Plan ideation",
          mode: "ideation",
          contextId: "ideation-1",
        }),
      ],
    });

    fireEvent.click(await screen.findByTestId("agents-composer-runtimes-toggle"));
    expect(await screen.findByText("Plan ideation")).toBeInTheDocument();
    expect(screen.getByText("Viewing")).toBeInTheDocument();

    ideation.unmount();

    renderCard({
      currentFocus: {
        type: "verification",
        parentSessionId: "parent-session-1",
        childSessionId: "child-session-1",
      },
      runtimeRows: [
        runtimeRow({
          id: "verification:child-session-1",
          group: "ideation_verification",
          kind: "verification",
          title: "Plan verification",
          mode: null,
          contextId: "child-session-1",
          parentSessionId: "parent-session-1",
          childSessionId: "child-session-1",
        }),
      ],
    });

    fireEvent.click(await screen.findByTestId("agents-composer-runtimes-toggle"));
    expect(await screen.findByText("Plan verification")).toBeInTheDocument();
    expect(screen.getByText("Viewing")).toBeInTheDocument();
  });

  it("refetches runtime and task rows when generation settles", async () => {
    apiMocks.listAgentTasksMock.mockResolvedValue([agentTask()]);
    apiMocks.listAgentTaskListsMock.mockResolvedValue([agentTaskList()]);

    const { rerenderCard } = renderCard({
      isAgentGenerating: true,
      taskLedgerContext: {
        contextType: "conversation",
        contextId: "conversation-1",
      },
    });

    const tasksToggle = await screen.findByTestId("agents-composer-tasks-toggle");
    fireEvent.click(tasksToggle);
    await waitFor(() =>
      expect(apiMocks.listAgentTaskListsMock).toHaveBeenCalledTimes(1),
    );

    apiMocks.getRuntimeIndexMock.mockClear();
    apiMocks.listAgentTasksMock.mockClear();
    apiMocks.listAgentTaskListsMock.mockClear();

    rerenderCard({
      isAgentGenerating: false,
      taskLedgerContext: {
        contextType: "conversation",
        contextId: "conversation-1",
      },
    });

    await waitFor(() =>
      expect(apiMocks.getRuntimeIndexMock).toHaveBeenCalledWith("conversation-1"),
    );
    await waitFor(() =>
      expect(apiMocks.listAgentTasksMock).toHaveBeenCalledWith({
        contextType: "conversation",
        contextId: "conversation-1",
        projectId: "project-1",
        includeDone: true,
      }),
    );
  });
});
