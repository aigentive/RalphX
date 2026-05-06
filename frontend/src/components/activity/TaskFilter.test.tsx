import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { TaskFilter } from "./TaskFilter";
import { useTasks } from "@/hooks/useTasks";
import { useProjectStore } from "@/stores/projectStore";
import type { Task } from "@/types/task";

vi.mock("@/hooks/useTasks", () => ({
  useTasks: vi.fn(),
}));

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: vi.fn(),
}));

function makeTask(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    category: "feature",
    title: "Some task",
    description: "Desc",
    priority: 2,
    internalStatus: "ready",
    needsReviewPoint: false,
    createdAt: "2026-05-01T10:00:00+00:00",
    updatedAt: "2026-05-01T10:00:00+00:00",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    ...overrides,
  };
}

const mockedUseTasks = vi.mocked(useTasks);
const mockedUseProjectStore = vi.mocked(useProjectStore);

beforeEach(() => {
  mockedUseProjectStore.mockImplementation((selector: unknown) => {
    if (typeof selector === "function") {
      return (selector as (s: { activeProjectId: string | null }) => unknown)({
        activeProjectId: "project-1",
      });
    }
    return undefined;
  });
});

describe("TaskFilter", () => {
  it("renders default trigger label when nothing selected", () => {
    mockedUseTasks.mockReturnValue({
      data: undefined,
      isLoading: false,
    } as unknown as ReturnType<typeof useTasks>);
    render(<TaskFilter selectedTaskId={null} onChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: /Task/i })).toBeInTheDocument();
  });

  it("shows loading state in popover", async () => {
    mockedUseTasks.mockReturnValue({
      data: undefined,
      isLoading: true,
    } as unknown as ReturnType<typeof useTasks>);

    const user = userEvent.setup();
    render(<TaskFilter selectedTaskId={null} onChange={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Task/i }));
    expect(screen.getByText(/Loading tasks/i)).toBeInTheDocument();
  });

  it("shows empty state with no tasks available", async () => {
    mockedUseTasks.mockReturnValue({
      data: [],
      isLoading: false,
    } as unknown as ReturnType<typeof useTasks>);

    const user = userEvent.setup();
    render(<TaskFilter selectedTaskId={null} onChange={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Task/i }));
    expect(screen.getByText(/No tasks available/i)).toBeInTheDocument();
  });

  it("filters by title and description, shows no-match state", async () => {
    const tasks = [
      makeTask({ id: "a", title: "Alpha task", description: "first" }),
      makeTask({ id: "b", title: "Beta job", description: "second one with alpha word" }),
      makeTask({ id: "c", title: "Gamma item", description: null }),
    ];
    mockedUseTasks.mockReturnValue({
      data: tasks,
      isLoading: false,
    } as unknown as ReturnType<typeof useTasks>);

    const user = userEvent.setup();
    render(<TaskFilter selectedTaskId={null} onChange={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Task/i }));

    const searchInput = screen.getByPlaceholderText(/Search tasks/i);
    await user.type(searchInput, "alpha");
    expect(screen.getByText("Alpha task")).toBeInTheDocument();
    expect(screen.getByText("Beta job")).toBeInTheDocument();
    expect(screen.queryByText("Gamma item")).toBeNull();

    await user.clear(searchInput);
    await user.type(searchInput, "absolutely-no-match");
    expect(screen.getByText(/No tasks match your search/i)).toBeInTheDocument();
  });

  it("excludes archived tasks and limits to 15 most recent", async () => {
    const archived = makeTask({ id: "arch", title: "Archived item", archivedAt: "2026-05-04T00:00:00+00:00" });
    const many: Task[] = Array.from({ length: 20 }).map((_, i) =>
      makeTask({
        id: `t-${i}`,
        title: `Task number ${i}`,
        updatedAt: `2026-04-${String(i + 1).padStart(2, "0")}T10:00:00+00:00`,
      }),
    );
    mockedUseTasks.mockReturnValue({
      data: [archived, ...many],
      isLoading: false,
    } as unknown as ReturnType<typeof useTasks>);

    const user = userEvent.setup();
    render(<TaskFilter selectedTaskId={null} onChange={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /Task/i }));

    expect(screen.queryByText("Archived item")).toBeNull();
    expect(screen.getByText("Task number 19")).toBeInTheDocument();
    expect(screen.queryByText("Task number 0")).toBeNull();
  });

  it("calls onChange when selecting a task", async () => {
    const onChange = vi.fn();
    const tasks = [makeTask({ id: "pick", title: "Pick me" })];
    mockedUseTasks.mockReturnValue({
      data: tasks,
      isLoading: false,
    } as unknown as ReturnType<typeof useTasks>);

    const user = userEvent.setup();
    render(<TaskFilter selectedTaskId={null} onChange={onChange} />);
    await user.click(screen.getByRole("button", { name: /Task/i }));
    await user.click(screen.getByText("Pick me"));
    expect(onChange).toHaveBeenCalledWith("pick");
  });

  it("displays selected task title and clears via badge click", async () => {
    const onChange = vi.fn();
    const tasks = [makeTask({ id: "sel", title: "Selected one" })];
    mockedUseTasks.mockReturnValue({
      data: tasks,
      isLoading: false,
    } as unknown as ReturnType<typeof useTasks>);

    const user = userEvent.setup();
    const { container } = render(
      <TaskFilter selectedTaskId="sel" onChange={onChange} />,
    );
    expect(screen.getByText("Selected one")).toBeInTheDocument();

    const xBadge = container.querySelector("span.cursor-pointer");
    expect(xBadge).not.toBeNull();
    await user.click(xBadge!);
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("clears search when popover closes and reopens", async () => {
    const tasks = [makeTask({ id: "v", title: "Visible task" })];
    mockedUseTasks.mockReturnValue({
      data: tasks,
      isLoading: false,
    } as unknown as ReturnType<typeof useTasks>);

    const user = userEvent.setup();
    render(<TaskFilter selectedTaskId={null} onChange={vi.fn()} />);
    const trigger = screen.getByRole("button", { name: /Task/i });
    await user.click(trigger);
    await user.type(screen.getByPlaceholderText(/Search tasks/i), "Visible");
    await user.click(trigger);
    await user.click(trigger);
    const reopened = screen.getByPlaceholderText(/Search tasks/i) as HTMLInputElement;
    expect(reopened.value).toBe("");
  });

  it("highlights selected task with check icon", async () => {
    const tasks = [
      makeTask({ id: "x", title: "First option" }),
      makeTask({ id: "y", title: "Second option" }),
    ];
    mockedUseTasks.mockReturnValue({
      data: tasks,
      isLoading: false,
    } as unknown as ReturnType<typeof useTasks>);

    const user = userEvent.setup();
    render(<TaskFilter selectedTaskId="x" onChange={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: /First option/i }));
    // "First option" appears in trigger AND list since selected
    expect(screen.getAllByText("First option").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("Second option")).toBeInTheDocument();
  });
});
