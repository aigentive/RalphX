import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { taskKeys } from "@/hooks/useTasks";
import type { Task } from "@/types/task";
import { MergeConflictTaskDetail as TasksMergeConflict } from "@/components/tasks/detail-views/MergeConflictTaskDetail";
import { MergeIncompleteTaskDetail as TasksMergeIncomplete } from "@/components/tasks/detail-views/MergeIncompleteTaskDetail";

import { MergeConflictTaskDetail as AgentsMergeConflict } from "./MergeConflictTaskDetail";
import { MergeIncompleteTaskDetail as AgentsMergeIncomplete } from "./MergeIncompleteTaskDetail";
import { renderGatedDetailView } from "./agent-gate.test-utils";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), setTaskHistoryState: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@/hooks/useConflictDetection", () => ({
  useConflictDetection: () => ({ conflicts: [], isLoading: false, isEnabled: true }),
}));
vi.mock("@/hooks/useConflictDiff", () => ({
  useConflictDiff: () => ({ data: null, isLoading: false }),
}));
vi.mock("@/hooks/useMergePipeline", () => ({
  useMergePipeline: () => ({ data: undefined }),
}));
vi.mock("@/hooks/usePlanBranchForTask", () => ({
  usePlanBranchForTask: () => ({ data: undefined }),
}));
vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: vi.fn(async () => true),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));
vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (state: { setTaskHistoryState: typeof mocks.setTaskHistoryState }) => unknown) =>
    selector({ setTaskHistoryState: mocks.setTaskHistoryState }),
}));
vi.mock("@/components/diff/ConflictDiffViewer", () => ({ ConflictDiffViewer: () => null }));
vi.mock("@/lib/tauri", () => ({ api: { tasks: { move: vi.fn() } } }));

function task(internalStatus: "merge_incomplete" | "merge_conflict"): Task {
  return {
    id: "task-merge",
    projectId: "project-1",
    title: "Merge recovery",
    description: "Merge recovery fixture",
    internalStatus,
    category: "feature",
    taskBranch: "task/merge-recovery",
    metadata: null,
  } as Task;
}

describe.each([
  ["tasks", TasksMergeIncomplete],
  ["agents", AgentsMergeIncomplete],
] as const)("%s MergeIncompleteTaskDetail gate", (_copy, Component) => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.invoke.mockResolvedValue(undefined);
  });

  it.each(["remote-default", "remote-agent"] as const)(
    "disables unavailable retry in %s and suppresses cache and command effects",
    (environment) => {
      const currentTask = task("merge_incomplete");
      const { queryClient } = renderGatedDetailView(<Component task={currentTask} />, environment);
      queryClient.setQueryData(taskKeys.list(currentTask.projectId), [currentTask]);

      const retry = screen.getByTestId("retry-merge-button");
      expect(retry).toBeDisabled();
      expect(screen.getAllByTestId("agent-gate-tooltip").length).toBeGreaterThan(0);
      retry.click();

      expect(mocks.invoke).not.toHaveBeenCalledWith("retry_merge", expect.anything());
      expect(queryClient.getQueryData<Task[]>(taskKeys.list(currentTask.projectId))?.[0]?.internalStatus)
        .toBe("merge_incomplete");
    },
  );

  it("enables retry locally without a gate hint", () => {
    renderGatedDetailView(<Component task={task("merge_incomplete")} />, "local");
    expect(screen.getByTestId("retry-merge-button")).toBeEnabled();
    expect(screen.queryByTestId("agent-gate-tooltip")).not.toBeInTheDocument();
  });
});

describe.each([
  ["tasks", TasksMergeConflict],
  ["agents", AgentsMergeConflict],
] as const)("%s MergeConflictTaskDetail gate", (_copy, Component) => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.invoke.mockImplementation(async (command: string) => command === "detect_merge_conflicts" ? [] : undefined);
  });

  it.each(["remote-default", "remote-agent"] as const)(
    "disables unavailable resolve and retry in %s without dispatching",
    (environment) => {
      renderGatedDetailView(<Component task={task("merge_conflict")} />, environment);
      const retry = screen.getByTestId("retry-merge-button");
      const resolve = screen.getByTestId("resolve-conflict-button");
      expect(retry).toBeDisabled();
      expect(resolve).toBeDisabled();
      retry.click();
      resolve.click();
      expect(mocks.invoke).not.toHaveBeenCalledWith("retry_merge", expect.anything());
      expect(mocks.invoke).not.toHaveBeenCalledWith("resolve_merge_conflict", expect.anything());
    },
  );

  it("enables resolve and retry locally without a gate hint", () => {
    renderGatedDetailView(<Component task={task("merge_conflict")} />, "local");
    expect(screen.getByTestId("retry-merge-button")).toBeEnabled();
    expect(screen.getByTestId("resolve-conflict-button")).toBeEnabled();
    expect(screen.queryByTestId("agent-gate-tooltip")).not.toBeInTheDocument();
  });
});
