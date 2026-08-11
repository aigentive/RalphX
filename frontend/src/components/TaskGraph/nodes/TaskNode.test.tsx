import React from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TaskNode, type TaskNodeData, type TaskNodeHandlers } from "./TaskNode";

vi.mock("@/hooks/useTaskSteps", () => ({
  useStepProgress: vi.fn(() => ({ data: null })),
}));

type MockMenuHandlers = {
  onViewDetails: () => void;
  onStartExecution: () => void;
  onBlockWithReason: (reason?: string) => void;
  onUnblock: () => void;
  onApprove: () => void;
  onReject: () => void;
  onRequestChanges: () => void;
  onMarkResolved: () => void;
  onRemove: () => void;
};

vi.mock("./TaskNodeContextMenu", () => ({
  TaskNodeContextMenu: ({
    children,
    task,
    handlers,
    groupInfo,
  }: {
    children: React.ReactNode;
    task: { id: string; internalStatus: string };
    handlers: MockMenuHandlers;
    groupInfo?: { groupLabel: string };
  }) => (
    <div
      data-testid="context-menu-wrapper"
      data-task-id={task.id}
      data-task-status={task.internalStatus}
      data-group-label={groupInfo?.groupLabel ?? ""}
    >
      <button
        data-testid="menu-view-details"
        onClick={() => handlers.onViewDetails()}
      />
      <button
        data-testid="menu-start"
        onClick={() => handlers.onStartExecution()}
      />
      <button
        data-testid="menu-block"
        onClick={() => handlers.onBlockWithReason("nope")}
      />
      <button
        data-testid="menu-unblock"
        onClick={() => handlers.onUnblock()}
      />
      <button
        data-testid="menu-approve"
        onClick={() => handlers.onApprove()}
      />
      <button data-testid="menu-reject" onClick={() => handlers.onReject()} />
      <button
        data-testid="menu-request-changes"
        onClick={() => handlers.onRequestChanges()}
      />
      <button
        data-testid="menu-resolved"
        onClick={() => handlers.onMarkResolved()}
      />
      <button data-testid="menu-remove" onClick={() => handlers.onRemove()} />
      {children}
    </div>
  ),
}));

import { useStepProgress } from "@/hooks/useTaskSteps";

function makeData(overrides: Partial<TaskNodeData> = {}): TaskNodeData {
  return {
    label: "Sample task",
    taskId: "task-1",
    internalStatus: "ready",
    priority: 1,
    isCriticalPath: false,
    ...overrides,
  } as TaskNodeData;
}

function renderTaskNode(
  data: TaskNodeData,
  options: { selected?: boolean } = {}
) {
  const props = {
    id: data.taskId,
    type: "task",
    selected: options.selected ?? false,
    data,
    isConnectable: true,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    zIndex: 0,
  } as React.ComponentProps<typeof TaskNode>;

  return render(
    <ReactFlowProvider>
      <TaskNode {...props} />
    </ReactFlowProvider>
  );
}

describe("TaskNode", () => {
  it("renders plan merge category and merged PR context", () => {
    renderTaskNode(
      makeData({
        label: "Merge plan into main",
        taskId: "task-123",
        internalStatus: "merged",
        priority: 2,
        category: "plan_merge",
        mergeTarget: "main",
        prNumber: 68,
        prStatus: "Open",
        planBranchStatus: "merged",
        description: "Auto-created merge task",
      })
    );

    expect(screen.getByText("Merge -> main")).toBeInTheDocument();
    expect(screen.getByTestId("graph-pr-indicator")).toHaveTextContent(
      "PR #68 merged"
    );
    expect(screen.queryByText("plan_merge")).not.toBeInTheDocument();
  });

  it("renders PR closed state", () => {
    renderTaskNode(
      makeData({
        category: "plan_merge",
        mergeTarget: "develop",
        prNumber: 12,
        prStatus: "Closed",
      })
    );
    expect(screen.getByTestId("graph-pr-indicator")).toHaveTextContent(
      "PR #12 closed"
    );
  });

  it("renders PR review state when not merged or closed", () => {
    renderTaskNode(
      makeData({
        category: "plan_merge",
        mergeTarget: "main",
        prNumber: 99,
        prStatus: "Open",
      })
    );
    const indicator = screen.getByTestId("graph-pr-indicator");
    expect(indicator).toHaveTextContent("PR #99");
    expect(indicator).not.toHaveTextContent("merged");
    expect(indicator).not.toHaveTextContent("closed");
  });

  it("falls back to plain Merge label when mergeTarget is missing", () => {
    renderTaskNode(
      makeData({ category: "plan_merge", prNumber: 7, prStatus: "Open" })
    );
    expect(screen.getByText("Merge")).toBeInTheDocument();
  });

  it("treats internalStatus=merged as merged PR even when prStatus is open", () => {
    renderTaskNode(
      makeData({
        category: "plan_merge",
        mergeTarget: "main",
        prNumber: 5,
        prStatus: "Open",
        internalStatus: "merged",
      })
    );
    expect(screen.getByTestId("graph-pr-indicator")).toHaveTextContent(
      "PR #5 merged"
    );
  });

  it("does not render PR indicator without plan_merge category", () => {
    renderTaskNode(
      makeData({ category: "feature", prNumber: 11, prStatus: "Open" })
    );
    expect(screen.queryByTestId("graph-pr-indicator")).not.toBeInTheDocument();
  });

  it("does not render PR indicator when prNumber is null", () => {
    renderTaskNode(
      makeData({ category: "plan_merge", mergeTarget: "main", prNumber: null })
    );
    expect(screen.queryByTestId("graph-pr-indicator")).not.toBeInTheDocument();
  });

  it("renders generic category label when category is not plan_merge", () => {
    renderTaskNode(makeData({ category: "feature" }));
    // category label shown; ensure it is not the merge variant
    expect(screen.queryByText(/Merge ->/)).not.toBeInTheDocument();
  });

  it("renders description when provided", () => {
    renderTaskNode(makeData({ description: "Detailed description here" }));
    expect(screen.getByText("Detailed description here")).toBeInTheDocument();
  });

  it("does not render description block when description is null", () => {
    renderTaskNode(makeData({ description: null }));
    expect(screen.queryByText(/Detailed description/)).not.toBeInTheDocument();
  });

  it("sets data attributes for status and flags", () => {
    renderTaskNode(
      makeData({
        internalStatus: "executing",
        isCriticalPath: true,
        isHighlighted: true,
        isFocused: false,
      })
    );
    const node = screen.getByTestId("task-node");
    expect(node.getAttribute("data-status")).toBe("executing");
    expect(node.getAttribute("data-critical-path")).toBe("true");
    expect(node.getAttribute("data-highlighted")).toBe("true");
    expect(node.getAttribute("data-focused")).toBe("false");
  });

  it("applies critical path ring class when not selected/highlighted/focused", () => {
    const { container } = renderTaskNode(
      makeData({ isCriticalPath: true, isHighlighted: false, isFocused: false })
    );
    const inner = container.querySelector(".relative.rounded-lg");
    expect(inner?.className).toMatch(/ring-1/);
  });

  it("does not apply critical-path ring when node is selected", () => {
    const { container } = renderTaskNode(
      makeData({ isCriticalPath: true }),
      { selected: true }
    );
    const inner = container.querySelector(".relative.rounded-lg");
    expect(inner?.className).not.toMatch(/ring-1 ring/);
  });

  it("applies thicker accent border when isFocused is true", () => {
    const { container } = renderTaskNode(makeData({ isFocused: true }));
    const inner = container.querySelector(
      ".relative.rounded-lg"
    ) as HTMLElement | null;
    expect(inner).not.toBeNull();
    expect(inner!.style.borderWidth).toBe("2px");
  });

  it("renders without context menu when handlers are absent", () => {
    renderTaskNode(makeData());
    expect(
      screen.queryByTestId("context-menu-wrapper")
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("task-node")).toBeInTheDocument();
  });

  it("wraps node in context menu when handlers are provided", () => {
    const handlers: TaskNodeHandlers = {
      onViewDetails: vi.fn(),
      onStartExecution: vi.fn(),
      onBlockWithReason: vi.fn(),
      onUnblock: vi.fn(),
      onApprove: vi.fn(),
      onReject: vi.fn(),
      onRequestChanges: vi.fn(),
      onMarkResolved: vi.fn(),
      onRemove: vi.fn(),
    };
    renderTaskNode(makeData({ handlers, taskId: "task-with-menu" }));
    const wrapper = screen.getByTestId("context-menu-wrapper");
    expect(wrapper.getAttribute("data-task-id")).toBe("task-with-menu");
    expect(wrapper.querySelector('[data-testid="task-node"]')).not.toBeNull();
  });

  it("invokes handler callbacks with the bound taskId", async () => {
    const onViewDetails = vi.fn();
    const onStartExecution = vi.fn();
    const onBlockWithReason = vi.fn();
    const onUnblock = vi.fn();
    const onApprove = vi.fn();
    const onReject = vi.fn();
    const onRequestChanges = vi.fn();
    const onMarkResolved = vi.fn();
    const onRemove = vi.fn();

    const handlers: TaskNodeHandlers = {
      onViewDetails,
      onStartExecution,
      onBlockWithReason,
      onUnblock,
      onApprove,
      onReject,
      onRequestChanges,
      onMarkResolved,
      onRemove,
    };

    renderTaskNode(makeData({ handlers, taskId: "task-callbacks" }));

    screen.getByTestId("menu-view-details").click();
    screen.getByTestId("menu-start").click();
    screen.getByTestId("menu-block").click();
    screen.getByTestId("menu-unblock").click();
    screen.getByTestId("menu-approve").click();
    screen.getByTestId("menu-reject").click();
    screen.getByTestId("menu-request-changes").click();
    screen.getByTestId("menu-resolved").click();
    screen.getByTestId("menu-remove").click();

    expect(onViewDetails).toHaveBeenCalledWith("task-callbacks");
    expect(onStartExecution).toHaveBeenCalledWith("task-callbacks");
    expect(onBlockWithReason).toHaveBeenCalledWith("task-callbacks", "nope");
    expect(onUnblock).toHaveBeenCalledWith("task-callbacks");
    expect(onApprove).toHaveBeenCalledWith("task-callbacks");
    expect(onReject).toHaveBeenCalledWith("task-callbacks");
    expect(onRequestChanges).toHaveBeenCalledWith("task-callbacks");
    expect(onMarkResolved).toHaveBeenCalledWith("task-callbacks");
    expect(onRemove).toHaveBeenCalledWith("task-callbacks");
  });

  it("safely no-ops optional handler callbacks when not provided", () => {
    const onViewDetails = vi.fn();
    // Only required handler set — the rest are undefined; wrappers should not throw
    const handlers: TaskNodeHandlers = { onViewDetails };
    renderTaskNode(makeData({ handlers, taskId: "task-optional" }));

    expect(() => screen.getByTestId("menu-start").click()).not.toThrow();
    expect(() => screen.getByTestId("menu-block").click()).not.toThrow();
    expect(() => screen.getByTestId("menu-unblock").click()).not.toThrow();
    expect(() => screen.getByTestId("menu-approve").click()).not.toThrow();
    expect(() => screen.getByTestId("menu-reject").click()).not.toThrow();
    expect(() =>
      screen.getByTestId("menu-request-changes").click()
    ).not.toThrow();
    expect(() => screen.getByTestId("menu-resolved").click()).not.toThrow();
    expect(() => screen.getByTestId("menu-remove").click()).not.toThrow();
  });

  it("forwards groupInfo to context menu when provided", () => {
    const handlers: TaskNodeHandlers = { onViewDetails: vi.fn() };
    renderTaskNode(
      makeData({
        handlers,
        groupInfo: {
          groupKind: "plan",
          groupId: "g-1",
          groupLabel: "Group A",
          taskCount: 3,
          projectId: "p-1",
        },
      })
    );
    expect(
      screen.getByTestId("context-menu-wrapper").getAttribute("data-group-label")
    ).toBe("Group A");
  });

  it("renders step progress dots and percent when stepProgress is present", () => {
    vi.mocked(useStepProgress).mockReturnValueOnce({
      data: {
        total: 5,
        completed: 2,
        skipped: 1,
        failed: 1,
        inProgress: 1,
        pending: 0,
      },
    } as ReturnType<typeof useStepProgress>);

    const { container } = renderTaskNode(
      makeData({ internalStatus: "executing" })
    );

    const footer = screen.getByTestId("step-progress-footer");
    // 5 dots rendered
    expect(footer.querySelectorAll("div[style*='border-radius: 50%']").length)
      .toBe(5);
    // percent label: completed / (total - skipped) = 2 / 4 = 50%
    expect(container.textContent).toContain("50%");
  });

  it("colors completed step dots with success when in terminal state", () => {
    vi.mocked(useStepProgress).mockReturnValueOnce({
      data: {
        total: 2,
        completed: 2,
        skipped: 0,
        failed: 0,
        inProgress: 0,
        pending: 0,
      },
    } as ReturnType<typeof useStepProgress>);

    renderTaskNode(makeData({ internalStatus: "approved" }));
    const footer = screen.getByTestId("step-progress-footer");
    const dots = footer.querySelectorAll<HTMLDivElement>(
      "div[style*='border-radius: 50%']"
    );
    expect(dots.length).toBe(2);
    // first completed dot uses success color in terminal-complete state
    expect(dots[0]!.style.backgroundColor).toContain("status-success");
  });

  it("uses muted color for completed dots when not in terminal-complete state", () => {
    vi.mocked(useStepProgress).mockReturnValueOnce({
      data: {
        total: 4,
        completed: 1,
        skipped: 1,
        failed: 1,
        inProgress: 1,
        pending: 0,
      },
    } as ReturnType<typeof useStepProgress>);

    renderTaskNode(makeData({ internalStatus: "executing" }));
    const footer = screen.getByTestId("step-progress-footer");
    const dots = footer.querySelectorAll<HTMLDivElement>(
      "div[style*='border-radius: 50%']"
    );
    expect(dots[0]!.style.backgroundColor).toContain("text-muted"); // completed (not terminal)
    expect(dots[1]!.style.backgroundColor).toContain("text-muted"); // skipped
    expect(dots[2]!.style.backgroundColor).toContain("status-error"); // failed
    expect(dots[3]!.style.backgroundColor).toContain("accent-primary"); // in-progress
  });

  it("uses pending color for steps beyond active states", () => {
    vi.mocked(useStepProgress).mockReturnValueOnce({
      data: {
        total: 3,
        completed: 0,
        skipped: 0,
        failed: 0,
        inProgress: 0,
        pending: 3,
      },
    } as ReturnType<typeof useStepProgress>);

    renderTaskNode(makeData());
    const footer = screen.getByTestId("step-progress-footer");
    const dots = footer.querySelectorAll<HTMLDivElement>(
      "div[style*='border-radius: 50%']"
    );
    dots.forEach((dot) => {
      expect(dot.style.backgroundColor).toContain("border-subtle");
    });
  });

  it("does not render progress bar when total steps is 0", () => {
    vi.mocked(useStepProgress).mockReturnValueOnce({
      data: {
        total: 0,
        completed: 0,
        skipped: 0,
        failed: 0,
        inProgress: 0,
        pending: 0,
      },
    } as ReturnType<typeof useStepProgress>);

    const { container } = renderTaskNode(makeData());
    expect(container.textContent).not.toMatch(/\d+%/);
  });
});
