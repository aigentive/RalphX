import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { useUiStore } from "@/stores/uiStore";
import { GraphSplitLayout } from "./GraphSplitLayout";

const { mockUseTaskChatAvailability } = vi.hoisted(() => ({
  mockUseTaskChatAvailability: vi.fn(() => false),
}));

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({ projectId }: { projectId: string }) => (
    <div data-testid="integrated-chat-panel">Task chat for {projectId}</div>
  ),
}));

vi.mock("@/components/tasks/TaskDetailOverlay", () => ({
  TaskDetailOverlay: ({
    projectId,
    constrainContent,
  }: {
    projectId: string;
    constrainContent?: boolean;
  }) => (
    <div data-testid="task-detail-overlay" data-constrained={String(Boolean(constrainContent))}>
      Task detail for {projectId}
    </div>
  ),
}));

vi.mock("@/components/tasks/TaskCreationOverlay", () => ({
  TaskCreationOverlay: () => <div data-testid="task-creation-overlay" />,
}));

vi.mock("@/hooks/useTaskChatAvailability", () => ({
  useTaskChatAvailability: mockUseTaskChatAvailability,
}));

function renderGraphSplitLayout(
  overrides: Partial<{
    rightPanelMode: "split" | "overlay" | "hidden";
    footer: React.ReactNode;
  }> = {}
) {
  return render(
    <GraphSplitLayout
      projectId="project-1"
      rightPanelMode={overrides.rightPanelMode ?? "split"}
      timelineContent={<div data-testid="timeline-content">Timeline</div>}
      footer={overrides.footer}
    >
      <div data-testid="graph-canvas">Graph</div>
    </GraphSplitLayout>
  );
}

describe("GraphSplitLayout", () => {
  beforeEach(() => {
    mockUseTaskChatAvailability.mockReturnValue(false);
    useUiStore.setState({
      selectedTaskId: null,
      taskCreationContext: null,
      taskHistoryState: null,
    });
    localStorage.clear();
  });

  it("shows the timeline instead of an empty chat pane for selected tasks without chat", () => {
    useUiStore.setState({ selectedTaskId: "task-1" });

    renderGraphSplitLayout();

    expect(screen.getByTestId("graph-split-layout").getAttribute("style")).toContain(
      "background-color: var(--app-content-bg)"
    );
    expect(screen.getByTestId("task-detail-overlay")).toBeInTheDocument();
    expect(screen.getByTestId("task-detail-overlay")).toHaveAttribute("data-constrained", "true");
    expect(screen.getByTestId("timeline-content")).toBeInTheDocument();
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
  });

  it("shows selected-task chat when an agent chat is available", () => {
    mockUseTaskChatAvailability.mockReturnValue(true);
    useUiStore.setState({ selectedTaskId: "task-1" });

    renderGraphSplitLayout();

    expect(screen.getByTestId("task-detail-overlay")).toHaveAttribute("data-constrained", "false");
    expect(screen.getByTestId("integrated-chat-panel")).toHaveTextContent("Task chat for project-1");
    expect(screen.queryByTestId("timeline-content")).not.toBeInTheDocument();
  });

  it("renders footer in the left section", () => {
    renderGraphSplitLayout({ footer: <div data-testid="footer-bar">Footer</div> });
    expect(screen.getByTestId("footer-bar")).toBeInTheDocument();
  });

  it("renders TaskCreationOverlay when taskCreationContext is set", () => {
    useUiStore.setState({
      taskCreationContext: { projectId: "project-1" },
    });
    renderGraphSplitLayout();
    expect(screen.getByTestId("task-creation-overlay")).toBeInTheDocument();
  });

  it("hides the right split section when rightPanelMode is hidden", () => {
    renderGraphSplitLayout({ rightPanelMode: "hidden" });
    expect(screen.queryByTestId("graph-split-right")).not.toBeInTheDocument();
    expect(screen.queryByTestId("graph-split-right-overlay")).not.toBeInTheDocument();
  });

  it("renders the overlay panel when rightPanelMode is overlay (timeline content)", () => {
    renderGraphSplitLayout({ rightPanelMode: "overlay" });
    expect(screen.getByTestId("graph-split-right-overlay")).toBeInTheDocument();
    expect(screen.getByTestId("timeline-content")).toBeInTheDocument();
  });

  it("renders chat inside overlay when chat is available", () => {
    mockUseTaskChatAvailability.mockReturnValue(true);
    useUiStore.setState({ selectedTaskId: "task-1" });
    renderGraphSplitLayout({ rightPanelMode: "overlay" });
    const overlay = screen.getByTestId("graph-split-right-overlay");
    expect(overlay).toBeInTheDocument();
    expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument();
  });

  it("transitions overlay from visible to exiting/hidden when rightPanelMode flips back to split", () => {
    vi.useFakeTimers();
    try {
      const { rerender } = render(
        <GraphSplitLayout
          projectId="project-1"
          rightPanelMode="overlay"
          timelineContent={<div data-testid="timeline-content">Timeline</div>}
        >
          <div data-testid="graph-canvas">Graph</div>
        </GraphSplitLayout>
      );
      expect(screen.getByTestId("graph-split-right-overlay")).toBeInTheDocument();

      rerender(
        <GraphSplitLayout
          projectId="project-1"
          rightPanelMode="split"
          timelineContent={<div data-testid="timeline-content">Timeline</div>}
        >
          <div data-testid="graph-canvas">Graph</div>
        </GraphSplitLayout>
      );
      // Overlay is now exiting but still mounted briefly
      expect(screen.getByTestId("graph-split-right-overlay").className).toContain(
        "graph-panel-exit"
      );

      act(() => {
        vi.advanceTimersByTime(250);
      });
      expect(screen.queryByTestId("graph-split-right-overlay")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("reads persisted chat panel width from localStorage on mount", () => {
    localStorage.setItem("ralphx-graph-chat-width", "450");
    mockUseTaskChatAvailability.mockReturnValue(true);
    useUiStore.setState({ selectedTaskId: "task-1" });
    renderGraphSplitLayout();
    const right = screen.getByTestId("graph-split-right");
    expect(right.getAttribute("style")).toContain("width: 450px");
  });

  it("ignores invalid persisted chat panel width", () => {
    localStorage.setItem("ralphx-graph-chat-width", "not-a-number");
    mockUseTaskChatAvailability.mockReturnValue(true);
    useUiStore.setState({ selectedTaskId: "task-1" });
    renderGraphSplitLayout();
    expect(screen.getByTestId("graph-split-right")).toBeInTheDocument();
  });

  it("ignores out-of-range persisted chat panel width", () => {
    localStorage.setItem("ralphx-graph-chat-width", "5"); // below MIN
    mockUseTaskChatAvailability.mockReturnValue(true);
    useUiStore.setState({ selectedTaskId: "task-1" });
    renderGraphSplitLayout();
    expect(screen.getByTestId("graph-split-right")).toBeInTheDocument();
  });

  it("renders ResizeHandle and updates chat panel width on drag", () => {
    mockUseTaskChatAvailability.mockReturnValue(true);
    useUiStore.setState({ selectedTaskId: "task-1" });
    renderGraphSplitLayout();

    const handle = screen.getByTestId("graph-split-resize-handle");
    // Mock the container's bounding rect
    const layout = screen.getByTestId("graph-split-layout");
    vi.spyOn(layout, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      bottom: 0,
      right: 1000,
      width: 1000,
      height: 600,
      toJSON: () => ({}),
    } as DOMRect);

    fireEvent.mouseDown(handle);
    act(() => {
      fireEvent.mouseMove(document, { clientX: 600 });
    });
    // 1000 - 600 = 400 (within MIN..MAX)
    expect(screen.getByTestId("graph-split-right").getAttribute("style")).toContain("width: 400px");

    act(() => {
      fireEvent.mouseUp(document);
    });
    // After mouseup, isResizing=false; further mousemoves should not change width
    act(() => {
      fireEvent.mouseMove(document, { clientX: 100 });
    });
    expect(screen.getByTestId("graph-split-right").getAttribute("style")).toContain("width: 400px");
  });

  it("clamps chat panel width to MAX (600) when dragged too wide", () => {
    mockUseTaskChatAvailability.mockReturnValue(true);
    useUiStore.setState({ selectedTaskId: "task-1" });
    renderGraphSplitLayout();
    const handle = screen.getByTestId("graph-split-resize-handle");
    const layout = screen.getByTestId("graph-split-layout");
    vi.spyOn(layout, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, top: 0, left: 0, bottom: 0, right: 1000, width: 1000, height: 600,
      toJSON: () => ({}),
    } as DOMRect);

    fireEvent.mouseDown(handle);
    act(() => {
      fireEvent.mouseMove(document, { clientX: 100 }); // would be 900 but clamped to 600
    });
    expect(screen.getByTestId("graph-split-right").getAttribute("style")).toContain("width: 600px");
    fireEvent.mouseUp(document);
  });

  it("renders SeparatorLine instead of ResizeHandle when chat is unavailable", () => {
    renderGraphSplitLayout();
    expect(screen.queryByTestId("graph-split-resize-handle")).not.toBeInTheDocument();
    expect(screen.getByTestId("graph-split-right").getAttribute("style")).toContain("width: 320px");
  });

  it("persists chat panel width changes to localStorage", () => {
    mockUseTaskChatAvailability.mockReturnValue(true);
    useUiStore.setState({ selectedTaskId: "task-1" });
    renderGraphSplitLayout();
    // Default width was written on mount
    expect(localStorage.getItem("ralphx-graph-chat-width")).toBeTruthy();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });
});
