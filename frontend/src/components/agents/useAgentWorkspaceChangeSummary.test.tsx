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

function makeReview(changes: FileChange[] = []): AgentWorkspaceReview {
  return {
    changes,
    commits: [],
    baseRef: "origin/main",
    headRef: "HEAD",
    supportsWorktreeModes: true,
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
});
