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

import {
  getAgentWorkspaceChangeFacts,
  useAgentWorkspaceChangeSummary,
} from "./useAgentWorkspaceChangeSummary";

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

  it("keeps failed staged and unstaged reads unknown instead of laundering them as empty", async () => {
    mockGetStagedFiles.mockRejectedValue(new Error("staged unavailable"));
    mockGetUnstagedFiles.mockRejectedValue(new Error("unstaged unavailable"));

    const { result } = renderHook(
      () =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-failed",
          review: makeReview(),
        }),
      { wrapper: makeWrapper() },
    );

    await waitFor(() => {
      expect(result.current.stagedFilesKnowledge).toBe("unknown");
      expect(result.current.unstagedFilesKnowledge).toBe("unknown");
    });
  });

  it("reports genuine empty staged and unstaged reads as known empty", async () => {
    const { result } = renderHook(
      () =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-empty",
          review: makeReview(),
        }),
      { wrapper: makeWrapper() },
    );

    await waitFor(() => {
      expect(result.current.stagedFilesKnowledge).toBe("knownEmpty");
      expect(result.current.unstagedFilesKnowledge).toBe("knownEmpty");
    });
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

  it("uses review changes for workspace count when a clean live summary is present", () => {
    const reviewChanges = [
      {
        path: "src/committed.ts",
        status: "modified" as const,
        additions: 3,
        deletions: 1,
        isGenerated: false,
      },
      {
        path: "src/also-committed.ts",
        status: "added" as const,
        additions: 4,
        deletions: 0,
        isGenerated: false,
      },
    ];

    const { result } = renderHook(
      () =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: makeReview(reviewChanges),
          liveSummary: makeLiveSummary(),
        }),
      { wrapper: makeWrapper() },
    );

    expect(result.current.effectiveMode).toBe("uncommitted");
    expect(result.current.workspaceChangeCount).toBe(reviewChanges.length);
    expect(result.current.currentFileCount).toBe(reviewChanges.length);
    expect(result.current.currentFiles).toEqual(reviewChanges);
    expect(result.current.stagedCount).toBe(0);
    expect(result.current.unstagedCount).toBe(0);
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
          review: makeReview([
            {
              path: "src/committed.ts",
              status: "modified",
              additions: 3,
              deletions: 1,
              isGenerated: false,
            },
            {
              path: "src/also-committed.ts",
              status: "added",
              additions: 4,
              deletions: 0,
              isGenerated: false,
            },
          ]),
          liveSummary,
        }),
      { wrapper: makeWrapper() },
    );

    expect(result.current.effectiveMode).toBe("staged");
    expect(result.current.workspaceChangeCount).toBe(2);
    expect(result.current.currentFileCount).toBe(1);
    expect(result.current.totalAdditions).toBe(2);
    expect(result.current.totalDeletions).toBe(1);

    await waitFor(() => expect(mockGetStagedFiles).toHaveBeenCalledWith("conversation-1"));
    await waitFor(() => expect(result.current.currentFiles).toEqual([stagedFile]));
    expect(mockGetUnstagedFiles).not.toHaveBeenCalled();
  });

  it("refreshes the unstaged file list when its live bucket changes", async () => {
    const firstFile: FileChange = {
      path: "src/first.ts",
      status: "modified",
      additions: 2,
      deletions: 1,
      isGenerated: false,
    };
    const secondFile: FileChange = {
      path: "src/second.ts",
      status: "added",
      additions: 4,
      deletions: 0,
      isGenerated: false,
    };
    mockGetUnstagedFiles.mockResolvedValue([firstFile]);

    const { result, rerender } = renderHook(
      ({ liveSummary }: { liveSummary: AgentWorkspaceChangeSummary }) =>
        useAgentWorkspaceChangeSummary({
          conversationId: "conversation-1",
          review: makeReview(),
          liveSummary,
        }),
      {
        initialProps: {
          liveSummary: makeLiveSummary({
            unstaged: { fileCount: 1, additions: 2, deletions: 1 },
          }),
        },
        wrapper: makeWrapper(),
      },
    );

    await waitFor(() => expect(result.current.currentFiles).toEqual([firstFile]));
    mockGetUnstagedFiles.mockResolvedValue([firstFile, secondFile]);

    rerender({
      liveSummary: makeLiveSummary({
        unstaged: { fileCount: 2, additions: 6, deletions: 1 },
      }),
    });

    await waitFor(() => expect(mockGetUnstagedFiles).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(result.current.currentFiles).toEqual([firstFile, secondFile]),
    );
    expect(result.current.currentFileCount).toBe(2);
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

describe("getAgentWorkspaceChangeFacts", () => {
  it("aggregates live staged, unstaged, and conflicted files without fabricating conflict deltas", () => {
    expect(
      getAgentWorkspaceChangeFacts(
        makeLiveSummary({
          staged: { fileCount: 2, additions: 6, deletions: 2 },
          unstaged: { fileCount: 1, additions: 3, deletions: 1 },
          conflicted: { fileCount: 2, files: ["a.ts", "b.ts"] },
        }),
        makeReview(),
      ),
    ).toEqual({
      fileCount: 5,
      additions: 9,
      deletions: 3,
    });
  });

  it("falls back to loaded review changes when live worktree facts are unavailable", () => {
    expect(
      getAgentWorkspaceChangeFacts(
        makeLiveSummary({
          supportsWorktreeModes: false,
          staged: { fileCount: 9, additions: 90, deletions: 9 },
        }),
        makeReview([
          {
            path: "src/review.ts",
            status: "modified",
            additions: 4,
            deletions: 2,
            isGenerated: false,
          },
        ]),
      ),
    ).toEqual({
      fileCount: 1,
      additions: 4,
      deletions: 2,
    });
  });

  it("returns null while neither live nor review facts are known", () => {
    expect(getAgentWorkspaceChangeFacts(null, null)).toBeNull();
  });
});
