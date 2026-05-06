import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { TaskPickerDialog } from "./TaskPickerDialog";
import type { Task } from "@/types/task";

const { tasksMock, useProjectStoreMock } = vi.hoisted(() => ({
  tasksMock: vi.fn(),
  useProjectStoreMock: vi.fn(),
}));

vi.mock("@/hooks/useTasks", () => ({
  useTasks: () => tasksMock(),
}));

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: (selector: (s: { activeProjectId: string | null }) => unknown) =>
    selector(useProjectStoreMock()),
}));

function task(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-1",
    projectId: "proj-1",
    category: "feature",
    title: "Add dependency graph",
    description: "Render dependency edges",
    priority: 0,
    internalStatus: "backlog",
    needsReviewPoint: false,
    createdAt: "2026-04-22T10:00:00Z",
    updatedAt: "2026-04-22T10:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    taskBranch: null,
    worktreePath: null,
    mergeCommitSha: null,
    metadata: null,
    ...overrides,
  };
}

beforeEach(() => {
  useProjectStoreMock.mockReturnValue({ activeProjectId: "proj-1" });
});

describe("TaskPickerDialog", () => {
  it("renders the loading state", () => {
    tasksMock.mockReturnValue({ data: undefined, isLoading: true });
    render(<TaskPickerDialog isOpen onClose={vi.fn()} onSelect={vi.fn()} />);
    expect(screen.getByText(/Loading tasks/i)).toBeInTheDocument();
  });

  it("filters out non-backlog and archived tasks", () => {
    tasksMock.mockReturnValue({
      data: [
        task({ id: "t1", title: "Backlog item", internalStatus: "backlog" }),
        task({ id: "t2", title: "Done item", internalStatus: "merged" }),
        task({
          id: "t3",
          title: "Archived item",
          internalStatus: "backlog",
          archivedAt: "2026-04-22T09:00:00Z",
        }),
      ],
      isLoading: false,
    });
    render(<TaskPickerDialog isOpen onClose={vi.fn()} onSelect={vi.fn()} />);
    expect(screen.getByText("Backlog item")).toBeInTheDocument();
    expect(screen.queryByText("Done item")).toBeNull();
    expect(screen.queryByText("Archived item")).toBeNull();
  });

  it("filters by search query and selects a task", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const onClose = vi.fn();
    tasksMock.mockReturnValue({
      data: [
        task({ id: "t1", title: "Build chart" }),
        task({ id: "t2", title: "Polish kanban" }),
      ],
      isLoading: false,
    });
    render(<TaskPickerDialog isOpen onClose={onClose} onSelect={onSelect} />);

    const input = screen.getByPlaceholderText(/Search tasks/i);
    fireEvent.change(input, { target: { value: "kanban" } });
    expect(screen.queryByText("Build chart")).toBeNull();

    await user.click(screen.getByText("Polish kanban"));
    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: "t2" }));
    expect(onClose).toHaveBeenCalled();
  });
});
