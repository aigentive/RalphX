import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useUiStore } from "@/stores/uiStore";
import { GraphSplitLayout } from "./GraphSplitLayout";

vi.mock("@/components/tasks/TaskCreationOverlay", () => ({
  TaskCreationOverlay: () => <div data-testid="task-creation-overlay" />,
}));

function renderLayout(
  rightPanelMode: "split" | "overlay" | "hidden" = "split",
) {
  return render(
    <GraphSplitLayout
      projectId="project-1"
      rightPanelMode={rightPanelMode}
      timelineContent={<div data-testid="timeline-content">Timeline</div>}
    >
      <div data-testid="graph-canvas">Graph</div>
    </GraphSplitLayout>,
  );
}

describe("GraphSplitLayout", () => {
  beforeEach(() => {
    useUiStore.setState({ taskCreationContext: null });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps Graph and timeline live without mounting the retired task detail/chat branch", () => {
    renderLayout();

    expect(screen.getByTestId("graph-canvas")).toBeInTheDocument();
    expect(screen.getByTestId("timeline-content")).toBeInTheDocument();
    expect(screen.queryByTestId("task-detail-overlay")).not.toBeInTheDocument();
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
  });

  it("keeps task creation host-owned", () => {
    useUiStore.setState({ taskCreationContext: { projectId: "project-1" } });
    renderLayout();

    expect(screen.getByTestId("task-creation-overlay")).toBeInTheDocument();
  });

  it("renders the timeline in the compact overlay mode", () => {
    renderLayout("overlay");
    expect(screen.getByTestId("graph-split-right-overlay")).toBeInTheDocument();
    expect(screen.getByTestId("timeline-content")).toBeInTheDocument();
  });

  it("defers overlay teardown after returning to split mode", () => {
    vi.useFakeTimers();
    const { rerender } = renderLayout("overlay");

    rerender(
      <GraphSplitLayout
        projectId="project-1"
        rightPanelMode="split"
        timelineContent={<div data-testid="timeline-content">Timeline</div>}
      >
        <div data-testid="graph-canvas">Graph</div>
      </GraphSplitLayout>,
    );
    expect(screen.getByTestId("graph-split-right-overlay")).toBeInTheDocument();

    act(() => vi.advanceTimersByTime(250));
    expect(screen.queryByTestId("graph-split-right-overlay")).not.toBeInTheDocument();
  });
});
