import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { diffApi } from "@/api/diff";
import type {
  AgentWorkspaceChangeSummary,
  AgentWorkspaceReview,
  FileChange,
} from "@/api/diff";

import { useAgentWorkspaceChangeSummary } from "./useAgentWorkspaceChangeSummary";

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceStagedFileChanges: vi.fn(),
    getAgentConversationWorkspaceUnstagedFileChanges: vi.fn(),
    getAgentConversationWorkspaceCommitFileChanges: vi.fn(),
    getAgentConversationWorkspaceCumulativeFileChanges: vi.fn(),
  },
}));

const mockGetStagedFiles = vi.mocked(
  diffApi.getAgentConversationWorkspaceStagedFileChanges,
);
const mockGetUnstagedFiles = vi.mocked(
  diffApi.getAgentConversationWorkspaceUnstagedFileChanges,
);
const mockGetCumulativeFiles = vi.mocked(
  diffApi.getAgentConversationWorkspaceCumulativeFileChanges,
);
const mockGetCommitFiles = vi.mocked(
  diffApi.getAgentConversationWorkspaceCommitFileChanges,
);

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        gcTime: 0,
        retry: false,
      },
    },
  });
}

function makeWrapper(queryClient = makeQueryClient()) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function makeReview(
  changes: FileChange[] = [],
  overrides: Partial<AgentWorkspaceReview> = {},
): AgentWorkspaceReview {
  return {
    changes,
    commits: [],
    baseRef: "origin/main",
    headRef: "HEAD",
    supportsWorktreeModes: true,
    ...overrides,
  };
}

function makeLiveSummary(
  overrides: Partial<AgentWorkspaceChangeSummary> = {},
): AgentWorkspaceChangeSummary {
  return {
    supportsWorktreeModes: true,
    staged: { fileCount: 0, additions: 0, deletions: 0 },
    unstaged: { fileCount: 0, additions: 0, deletions: 0 },
    ...overrides,
  };
}

describe("useAgentWorkspaceChangeSummary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetStagedFiles.mockResolvedValue([]);
    mockGetUnstagedFiles.mockResolvedValue([]);
    mockGetCumulativeFiles.mockResolvedValue([]);
  });

  it("uses live unstaged summary totals without hydrating file lists", () => {
    const liveSummary = makeLiveSummary({
      unstaged: { fileCount: 2, additions: 7, deletions: 3 },
    });

    const { result } = renderHook(
      () =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: null,
          liveSummary,
          hydrateWorktreeFileLists: false,
        }),
      { wrapper: makeWrapper() },
    );

    expect(result.current.effectiveMode).toBe("unstaged");
    expect(result.current.refKind).toEqual({ kind: "unstaged" });
    expect(result.current.workspaceChangeCount).toBe(2);
    expect(result.current.currentFileCount).toBe(2);
    expect(result.current.totalAdditions).toBe(7);
    expect(result.current.totalDeletions).toBe(3);
    expect(mockGetStagedFiles).not.toHaveBeenCalled();
    expect(mockGetUnstagedFiles).not.toHaveBeenCalled();
  });

  it("hydrates staged files only when live summary selects staged mode", async () => {
    const stagedFile: FileChange = {
      path: "src/staged.ts",
      status: "modified",
      additions: 2,
      deletions: 1,
      isGenerated: false,
    };
    mockGetStagedFiles.mockResolvedValue([stagedFile]);
    const liveSummary = makeLiveSummary({
      staged: { fileCount: 1, additions: 2, deletions: 1 },
      unstaged: { fileCount: 0, additions: 0, deletions: 0 },
    });

    const { result } = renderHook(
      () =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: makeReview(),
          liveSummary,
        }),
      { wrapper: makeWrapper() },
    );

    expect(result.current.effectiveMode).toBe("staged");
    expect(result.current.currentFileCount).toBe(1);
    expect(result.current.totalAdditions).toBe(2);
    expect(result.current.totalDeletions).toBe(1);

    await waitFor(() => expect(mockGetStagedFiles).toHaveBeenCalledWith("conversation-1"));
    await waitFor(() => expect(result.current.currentFiles).toEqual([stagedFile]));
    expect(mockGetUnstagedFiles).not.toHaveBeenCalled();
  });

  it("uses the default mode when no user-selected mode exists", async () => {
    const cumulativeFile: FileChange = {
      path: "src/merged.ts",
      status: "modified",
      additions: 4,
      deletions: 1,
      isGenerated: false,
    };
    mockGetCumulativeFiles.mockResolvedValue([cumulativeFile]);

    const { result } = renderHook(
      () =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: makeReview(),
          defaultMode: "cumulative",
        }),
      { wrapper: makeWrapper() },
    );

    expect(result.current.effectiveMode).toBe("cumulative");
    expect(result.current.refKind).toEqual({ kind: "cumulative_head" });
    await waitFor(() => expect(mockGetCumulativeFiles).toHaveBeenCalledWith("conversation-1"));
    await waitFor(() => expect(result.current.currentFiles).toEqual([cumulativeFile]));
  });

  it("preserves an explicit user-selected mode over the default mode", () => {
    const { result } = renderHook(
      () =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: makeReview(),
          defaultMode: "cumulative",
        }),
      { wrapper: makeWrapper() },
    );

    expect(result.current.effectiveMode).toBe("cumulative");

    act(() => {
      result.current.setMode("uncommitted");
    });

    expect(result.current.effectiveMode).toBe("uncommitted");
    expect(result.current.refKind).toEqual({ kind: "head" });
  });

  it("resets explicit mode selection when the conversation changes", async () => {
    const { result, rerender } = renderHook(
      ({ conversationId, defaultMode }) =>
        useAgentWorkspaceChangeSummary({
          conversationId,
          review: makeReview(),
          defaultMode,
        }),
      {
        initialProps: {
          conversationId: "conversation-1",
          defaultMode: "cumulative",
        },
        wrapper: makeWrapper(),
      },
    );

    expect(result.current.effectiveMode).toBe("cumulative");

    act(() => {
      result.current.setMode("uncommitted");
    });

    expect(result.current.effectiveMode).toBe("uncommitted");

    rerender({
      conversationId: "conversation-2",
      defaultMode: "cumulative",
    });

    await waitFor(() => expect(result.current.effectiveMode).toBe("cumulative"));
    expect(result.current.refKind).toEqual({ kind: "cumulative_head" });
  });

  it("falls back to cumulative files for non-worktree summaries", async () => {
    const cumulativeFile: FileChange = {
      path: "src/plan-branch.ts",
      status: "modified",
      additions: 3,
      deletions: 1,
      isGenerated: false,
    };
    mockGetCumulativeFiles.mockResolvedValue([cumulativeFile]);
    const liveSummary = makeLiveSummary({
      supportsWorktreeModes: false,
      staged: { fileCount: 2, additions: 6, deletions: 2 },
      unstaged: { fileCount: 1, additions: 3, deletions: 1 },
    });

    const { result } = renderHook(
      () =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: makeReview([], { supportsWorktreeModes: false }),
          liveSummary,
          hydrateWorktreeFileLists: true,
        }),
      { wrapper: makeWrapper() },
    );

    expect(result.current.supportsWorktreeModes).toBe(false);
    expect(result.current.effectiveMode).toBe("cumulative");
    expect(result.current.refKind).toEqual({ kind: "cumulative_head" });
    await waitFor(() => expect(mockGetCumulativeFiles).toHaveBeenCalledWith("conversation-1"));
    await waitFor(() => expect(result.current.currentFiles).toEqual([cumulativeFile]));
    expect(mockGetStagedFiles).not.toHaveBeenCalled();
    expect(mockGetUnstagedFiles).not.toHaveBeenCalled();
  });

  it("does not reuse active cumulative cache for terminal review history", () => {
    const queryClient = makeQueryClient();
    const activeFile: FileChange = {
      path: "src/active-only.ts",
      status: "modified",
      additions: 9,
      deletions: 2,
      isGenerated: false,
    };
    const publishedFile: FileChange = {
      path: "src/published.ts",
      status: "modified",
      additions: 3,
      deletions: 1,
      isGenerated: false,
    };
    queryClient.setQueryData(
      ["agents", "workspace-diff", "conversation-1", "cumulative-files"],
      [activeFile],
    );

    const { result } = renderHook(
      () =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: makeReview([publishedFile], {
            baseRef: "base-sha",
            headRef: "refs/ralphx/pr-heads/451",
            supportsWorktreeModes: false,
          }),
          defaultMode: "cumulative",
        }),
      { wrapper: makeWrapper(queryClient) },
    );

    expect(result.current.currentFiles).toEqual([publishedFile]);
    expect(result.current.currentFiles).not.toContainEqual(activeFile);
  });

  it("disables every query and neutralizes cached results when the surface is disabled", () => {
    const queryClient = makeQueryClient();
    const cachedFile = {
      path: "src/historical.ts",
      status: "modified" as const,
      additions: 8,
      deletions: 3,
      isGenerated: false,
    };
    queryClient.setQueryData(
      ["agents", "workspace-diff", "conversation-1", "staged-files"],
      [cachedFile],
    );
    queryClient.setQueryData(
      ["agents", "workspace-diff", "conversation-1", "unstaged-files"],
      [cachedFile],
    );
    queryClient.setQueryData(
      ["agents", "workspace-diff", "conversation-1", "commit-files", "sha-1"],
      [cachedFile],
    );
    queryClient.setQueryData(
      ["agents", "workspace-diff", "conversation-1", "cumulative-files"],
      [cachedFile],
    );

    const { result } = renderHook(
      () => ({
        commit: useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: makeReview([cachedFile]),
          defaultMode: "sha-1",
          enabled: false,
        }),
        cumulative: useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: makeReview([cachedFile], { supportsWorktreeModes: false }),
          defaultMode: "cumulative",
          enabled: false,
        }),
      }),
      { wrapper: makeWrapper(queryClient) },
    );

    expect(mockGetStagedFiles).not.toHaveBeenCalled();
    expect(mockGetUnstagedFiles).not.toHaveBeenCalled();
    expect(mockGetCommitFiles).not.toHaveBeenCalled();
    expect(mockGetCumulativeFiles).not.toHaveBeenCalled();
    for (const state of [result.current.commit, result.current.cumulative]) {
      expect(state.currentFiles).toEqual([]);
      expect(state.currentFileCount).toBe(0);
      expect(state.workspaceChangeCount).toBe(0);
      expect(state.totalAdditions).toBe(0);
      expect(state.totalDeletions).toBe(0);
      expect(state.currentFilesError).toBeNull();
      expect(state.isCurrentFilesLoading).toBe(false);
    }
  });
});
