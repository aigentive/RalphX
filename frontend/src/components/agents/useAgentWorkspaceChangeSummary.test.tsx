import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
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
});
