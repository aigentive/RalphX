import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Task } from "@/types/task";

import { BranchUpdateTaskDetail } from "./BranchUpdateTaskDetail";

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(() => Promise.resolve(null)),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));

function task(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-branch-update",
    projectId: "project-1",
    category: "feature",
    title: "Update branches",
    description: "",
    priority: 2,
    internalStatus: "branch_update_blocked",
    needsReviewPoint: false,
    createdAt: "2026-07-13T00:00:00Z",
    updatedAt: "2026-07-13T00:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    taskBranch: "ralphx/task-branch-update",
    worktreePath: null,
    mergeCommitSha: null,
    metadata: JSON.stringify({
      branch_update: {
        direction: "task_branch",
        source_branch: "ralphx/plan",
        target_branch: "ralphx/task-branch-update",
        failure_kind: "conflict",
        diagnostics: "Resolve src/main.rs before continuing",
        conflict_files: ["src/main.rs"],
      },
    }),
    ...overrides,
  };
}

function Wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      {children}
    </QueryClientProvider>
  );
}

describe("BranchUpdateTaskDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue(task({ internalStatus: "updating_task_branch" }));
  });

  it("renders truthful blocked branch-update telemetry", () => {
    render(<BranchUpdateTaskDetail task={task()} />, { wrapper: Wrapper });

    expect(screen.getByText("Branch update needs attention")).toBeInTheDocument();
    expect(screen.getByText("task branch")).toBeInTheDocument();
    expect(
      screen.getByText("ralphx/plan → ralphx/task-branch-update"),
    ).toBeInTheDocument();
    expect(screen.getByText("conflict")).toBeInTheDocument();
    expect(
      screen.getByText("Resolve src/main.rs before continuing"),
    ).toBeInTheDocument();
    expect(screen.getByText("src/main.rs")).toBeInTheDocument();
  });

  it("retries through the dedicated command", async () => {
    const user = userEvent.setup();
    render(<BranchUpdateTaskDetail task={task()} />, { wrapper: Wrapper });

    await user.click(screen.getByRole("button", { name: "Retry branch update" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("retry_branch_update", {
        taskId: "task-branch-update",
      });
    });
  });

  it("keeps historical checkpoints read-only", () => {
    render(<BranchUpdateTaskDetail task={task()} isHistorical />, { wrapper: Wrapper });

    expect(screen.getByText("Historical checkpoint — controls are read-only.")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Retry branch update" }),
    ).not.toBeInTheDocument();
  });
});
