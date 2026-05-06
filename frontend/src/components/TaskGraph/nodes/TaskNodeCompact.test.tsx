import React from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TaskNodeCompact } from "./TaskNodeCompact";
import type { TaskNodeData } from "./TaskNode";

vi.mock("./TaskNodeContextMenu", () => ({
  TaskNodeContextMenu: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

function renderCompact(overrides: Partial<TaskNodeData> = {}) {
  const data: TaskNodeData = {
    label: "Compact Task",
    taskId: "task-compact-1",
    internalStatus: "ready",
    priority: 2,
    isCriticalPath: false,
    description: "",
    category: null,
    ...overrides,
  } as TaskNodeData;
  const props = {
    id: data.taskId,
    type: "taskCompact",
    selected: false,
    data,
    isConnectable: true,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    zIndex: 0,
  } as React.ComponentProps<typeof TaskNodeCompact>;
  return render(
    <ReactFlowProvider>
      <TaskNodeCompact {...props} />
    </ReactFlowProvider>,
  );
}

describe("TaskNodeCompact", () => {
  it("renders the data-testid hook with taskId-driven attributes", () => {
    renderCompact();
    const node = screen.getByTestId("task-node-compact");
    expect(node.getAttribute("data-status")).toBe("ready");
    expect(node.getAttribute("data-critical-path")).toBe("false");
  });

  it("uses the scaled compact node width derived from useScaledNodeDimensions", () => {
    renderCompact();
    const node = screen.getByTestId("task-node-compact");
    const width = node.style.width;
    expect(width).toMatch(/\d+px/);
    expect(width).not.toBe("0px");
  });

  it("flags critical path tasks via data attribute", () => {
    renderCompact({ isCriticalPath: true });
    expect(screen.getByTestId("task-node-compact").getAttribute("data-critical-path")).toBe("true");
  });

  it("renders activity dots for active statuses", () => {
    const { container } = renderCompact({ internalStatus: "executing" });
    expect(container.querySelector('[data-status="executing"]')).toBeInTheDocument();
  });
});
