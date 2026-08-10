/**
 * useIdeation hooks tests
 *
 * Tests for useIdeationSession and useIdeationSessions hooks
 * using TanStack Query with mocked API.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import {
  useIdeationSession,
  useIdeationSessions,
  useCreateIdeationSession,
  useArchiveIdeationSession,
  useReopenSession,
  useResetAndReaccept,
  ideationKeys,
} from "./useIdeation";
import { ideationApi } from "@/api/ideation";
import { planBranchApi } from "@/api/plan-branch";
import { useIdeationStore } from "@/stores/ideationStore";
import { dependencyKeys } from "./useDependencyGraph";
import { taskKeys } from "./useTasks";
import type { IdeationSession, TaskProposal, ChatMessage } from "@/types/ideation";

// Mock the ideation API
vi.mock("@/api/ideation", () => ({
  ideationApi: {
    sessions: {
      get: vi.fn(),
      getWithData: vi.fn(),
      list: vi.fn(),
      create: vi.fn(),
      archive: vi.fn(),
      reopen: vi.fn(),
      delete: vi.fn(),
    },
    apply: {
      toKanban: vi.fn(),
    },
  },
}));

vi.mock("@/api/plan-branch", () => ({
  planBranchApi: {
    getByPlan: vi.fn(),
  },
}));

vi.mock("@/hooks/useAgentGate", () => ({
  useAgentGate: () => ({ gated: false, reason: null }),
}));

// Create mock data
const mockSession: IdeationSession = {
  id: "session-1",
  projectId: "project-1",
  title: "Test Session",
  status: "active",
  createdAt: "2026-01-24T10:00:00Z",
  updatedAt: "2026-01-24T10:00:00Z",
  archivedAt: null,
  convertedAt: null,
};

const mockSession2: IdeationSession = {
  id: "session-2",
  projectId: "project-1",
  title: "Second Session",
  status: "archived",
  createdAt: "2026-01-23T10:00:00Z",
  updatedAt: "2026-01-23T12:00:00Z",
  archivedAt: "2026-01-23T12:00:00Z",
  convertedAt: null,
};

const mockProposal: TaskProposal = {
  id: "proposal-1",
  sessionId: "session-1",
  title: "Test Proposal",
  description: "Test description",
  category: "feature",
  steps: ["Step 1", "Step 2"],
  acceptanceCriteria: ["AC 1"],
  suggestedPriority: "high",
  priorityScore: 75,
  priorityReason: "Blocks other tasks",
  estimatedComplexity: "moderate",
  userPriority: null,
  userModified: false,
  status: "pending",
  createdTaskId: null,
  sortOrder: 0,
  createdAt: "2026-01-24T10:00:00Z",
  updatedAt: "2026-01-24T10:00:00Z",
};

const mockMessage: ChatMessage = {
  id: "message-1",
  sessionId: "session-1",
  projectId: null,
  taskId: null,
  role: "user",
  content: "Hello",
  metadata: null,
  parentMessageId: null,
  createdAt: "2026-01-24T10:00:00Z",
};

// Test wrapper with QueryClientProvider
function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
      },
    },
  });

  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("ideationKeys", () => {
  it("should generate correct key for all", () => {
    expect(ideationKeys.all).toEqual(["ideation"]);
  });

  it("should generate correct key for sessions", () => {
    expect(ideationKeys.sessions()).toEqual(["ideation", "sessions"]);
  });

  it("should generate correct key for session list by project", () => {
    expect(ideationKeys.sessionList("project-1")).toEqual([
      "ideation",
      "sessions",
      "list",
      "project-1",
    ]);
  });

  it("should generate correct key for session details", () => {
    expect(ideationKeys.sessionDetails()).toEqual(["ideation", "sessions", "detail"]);
  });

  it("should generate correct key for session detail", () => {
    expect(ideationKeys.sessionDetail("session-1")).toEqual([
      "ideation",
      "sessions",
      "detail",
      "session-1",
    ]);
  });

  it("should generate correct key for session with data", () => {
    expect(ideationKeys.sessionWithData("session-1")).toEqual([
      "ideation",
      "sessions",
      "detail",
      "session-1",
      "with-data",
    ]);
  });
});

describe("useIdeationSession", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should fetch session with data successfully", async () => {
    const mockData = {
      session: mockSession,
      proposals: [mockProposal],
      messages: [mockMessage],
    };
    vi.mocked(ideationApi.sessions.getWithData).mockResolvedValueOnce(mockData);

    const { result } = renderHook(() => useIdeationSession("session-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(ideationApi.sessions.getWithData).toHaveBeenCalledWith("session-1");
    });
    await waitFor(() => expect(result.current.data).toEqual(mockData));

    expect(result.current.data).toEqual(mockData);
  });

  it("should return null for non-existent session", async () => {
    vi.mocked(ideationApi.sessions.getWithData).mockResolvedValueOnce(null);

    const { result } = renderHook(() => useIdeationSession("non-existent"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toBeNull();
  });

  it("should handle fetch error", async () => {
    const error = new Error("Failed to fetch session");
    vi.mocked(ideationApi.sessions.getWithData).mockRejectedValueOnce(error);

    const { result } = renderHook(() => useIdeationSession("session-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(result.current.error).toEqual(error);
  });

  it("should not fetch when sessionId is empty", async () => {
    const { result } = renderHook(() => useIdeationSession(""), {
      wrapper: createWrapper(),
    });

    // Query should be disabled
    expect(result.current.isFetching).toBe(false);
    expect(ideationApi.sessions.getWithData).not.toHaveBeenCalled();
  });
});

describe("useIdeationSessions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should fetch sessions for project successfully", async () => {
    const mockSessions = [mockSession, mockSession2];
    vi.mocked(ideationApi.sessions.list).mockResolvedValueOnce(mockSessions);

    const { result } = renderHook(() => useIdeationSessions("project-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual(mockSessions);
    expect(ideationApi.sessions.list).toHaveBeenCalledWith("project-1");
  });

  it("should return empty array for project with no sessions", async () => {
    vi.mocked(ideationApi.sessions.list).mockResolvedValueOnce([]);

    const { result } = renderHook(() => useIdeationSessions("project-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual([]);
  });

  it("should handle fetch error", async () => {
    const error = new Error("Failed to fetch sessions");
    vi.mocked(ideationApi.sessions.list).mockRejectedValueOnce(error);

    const { result } = renderHook(() => useIdeationSessions("project-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(result.current.error).toEqual(error);
  });

  it("should not fetch when projectId is empty", async () => {
    const { result } = renderHook(() => useIdeationSessions(""), {
      wrapper: createWrapper(),
    });

    expect(result.current.isFetching).toBe(false);
    expect(ideationApi.sessions.list).not.toHaveBeenCalled();
  });
});

describe("useCreateIdeationSession", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should create a session successfully", async () => {
    vi.mocked(ideationApi.sessions.create).mockResolvedValueOnce(mockSession);

    const { result } = renderHook(() => useCreateIdeationSession(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await result.current.mutateAsync({ projectId: "project-1", title: "Test Session" });
    });

    expect(ideationApi.sessions.create).toHaveBeenCalledWith(
      "project-1",
      "Test Session",
      undefined,
      undefined,
    );
  });

  it("should create a session without title", async () => {
    const sessionNoTitle = { ...mockSession, title: null };
    vi.mocked(ideationApi.sessions.create).mockResolvedValueOnce(sessionNoTitle);

    const { result } = renderHook(() => useCreateIdeationSession(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await result.current.mutateAsync({ projectId: "project-1" });
    });

    expect(ideationApi.sessions.create).toHaveBeenCalledWith(
      "project-1",
      undefined,
      undefined,
      undefined,
    );
  });

  it("should handle creation error", async () => {
    const error = new Error("Failed to create session");
    vi.mocked(ideationApi.sessions.create).mockRejectedValueOnce(error);

    const { result } = renderHook(() => useCreateIdeationSession(), {
      wrapper: createWrapper(),
    });

    await expect(
      act(async () => {
        await result.current.mutateAsync({ projectId: "project-1" });
      })
    ).rejects.toThrow("Failed to create session");
  });
});

describe("useArchiveIdeationSession", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should archive a session successfully", async () => {
    vi.mocked(ideationApi.sessions.archive).mockResolvedValueOnce(undefined);

    const { result } = renderHook(() => useArchiveIdeationSession(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await result.current.mutateAsync("session-1");
    });

    expect(ideationApi.sessions.archive).toHaveBeenCalledWith("session-1");
  });

  it("should handle archive error", async () => {
    const error = new Error("Failed to archive session");
    vi.mocked(ideationApi.sessions.archive).mockRejectedValueOnce(error);

    const { result } = renderHook(() => useArchiveIdeationSession(), {
      wrapper: createWrapper(),
    });

    await expect(
      act(async () => {
        await result.current.mutateAsync("session-1");
      })
    ).rejects.toThrow("Failed to archive session");
  });
});

describe("useReopenSession", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("invalidates dependency graphs after a reopen", async () => {
    vi.mocked(ideationApi.sessions.reopen).mockResolvedValueOnce(undefined);

    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
          gcTime: 0,
        },
      },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);

    const { result } = renderHook(() => useReopenSession(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync("session-1");
    });

    expect(ideationApi.sessions.reopen).toHaveBeenCalledWith("session-1");
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ideationKeys.sessionDetail("session-1"),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ideationKeys.sessions(),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: taskKeys.all,
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: dependencyKeys.graphs(),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ideationKeys.proposals(),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["plan-branch"],
    });
  });
});

describe("useResetAndReaccept", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useIdeationStore.setState({
      planArtifact: {
        id: "artifact-1",
        type: "specification",
        name: "Plan",
        content: { type: "inline", text: "# Plan" },
        metadata: {
          createdAt: "2026-08-05T10:00:00Z",
          createdBy: "system",
          version: 3,
        },
        derivedFrom: [],
      },
    });
  });

  it("fails closed before destructive work when the existing base branch read fails", async () => {
    vi.mocked(planBranchApi.getByPlan).mockRejectedValue({
      outcome: "commandError",
      error: "REMOTE_INTERNAL_ERROR: plan branch unavailable",
    });

    const { result } = renderHook(() => useResetAndReaccept(), {
      wrapper: createWrapper(),
    });

    await expect(
      result.current.mutateAsync({ sessionId: "session-1", proposalIds: ["proposal-1"] }),
    ).rejects.toEqual({
      outcome: "commandError",
      error: "REMOTE_INTERNAL_ERROR: plan branch unavailable",
    });
    expect(planBranchApi.getByPlan).toHaveBeenCalledWith("artifact-1");
    expect(ideationApi.sessions.reopen).not.toHaveBeenCalled();
    expect(ideationApi.apply.toKanban).not.toHaveBeenCalled();
  });

  it("preserves the resolved base branch through reopen and re-apply", async () => {
    vi.mocked(planBranchApi.getByPlan).mockResolvedValue({
      id: "branch-1",
      planArtifactId: "artifact-1",
      sessionId: "session-1",
      projectId: "project-1",
      branchName: "ralphx/plan-1",
      sourceBranch: "main",
      status: "active",
      mergeTaskId: null,
      createdAt: "2026-08-05T10:00:00Z",
      mergedAt: null,
      prNumber: null,
      prUrl: null,
      prDraft: null,
      prPushStatus: null,
      prStatus: null,
      prPollingActive: false,
      prEligible: false,
      mergeCommitSha: null,
      baseBranchOverride: "release/2026.08",
    });
    vi.mocked(ideationApi.sessions.reopen).mockResolvedValue(undefined);
    vi.mocked(ideationApi.apply.toKanban).mockResolvedValue(undefined);

    const { result } = renderHook(() => useResetAndReaccept(), {
      wrapper: createWrapper(),
    });
    await result.current.mutateAsync({
      sessionId: "session-1",
      proposalIds: ["proposal-1"],
    });

    expect(ideationApi.sessions.reopen).toHaveBeenCalledWith("session-1");
    expect(ideationApi.apply.toKanban).toHaveBeenCalledWith({
      sessionId: "session-1",
      proposalIds: ["proposal-1"],
      targetColumn: "backlog",
      baseBranchOverride: "release/2026.08",
    });
  });
});
