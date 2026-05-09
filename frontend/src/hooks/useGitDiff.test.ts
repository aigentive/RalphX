/**
 * useGitDiff hook tests
 *
 * Tests the hook's integration with the diffApi for fetching
 * file changes and diff data from agent activity events.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { useGitDiff } from "./useGitDiff";

// Mock the diffApi
vi.mock("@/api/diff", () => ({
  diffApi: {
    getTaskFileChanges: vi.fn(),
    getFileDiff: vi.fn(),
    getTaskCommits: vi.fn(),
    getCommitFileChanges: vi.fn(),
    getCommitFileDiff: vi.fn(),
  },
}));

import { diffApi } from "@/api/diff";

const mockFileChanges = [
  {
    path: "src/components/auth/LoginForm.tsx",
    status: "modified" as const,
    additions: 25,
    deletions: 10,
  },
  {
    path: "src/hooks/useAuth.ts",
    status: "modified" as const,
    additions: 15,
    deletions: 3,
  },
  {
    path: "src/lib/api/auth.ts",
    status: "added" as const,
    additions: 45,
    deletions: 0,
  },
];

const mockFileDiff = {
  filePath: "src/components/LoginForm.tsx",
  oldContent: "// Old content\nexport function Login() {\n  return null;\n}\n",
  newContent: "// New content\nexport function Login() {\n  return <form />;\n}\n",
  language: "typescript",
};

const staleBaseCommits = [
  {
    sha: "base-ahead-commit",
    shortSha: "base123",
    message: "fix: unrelated base branch work",
    author: "Base Author",
    date: "2026-04-29T10:00:00+00:00",
  },
];

const taskScopedCommits = [
  {
    sha: "task-commit",
    shortSha: "task123",
    message: "feat: selected task work",
    author: "Task Author",
    date: "2026-04-29T11:00:00+00:00",
  },
];

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useGitDiff", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(diffApi.getTaskFileChanges).mockResolvedValue(mockFileChanges);
    vi.mocked(diffApi.getFileDiff).mockResolvedValue(mockFileDiff);
    vi.mocked(diffApi.getTaskCommits).mockResolvedValue([]);
    vi.mocked(diffApi.getCommitFileChanges).mockResolvedValue([]);
    vi.mocked(diffApi.getCommitFileDiff).mockResolvedValue(mockFileDiff);
  });

  describe("initialization", () => {
    it("starts with loading state true when enabled", async () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: true })
      );

      expect(result.current.isLoadingChanges).toBe(true);

      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(false);
      });
    });

    it("does not load when disabled", () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: false })
      );

      expect(result.current.changes).toEqual([]);
      expect(result.current.commits).toEqual([]);
      expect(result.current.isLoadingChanges).toBe(false);
      expect(diffApi.getTaskFileChanges).not.toHaveBeenCalled();
    });

    it("does not load when taskId is empty", () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "", enabled: true })
      );

      expect(result.current.changes).toEqual([]);
      expect(diffApi.getTaskFileChanges).not.toHaveBeenCalled();
    });
  });

  describe("data loading", () => {
    it("calls getTaskFileChanges with correct parameters", async () => {
      renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: true })
      );

      await waitFor(() => {
        expect(diffApi.getTaskFileChanges).toHaveBeenCalledWith("task-1");
      });
    });

    it("loads file changes data", async () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: true })
      );

      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(false);
      });

      expect(result.current.changes).toEqual(mockFileChanges);
      expect(result.current.changes[0]).toHaveProperty("path");
      expect(result.current.changes[0]).toHaveProperty("status");
      expect(result.current.changes[0]).toHaveProperty("additions");
      expect(result.current.changes[0]).toHaveProperty("deletions");
    });

    it("loads commit history", async () => {
      vi.mocked(diffApi.getTaskCommits).mockResolvedValue(taskScopedCommits);

      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: true })
      );

      await waitFor(() => {
        expect(result.current.isLoadingHistory).toBe(false);
      });

      expect(diffApi.getTaskCommits).toHaveBeenCalledWith("task-1");
      expect(result.current.commits).toEqual(taskScopedCommits);
    });

    it("sets error to null on successful load", async () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: true })
      );

      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(false);
      });

      expect(result.current.error).toBeNull();
    });

    it("sets error on API failure", async () => {
      vi.mocked(diffApi.getTaskFileChanges).mockRejectedValue(new Error("API Error"));

      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: true })
      );

      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(false);
      });

      expect(result.current.error).toBeInstanceOf(Error);
      expect(result.current.error?.message).toBe("API Error");
      expect(result.current.changes).toEqual([]);
    });
  });

  describe("fetchDiff", () => {
    it("returns diff data for a file path", async () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: true })
      );

      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(false);
      });

      let diffData;
      await act(async () => {
        diffData = await result.current.fetchDiff("src/components/LoginForm.tsx");
      });

      expect(diffApi.getFileDiff).toHaveBeenCalledWith(
        "task-1",
        "src/components/LoginForm.tsx"
      );
      expect(diffData).not.toBeNull();
      expect(diffData).toHaveProperty("filePath", "src/components/LoginForm.tsx");
      expect(diffData).toHaveProperty("oldContent");
      expect(diffData).toHaveProperty("newContent");
      expect(diffData).toHaveProperty("hunks");
      expect(diffData).toHaveProperty("language");
    });

    it("returns null when disabled", async () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: false })
      );

      let diffData;
      await act(async () => {
        diffData = await result.current.fetchDiff("src/file.ts");
      });

      expect(diffData).toBeNull();
      expect(diffApi.getFileDiff).not.toHaveBeenCalled();
    });

    it("returns null when taskId is missing", async () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "", enabled: true })
      );

      let diffData;
      await act(async () => {
        diffData = await result.current.fetchDiff("src/file.ts");
      });

      expect(diffData).toBeNull();
      expect(diffApi.getFileDiff).not.toHaveBeenCalled();
    });

    it("returns null on API error", async () => {
      vi.mocked(diffApi.getFileDiff).mockRejectedValue(new Error("Diff API Error"));

      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: true })
      );

      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(false);
      });

      let diffData;
      await act(async () => {
        diffData = await result.current.fetchDiff("src/file.ts");
      });

      expect(diffData).toBeNull();
    });
  });

  describe("refresh", () => {
    it("sets loading state during refresh", async () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: true })
      );

      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(false);
      });

      vi.mocked(diffApi.getTaskFileChanges).mockClear();
      const refreshChanges = deferred<typeof mockFileChanges>();
      vi.mocked(diffApi.getTaskFileChanges).mockReturnValueOnce(
        refreshChanges.promise
      );

      let refreshPromise: Promise<void> = Promise.resolve();
      act(() => {
        refreshPromise = result.current.refresh();
      });

      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(true);
      });

      await act(async () => {
        refreshChanges.resolve(mockFileChanges);
        await refreshPromise;
      });

      expect(result.current.isLoadingChanges).toBe(false);
      expect(diffApi.getTaskFileChanges).toHaveBeenCalledTimes(1);
    });

    it("does nothing when disabled", async () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "task-1", enabled: false })
      );

      await act(async () => {
        await result.current.refresh();
      });

      expect(result.current.isLoadingChanges).toBe(false);
      expect(diffApi.getTaskFileChanges).not.toHaveBeenCalled();
    });

    it("does nothing when taskId is missing", async () => {
      const { result } = renderHook(() =>
        useGitDiff({ taskId: "", enabled: true })
      );

      await act(async () => {
        await result.current.refresh();
      });

      expect(diffApi.getTaskFileChanges).not.toHaveBeenCalled();
    });
  });

  describe("re-fetching on taskId change", () => {
    it("refetches data when taskId changes", async () => {
      const { result, rerender } = renderHook(
        ({ taskId }) => useGitDiff({ taskId, enabled: true }),
        { initialProps: { taskId: "task-1" } }
      );

      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(false);
      });

      expect(diffApi.getTaskFileChanges).toHaveBeenCalledWith("task-1");

      // Change taskId
      rerender({ taskId: "task-2" });

      // Should start loading again
      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(true);
      });

      // Should eventually finish
      await waitFor(() => {
        expect(result.current.isLoadingChanges).toBe(false);
      });

      expect(diffApi.getTaskFileChanges).toHaveBeenCalledWith("task-2");
    });

    it("ignores stale commit history when an earlier task request resolves late", async () => {
      const task1Commits = deferred<typeof staleBaseCommits>();
      const task2Commits = deferred<typeof taskScopedCommits>();

      vi.mocked(diffApi.getTaskCommits).mockImplementation((taskId) => {
        if (taskId === "task-1") return task1Commits.promise;
        if (taskId === "task-2") return task2Commits.promise;
        return Promise.resolve([]);
      });

      const { result, rerender } = renderHook(
        ({ taskId }) => useGitDiff({ taskId, enabled: true }),
        { initialProps: { taskId: "task-1" } }
      );

      await waitFor(() => {
        expect(diffApi.getTaskCommits).toHaveBeenCalledWith("task-1");
      });

      rerender({ taskId: "task-2" });

      await waitFor(() => {
        expect(diffApi.getTaskCommits).toHaveBeenCalledWith("task-2");
      });

      await act(async () => {
        task2Commits.resolve(taskScopedCommits);
        await task2Commits.promise;
      });

      await waitFor(() => {
        expect(result.current.commits).toEqual(taskScopedCommits);
      });

      await act(async () => {
        task1Commits.resolve(staleBaseCommits);
        await task1Commits.promise;
      });

      expect(result.current.commits).toEqual(taskScopedCommits);
      expect(result.current.commits).not.toEqual(staleBaseCommits);
    });
  });
});
