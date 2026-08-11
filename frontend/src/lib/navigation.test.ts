import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAgentArtifactUiStore } from "@/components/agents/agentArtifactUiStore";
import {
  navigateToAgentConversation,
  navigateToAgentPlan,
  navigateToAgentTask,
  navigateToIdeationSession,
  openIdeationInAgents,
  openTaskInAgents,
} from "./navigation";

const {
  mockUiGetState,
  mockUiSetState,
  mockProjectGetState,
  mockGetQueriesData,
  resolveIdeationWorkspaceMock,
  resolveTaskWorkspaceMock,
  selectConversationMock,
  setArtifactTabMock,
  setFocusedProjectMock,
  setTaskArtifactModeMock,
  focusTaskArtifactMock,
  setActiveConversationMock,
  toastInfoMock,
} = vi.hoisted(() => ({
  mockUiGetState: vi.fn(),
  mockUiSetState: vi.fn(),
  mockProjectGetState: vi.fn(),
  mockGetQueriesData: vi.fn(),
  resolveIdeationWorkspaceMock: vi.fn(),
  resolveTaskWorkspaceMock: vi.fn(),
  selectConversationMock: vi.fn(),
  setArtifactTabMock: vi.fn(),
  setFocusedProjectMock: vi.fn(),
  setTaskArtifactModeMock: vi.fn(),
  focusTaskArtifactMock: vi.fn(),
  setActiveConversationMock: vi.fn(),
  toastInfoMock: vi.fn(),
}));

vi.mock("sonner", () => ({ toast: { info: toastInfoMock } }));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: { getState: mockUiGetState, setState: mockUiSetState },
}));
vi.mock("@/stores/projectStore", () => ({
  useProjectStore: { getState: mockProjectGetState },
}));
vi.mock("@/stores/agentSessionStore", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/stores/agentSessionStore")>()),
  useAgentSessionStore: {
    getState: () => ({
      artifactByConversationId: {},
      selectConversation: selectConversationMock,
      setArtifactTab: setArtifactTabMock,
      setFocusedProject: setFocusedProjectMock,
      setTaskArtifactMode: setTaskArtifactModeMock,
      focusTaskArtifact: focusTaskArtifactMock,
    }),
  },
}));
vi.mock("@/stores/chatStore", () => ({
  useChatStore: { getState: () => ({ setActiveConversation: setActiveConversationMock }) },
}));
vi.mock("@/api/ideation", () => ({
  ideationApi: { sessions: { resolveAgentWorkspace: resolveIdeationWorkspaceMock } },
}));
vi.mock("@/api/tasks", () => ({
  tasksApi: { resolveAgentWorkspace: resolveTaskWorkspaceMock },
}));
vi.mock("@/lib/queryClient", () => ({
  getQueryClient: () => ({ getQueriesData: mockGetQueriesData }),
}));
const PROJECT_A = "project-a";
const PROJECT_B = "project-b";
const SESSION_A = "session-a";
const CONVERSATION_A = "conversation-a";

const setCurrentViewMock = vi.fn();
const selectProjectMock = vi.fn();

function cacheLinkedConversation(
  sessionId: string,
  conversationId: string,
  projectId: string,
) {
  mockGetQueriesData.mockReturnValue([
    [["agents", "sidebar-conversations"], {
      pages: [{ rows: [{ workspace: {
        conversationId,
        projectId,
        linkedIdeationSessionId: sessionId,
      } }] }],
      pageParams: [0],
    }],
  ]);
}

beforeEach(() => {
  vi.clearAllMocks();
  useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
  mockProjectGetState.mockReturnValue({
    activeProjectId: PROJECT_A,
    selectProject: selectProjectMock,
  });
  mockUiGetState.mockReturnValue({ viewByProject: {}, setCurrentView: setCurrentViewMock });
  mockGetQueriesData.mockReturnValue([]);
  resolveIdeationWorkspaceMock.mockResolvedValue(null);
  resolveTaskWorkspaceMock.mockResolvedValue(null);
});

describe("navigateToIdeationSession", () => {
  it("opens the backend-validated Agent conversation's Plan artifact", async () => {
    resolveIdeationWorkspaceMock.mockResolvedValue({
      projectId: PROJECT_A,
      conversationId: CONVERSATION_A,
    });

    navigateToIdeationSession(SESSION_A);
    await vi.waitFor(() => expect(resolveIdeationWorkspaceMock).toHaveBeenCalledWith(SESSION_A));
    await vi.waitFor(() => expect(selectConversationMock).toHaveBeenCalledWith(PROJECT_A, CONVERSATION_A));

    expect(selectConversationMock).toHaveBeenCalledWith(PROJECT_A, CONVERSATION_A);
    expect(setArtifactTabMock).toHaveBeenCalledWith(CONVERSATION_A, "plan");
    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
  });

  it("fails closed in Agents when its conversation is not backend-linked", async () => {
    await expect(openIdeationInAgents(SESSION_A)).resolves.toBe(false);

    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
    expect(toastInfoMock).toHaveBeenCalledWith(
      "This ideation session is no longer linked to an Agent workspace.",
    );
    expect(selectConversationMock).not.toHaveBeenCalled();
    expect(setActiveConversationMock).not.toHaveBeenCalled();
    expect(setFocusedProjectMock).not.toHaveBeenCalled();
    expect(mockUiSetState).not.toHaveBeenCalled();
    expect(selectProjectMock).not.toHaveBeenCalled();
  });

  it("uses backend ownership when a cached Agent target is stale", async () => {
    cacheLinkedConversation(SESSION_A, "conversation-stale", PROJECT_A);
    resolveIdeationWorkspaceMock.mockResolvedValue({
      projectId: PROJECT_B,
      conversationId: "conversation-authoritative",
    });

    await expect(openIdeationInAgents(SESSION_A)).resolves.toBe(true);

    expect(resolveIdeationWorkspaceMock).toHaveBeenCalledWith(SESSION_A);
    expect(selectConversationMock).toHaveBeenCalledWith(
      PROJECT_B,
      "conversation-authoritative",
    );
    expect(selectConversationMock).not.toHaveBeenCalledWith(
      PROJECT_A,
      CONVERSATION_A,
    );
    expect(selectConversationMock).not.toHaveBeenCalledWith(
      PROJECT_A,
      "conversation-stale",
    );
  });

  it("does not use an invalid Agent target after backend resolution fails", async () => {
    cacheLinkedConversation(SESSION_A, "conversation-invalid", PROJECT_A);
    await expect(openIdeationInAgents(SESSION_A)).resolves.toBe(false);

    expect(resolveIdeationWorkspaceMock).toHaveBeenCalledWith(SESSION_A);
    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
    expect(selectConversationMock).not.toHaveBeenCalled();
    expect(setActiveConversationMock).not.toHaveBeenCalled();
  });
});

describe("navigateToAgentConversation", () => {
  it("selects the exact conversation and shows Agents", () => {
    navigateToAgentConversation(PROJECT_A, CONVERSATION_A);

    expect(selectConversationMock).toHaveBeenCalledWith(PROJECT_A, CONVERSATION_A);
    expect(setActiveConversationMock).toHaveBeenCalledWith(
      `project:${PROJECT_A}`,
      CONVERSATION_A,
    );
    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
  });

  it("switches projects before showing a cross-project conversation", () => {
    navigateToAgentConversation(PROJECT_B, CONVERSATION_A);

    expect(selectConversationMock).toHaveBeenCalledWith(PROJECT_B, CONVERSATION_A);
    expect(setActiveConversationMock).toHaveBeenCalledWith(
      `project:${PROJECT_B}`,
      CONVERSATION_A,
    );
    expect(mockUiSetState).toHaveBeenCalledWith({
      viewByProject: { [PROJECT_B]: "agents" },
    });
    expect(selectProjectMock).toHaveBeenCalledWith(PROJECT_B);
    expect(setCurrentViewMock).not.toHaveBeenCalled();
  });

  it("opens a standalone conversation without inventing project ownership", () => {
    navigateToAgentConversation(null, CONVERSATION_A);

    expect(selectConversationMock).toHaveBeenCalledWith(null, CONVERSATION_A);
    expect(setActiveConversationMock).toHaveBeenCalledWith(
      `standalone:${CONVERSATION_A}`,
      CONVERSATION_A,
    );
    expect(selectProjectMock).not.toHaveBeenCalled();
    expect(mockUiSetState).not.toHaveBeenCalled();
    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
  });
});

describe("navigateToAgentPlan", () => {
  it("opens the exact conversation's Plan artifact", () => {
    useAgentArtifactUiStore.getState().setArtifactState(CONVERSATION_A, {
      isOpen: false,
      activeTab: "tasks",
      taskMode: "kanban",
      hiddenTabs: ["plan"],
    });

    navigateToAgentPlan(PROJECT_A, CONVERSATION_A);

    expect(
      useAgentArtifactUiStore.getState().artifactByConversationId[CONVERSATION_A],
    ).toEqual({
      isOpen: true,
      activeTab: "plan",
      taskMode: "kanban",
      hiddenTabs: [],
    });
    expect(setArtifactTabMock).toHaveBeenCalledWith(CONVERSATION_A, "plan");
    expect(selectConversationMock).toHaveBeenCalledWith(PROJECT_A, CONVERSATION_A);
    expect(setActiveConversationMock).toHaveBeenCalledWith(
      `project:${PROJECT_A}`,
      CONVERSATION_A,
    );
    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
  });
});

describe("navigateToAgentTask", () => {
  it("opens Tasks in the requested mode and selects the linked task", () => {
    navigateToAgentTask(PROJECT_A, CONVERSATION_A, "task-42", "graph");

    expect(selectConversationMock).toHaveBeenCalledWith(PROJECT_A, CONVERSATION_A);
    expect(setTaskArtifactModeMock).toHaveBeenCalledWith(CONVERSATION_A, "graph");
    expect(focusTaskArtifactMock).toHaveBeenCalledWith(CONVERSATION_A, "task-42");
    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
  });
});

describe("latest-intent Agents resolution", () => {
  it("opens a task after backend ownership resolves", async () => {
    resolveTaskWorkspaceMock.mockResolvedValue({
      projectId: PROJECT_B,
      conversationId: "conversation-task",
      title: "Task workspace",
    });

    await expect(openTaskInAgents("task-42", "kanban")).resolves.toBe(true);

    expect(setTaskArtifactModeMock).toHaveBeenCalledWith(
      "conversation-task",
      "kanban",
    );
    expect(focusTaskArtifactMock).toHaveBeenCalledWith(
      "conversation-task",
      "task-42",
    );
  });

  it("uses backend ownership when caller-supplied task hints are mismatched", async () => {
    resolveTaskWorkspaceMock.mockResolvedValue({
      projectId: PROJECT_B,
      conversationId: "conversation-authoritative-task",
      title: "Authoritative task workspace",
    });

    await expect(
      openTaskInAgents("task-42", "kanban", {
        projectId: PROJECT_A,
        conversationId: CONVERSATION_A,
      }),
    ).resolves.toBe(true);

    expect(resolveTaskWorkspaceMock).toHaveBeenCalledWith("task-42");
    expect(selectConversationMock).toHaveBeenCalledWith(
      PROJECT_B,
      "conversation-authoritative-task",
    );
    expect(selectConversationMock).not.toHaveBeenCalledWith(
      PROJECT_A,
      CONVERSATION_A,
    );
  });

  it("shows Agents guidance without replacing selection when task ownership is missing", async () => {
    await expect(
      openTaskInAgents("task-missing", "graph", {
        projectId: PROJECT_A,
        conversationId: CONVERSATION_A,
      }),
    ).resolves.toBe(false);

    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
    expect(toastInfoMock).toHaveBeenCalledWith(
      "Open the linked Agent conversation to view this task.",
    );
    expect(selectConversationMock).not.toHaveBeenCalled();
    expect(setActiveConversationMock).not.toHaveBeenCalled();
    expect(setTaskArtifactModeMock).not.toHaveBeenCalled();
    expect(focusTaskArtifactMock).not.toHaveBeenCalled();
  });

  it("uses the same fail-closed Agents behavior when task ownership lookup fails", async () => {
    resolveTaskWorkspaceMock.mockRejectedValueOnce(new Error("offline"));

    await expect(openTaskInAgents("task-error", "kanban")).resolves.toBe(false);

    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
    expect(toastInfoMock).toHaveBeenCalledWith(
      "Open the linked Agent conversation to view this task.",
    );
    expect(selectConversationMock).not.toHaveBeenCalled();
  });

  it("rejects an older slow resolution after a newer navigation intent", async () => {
    let resolveFirst: ((value: unknown) => void) | undefined;
    let resolveSecond: ((value: unknown) => void) | undefined;
    resolveTaskWorkspaceMock
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveFirst = resolve; }),
      )
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveSecond = resolve; }),
      );

    const first = openTaskInAgents("task-first", "graph");
    const second = openTaskInAgents("task-second", "graph");
    resolveSecond?.({
      projectId: PROJECT_B,
      conversationId: "conversation-second",
      title: "Second",
    });
    await expect(second).resolves.toBe(true);
    resolveFirst?.({
      projectId: PROJECT_A,
      conversationId: "conversation-first",
      title: "First",
    });
    await expect(first).resolves.toBe(false);

    expect(focusTaskArtifactMock).toHaveBeenCalledWith(
      "conversation-second",
      "task-second",
    );
    expect(focusTaskArtifactMock).not.toHaveBeenCalledWith(
      "conversation-first",
      "task-first",
    );
  });

  it("lets a newer ideation navigation supersede a slow task lookup", async () => {
    let resolveTask: ((value: {
      projectId: string;
      conversationId: string;
      title: string;
    }) => void) | undefined;
    resolveTaskWorkspaceMock.mockImplementationOnce(
      () => new Promise((resolve) => { resolveTask = resolve; }),
    );
    const taskNavigation = openTaskInAgents("task-slow", "graph");
    resolveIdeationWorkspaceMock.mockResolvedValueOnce({
      projectId: PROJECT_A,
      conversationId: "conversation-plan",
    });
    await expect(openIdeationInAgents(SESSION_A)).resolves.toBe(true);
    resolveTask?.({
      projectId: PROJECT_B,
      conversationId: "conversation-task",
      title: "Slow task",
    });
    await expect(taskNavigation).resolves.toBe(false);

    expect(setArtifactTabMock).toHaveBeenCalledWith("conversation-plan", "plan");
    expect(focusTaskArtifactMock).not.toHaveBeenCalledWith(
      "conversation-task",
      "task-slow",
    );
  });

  it("lets direct Agent navigation supersede a slow ideation lookup", async () => {
    let resolveIdeation: ((value: {
      projectId: string;
      conversationId: string;
    }) => void) | undefined;
    resolveIdeationWorkspaceMock.mockImplementationOnce(
      () => new Promise((resolve) => { resolveIdeation = resolve; }),
    );

    const ideationNavigation = openIdeationInAgents("session-slow");
    navigateToAgentConversation(PROJECT_A, "conversation-direct");
    resolveIdeation?.({
      projectId: PROJECT_B,
      conversationId: "conversation-stale-plan",
    });
    await expect(ideationNavigation).resolves.toBe(false);

    expect(selectConversationMock).toHaveBeenCalledTimes(1);
    expect(selectConversationMock).toHaveBeenCalledWith(
      PROJECT_A,
      "conversation-direct",
    );
    expect(setArtifactTabMock).not.toHaveBeenCalledWith(
      "conversation-stale-plan",
      "plan",
    );
    expect(toastInfoMock).not.toHaveBeenCalled();
  });
});
