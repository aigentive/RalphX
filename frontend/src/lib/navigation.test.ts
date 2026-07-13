import { beforeEach, describe, expect, it, vi } from "vitest";

import type { IdeationSession } from "@/types/ideation";
import {
  navigateToAgentConversation,
  navigateToAgentPlanConversation,
  navigateToAgentTask,
  navigateToIdeationSession,
} from "./navigation";

const {
  mockUiGetState,
  mockUiSetState,
  mockIdeationGetState,
  mockProjectGetState,
  mockGetQueriesData,
  selectConversationMock,
  setArtifactTabMock,
  setFocusedProjectMock,
  setTaskArtifactModeMock,
  focusTaskArtifactMock,
  setActiveConversationMock,
} = vi.hoisted(() => ({
  mockUiGetState: vi.fn(),
  mockUiSetState: vi.fn(),
  mockIdeationGetState: vi.fn(),
  mockProjectGetState: vi.fn(),
  mockGetQueriesData: vi.fn(),
  selectConversationMock: vi.fn(),
  setArtifactTabMock: vi.fn(),
  setFocusedProjectMock: vi.fn(),
  setTaskArtifactModeMock: vi.fn(),
  focusTaskArtifactMock: vi.fn(),
  setActiveConversationMock: vi.fn(),
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: { getState: mockUiGetState, setState: mockUiSetState },
}));
vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: { getState: mockIdeationGetState },
}));
vi.mock("@/stores/projectStore", () => ({
  useProjectStore: { getState: mockProjectGetState },
}));
vi.mock("@/stores/agentSessionStore", () => ({
  useAgentSessionStore: {
    getState: () => ({
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
vi.mock("@/lib/queryClient", () => ({
  getQueryClient: () => ({ getQueriesData: mockGetQueriesData }),
}));

const PROJECT_A = "project-a";
const PROJECT_B = "project-b";
const SESSION_A = "session-a";
const CONVERSATION_A = "conversation-a";

function makeSession(id: string, projectId: string): IdeationSession {
  return {
    id,
    projectId,
    title: null,
    titleSource: null,
    status: "active",
    planArtifactId: null,
    seedTaskId: null,
    parentSessionId: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    archivedAt: null,
    convertedAt: null,
    verificationStatus: "unverified",
    teamMode: null,
    teamConfig: null,
  };
}

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
  mockProjectGetState.mockReturnValue({
    activeProjectId: PROJECT_A,
    selectProject: selectProjectMock,
  });
  mockUiGetState.mockReturnValue({ viewByProject: {}, setCurrentView: setCurrentViewMock });
  mockGetQueriesData.mockReturnValue([]);
  mockIdeationGetState.mockReturnValue({
    sessions: { [SESSION_A]: makeSession(SESSION_A, PROJECT_A) },
  });
});

describe("navigateToIdeationSession", () => {
  it("opens the linked Agent conversation's Plan artifact", () => {
    cacheLinkedConversation(SESSION_A, CONVERSATION_A, PROJECT_A);

    navigateToIdeationSession(SESSION_A);

    expect(selectConversationMock).toHaveBeenCalledWith(PROJECT_A, CONVERSATION_A);
    expect(setArtifactTabMock).toHaveBeenCalledWith(CONVERSATION_A, "plan");
    expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
  });

  it("falls back to the owning project in Agents when its conversation is not cached", () => {
    mockIdeationGetState.mockReturnValue({
      sessions: { [SESSION_A]: makeSession(SESSION_A, PROJECT_B) },
    });

    navigateToIdeationSession(SESSION_A);

    expect(setFocusedProjectMock).toHaveBeenCalledWith(PROJECT_B);
    expect(mockUiSetState).toHaveBeenCalledWith({
      viewByProject: { [PROJECT_B]: "agents" },
    });
    expect(selectProjectMock).toHaveBeenCalledWith(PROJECT_B);
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
});

describe("navigateToAgentPlanConversation", () => {
  it("selects the exact conversation and opens the Plan artifact", () => {
    navigateToAgentPlanConversation(PROJECT_A, CONVERSATION_A);

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
