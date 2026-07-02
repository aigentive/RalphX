import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConversationRuntimeStatus } from "@/api/chat";
import { AgentRuntimeStatusWidget } from "./AgentRuntimeStatusWidget";

type RuntimeItem = AgentConversationRuntimeStatus["items"][number];

const scrollIntoViewMock = vi.fn();

function testRect(overrides: Partial<DOMRect> = {}): DOMRect {
  return {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
    top: 0,
    right: 0,
    bottom: 0,
    left: 0,
    toJSON: () => ({}),
    ...overrides,
  } as DOMRect;
}

function runtimeItem(overrides: Partial<RuntimeItem> = {}): RuntimeItem {
  return {
    source: "task_execution",
    contextType: "task_execution",
    contextId: "task-1",
    label: "Executing",
    title: "Runtime task",
    agentStatus: "generating",
    taskId: "task-1",
    internalStatus: "executing",
    runningProcess: null,
    ideationSession: null,
    parentSessionId: null,
    childSessionId: null,
    conversationId: null,
    ...overrides,
  };
}

function runtimeStatus(
  overrides: Partial<AgentConversationRuntimeStatus> = {},
): AgentConversationRuntimeStatus {
  return {
    conversationId: "conversation-1",
    isRunning: true,
    agentStatus: "generating",
    primarySource: "task_execution",
    summaryLabel: "Executing",
    items: [
      {
        source: "task_execution",
        contextType: "task_execution",
        contextId: "task-1",
        label: "Executing",
        title: "Runtime task",
        agentStatus: "generating",
        taskId: "task-1",
        internalStatus: "executing",
        runningProcess: null,
        ideationSession: null,
        parentSessionId: null,
        childSessionId: null,
        conversationId: null,
      },
    ],
    ...overrides,
  };
}

describe("AgentRuntimeStatusWidget", () => {
  beforeEach(() => {
    scrollIntoViewMock.mockReset();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value(this: HTMLElement, options?: boolean | ScrollIntoViewOptions) {
        scrollIntoViewMock(this, options);
      },
    });
  });

  it("does not render when the conversation has no active runtime", () => {
    const { container } = render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({ isRunning: false, items: [] })}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("does not render a single workspace runtime by default", () => {
    const { container } = render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "workspace",
          summaryLabel: "Agent running",
          items: [
            {
              source: "workspace",
              contextType: "project",
              contextId: "conversation-1",
              label: "Agent running",
              title: "Workspace chat",
              agentStatus: "generating",
              taskId: null,
              internalStatus: null,
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: "conversation-1",
            },
          ],
        })}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("renders a single workspace runtime when explicitly allowed", () => {
    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "workspace",
          summaryLabel: "Agent running",
          items: [
            {
              source: "workspace",
              contextType: "project",
              contextId: "conversation-1",
              label: "Agent running",
              title: "Workspace chat",
              agentStatus: "generating",
              taskId: null,
              internalStatus: null,
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: "conversation-1",
            },
          ],
        })}
        showSingleWorkspaceRuntime
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    expect(screen.getAllByText("Workspace chat")).toHaveLength(2);
  });

  it("renders active runtime status and routes task CTA", () => {
    const onViewTaskRuntime = vi.fn();

    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus()}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={onViewTaskRuntime}
      />,
    );

    expect(screen.getAllByText("Executing")).toHaveLength(2);
    expect(screen.getAllByText("Runtime task")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "View Task" }));

    expect(onViewTaskRuntime).toHaveBeenCalledWith("task-1", "task_execution");
  });

  it("routes workspace Review CTA to the review child conversation", () => {
    const onViewWorkspaceReview = vi.fn();

    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "workspace_review",
          summaryLabel: "Reviewing",
          items: [
            runtimeItem({
              source: "workspace_review",
              contextType: "project",
              contextId: "review-conversation-1",
              label: "Reviewing",
              title: "Review workspace changes",
              taskId: null,
              internalStatus: "reviewing",
              conversationId: "review-conversation-1",
            }),
          ],
        })}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
        onViewWorkspaceReview={onViewWorkspaceReview}
      />,
    );

    expect(screen.getAllByText("Review workspace changes")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "View Review" }));

    expect(onViewWorkspaceReview).toHaveBeenCalledWith("review-conversation-1");
  });

  it("keeps runtime target reveal local instead of calling ancestor scrollIntoView", () => {
    vi.useFakeTimers();
    try {
      render(
        <AgentRuntimeStatusWidget
          status={runtimeStatus({
            primarySource: "workspace_review",
            summaryLabel: "Reviewing",
            items: [
              runtimeItem({
                source: "workspace",
                contextType: "project",
                contextId: "conversation-1",
                label: "Agent running",
                title: "Workspace chat",
                agentStatus: "waiting_for_input",
                taskId: null,
                internalStatus: null,
                conversationId: "conversation-1",
              }),
              runtimeItem({
                source: "workspace_review",
                contextType: "project",
                contextId: "review-conversation-1",
                label: "Reviewing",
                title: "Review workspace changes",
                taskId: null,
                internalStatus: "reviewing",
                conversationId: "review-conversation-1",
              }),
            ],
          })}
          onViewWorkspace={vi.fn()}
          onViewIdeation={vi.fn()}
          onViewVerification={vi.fn()}
          onViewTaskRuntime={vi.fn()}
          onViewWorkspaceReview={vi.fn()}
        />,
      );

      act(() => {
        vi.runOnlyPendingTimers();
      });

      expect(scrollIntoViewMock).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not repeat runtime target reveal for semantically unchanged items", () => {
    vi.useFakeTimers();
    try {
      const firstStatus = runtimeStatus();
      const { rerender } = render(
        <AgentRuntimeStatusWidget
          status={firstStatus}
          onViewWorkspace={vi.fn()}
          onViewIdeation={vi.fn()}
          onViewVerification={vi.fn()}
          onViewTaskRuntime={vi.fn()}
        />,
      );

      act(() => {
        vi.runOnlyPendingTimers();
      });

      rerender(
        <AgentRuntimeStatusWidget
          status={runtimeStatus({ items: [...firstStatus.items] })}
          onViewWorkspace={vi.fn()}
          onViewIdeation={vi.fn()}
          onViewVerification={vi.fn()}
          onViewTaskRuntime={vi.fn()}
        />,
      );

      act(() => {
        vi.runOnlyPendingTimers();
      });

      expect(scrollIntoViewMock).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not duplicate the header for a single current workspace Review runtime", () => {
    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "workspace_review",
          summaryLabel: "Reviewing",
          items: [
            runtimeItem({
              source: "workspace_review",
              contextType: "project",
              contextId: "review-conversation-1",
              label: "Reviewing",
              title: "Review PR #521",
              taskId: null,
              internalStatus: "reviewing",
              conversationId: "review-conversation-1",
            }),
          ],
        })}
        currentFocus={{
          type: "workspace_review",
          conversationId: "review-conversation-1",
        }}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
        onViewWorkspaceReview={vi.fn()}
      />,
    );

    expect(screen.getAllByText("Reviewing")).toHaveLength(1);
    expect(screen.getAllByText("Review PR #521")).toHaveLength(1);
    expect(screen.queryByTestId("agents-runtime-status-icon")).not.toBeInTheDocument();
  });

  it("renders all waiting runtimes without active spinner presentation", () => {
    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          agentStatus: "waiting_for_input",
          primarySource: "workspace",
          summaryLabel: "Runtime waiting",
          items: [
            runtimeItem({
              source: "workspace",
              contextType: "project",
              contextId: "conversation-1",
              label: "Agent running",
              title: "Workspace chat",
              agentStatus: "waiting_for_input",
              taskId: null,
              internalStatus: null,
              conversationId: "conversation-1",
            }),
            runtimeItem({
              source: "review",
              contextType: "review",
              contextId: "task-2",
              label: "Reviewing",
              title: "Review task",
              agentStatus: "waiting_for_input",
              taskId: "task-2",
              internalStatus: "reviewing",
            }),
          ],
        })}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    expect(screen.getByText("Awaiting input")).toBeInTheDocument();
    expect(screen.getByText("2 waiting runtimes")).toBeInTheDocument();
    expect(screen.getAllByText("Waiting")).toHaveLength(2);
    expect(screen.getByTestId("agents-runtime-status-icon")).not.toHaveClass(
      "animate-spin",
    );
  });

  it("renders mixed waiting and generating rows with active presentation", () => {
    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "task_execution",
          summaryLabel: "Runtime activity",
          items: [
            runtimeItem({
              source: "workspace",
              contextType: "project",
              contextId: "conversation-1",
              label: "Agent running",
              title: "Workspace chat",
              agentStatus: "waiting_for_input",
              taskId: null,
              internalStatus: null,
              conversationId: "conversation-1",
            }),
            runtimeItem({
              source: "task_execution",
              contextType: "task_execution",
              contextId: "task-1",
              label: "Executing",
              title: "Runtime task",
              agentStatus: "generating",
              taskId: "task-1",
              internalStatus: "executing",
            }),
          ],
        })}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    expect(screen.getByText("Runtime activity")).toBeInTheDocument();
    expect(screen.getByText("2 active runtimes")).toBeInTheDocument();
    expect(screen.getByText("Running")).toBeInTheDocument();
    expect(screen.getByText("Waiting")).toBeInTheDocument();
    expect(screen.getByTestId("agents-runtime-status-icon")).toHaveClass(
      "animate-spin",
    );
  });

  it("matches composer tray width and caps the runtime list to three scrollable rows", () => {
    vi.useFakeTimers();
    try {
      render(
        <AgentRuntimeStatusWidget
          status={runtimeStatus({
            primarySource: "verification",
            summaryLabel: "Runtime activity",
            items: [
              runtimeItem({
                source: "workspace",
                contextType: "project",
                contextId: "conversation-1",
                label: "Workspace waiting",
                title: "Workspace chat",
                agentStatus: "waiting_for_input",
                taskId: null,
                internalStatus: null,
                conversationId: "conversation-1",
              }),
              runtimeItem({
                source: "review",
                contextType: "review",
                contextId: "task-2",
                label: "Review waiting",
                title: "Review task",
                agentStatus: "waiting_for_input",
                taskId: "task-2",
                internalStatus: "reviewing",
              }),
              runtimeItem({
                source: "verification",
                contextType: "ideation",
                contextId: "verification-child",
                label: "Verifying",
                title: "Verification run",
                parentSessionId: "parent-session",
                childSessionId: "verification-child",
              }),
              runtimeItem({
                source: "task_execution",
                contextType: "task_execution",
                contextId: "task-4",
                label: "Executing",
                title: "Execution task",
                taskId: "task-4",
              }),
            ],
          })}
          onViewWorkspace={vi.fn()}
          onViewIdeation={vi.fn()}
          onViewVerification={vi.fn()}
          onViewTaskRuntime={vi.fn()}
        />,
      );

      expect(screen.getByTestId("agents-runtime-status-widget")).toHaveClass(
        "mx-1",
        "mb-1.5",
      );
      const list = screen.getByTestId("agents-runtime-status-list");
      expect(list).toHaveClass("overflow-y-auto", "overscroll-contain");
      expect(list).toHaveStyle({ maxHeight: "108px" });

      vi.spyOn(list, "getBoundingClientRect").mockReturnValue(
        testRect({ top: 0, bottom: 108 }),
      );
      vi.spyOn(
        screen.getByTestId("agents-runtime-status-item-verification"),
        "getBoundingClientRect",
      ).mockReturnValue(testRect({ top: 120, bottom: 152 }));

      act(() => {
        vi.runOnlyPendingTimers();
      });

      expect(list.scrollTop).toBe(44);
      expect(scrollIntoViewMock).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("scrolls the selected task runtime row before the first running row", () => {
    vi.useFakeTimers();
    try {
      render(
        <AgentRuntimeStatusWidget
          status={runtimeStatus({
            primarySource: "verification",
            summaryLabel: "Runtime activity",
            items: [
              runtimeItem({
                source: "verification",
                contextType: "ideation",
                contextId: "verification-child",
                label: "Verifying",
                title: "Verification run",
                parentSessionId: "parent-session",
                childSessionId: "verification-child",
              }),
              runtimeItem({
                source: "review",
                contextType: "review",
                contextId: "task-2",
                label: "Reviewing",
                title: "Review task",
                taskId: "task-2",
                internalStatus: "reviewing",
                agentStatus: "waiting_for_input",
              }),
            ],
          })}
          selectedTaskId="task-2"
          onViewWorkspace={vi.fn()}
          onViewIdeation={vi.fn()}
          onViewVerification={vi.fn()}
          onViewTaskRuntime={vi.fn()}
        />,
      );

      const list = screen.getByTestId("agents-runtime-status-list");
      vi.spyOn(list, "getBoundingClientRect").mockReturnValue(
        testRect({ top: 0, bottom: 32 }),
      );
      vi.spyOn(
        screen.getByTestId("agents-runtime-status-item-review"),
        "getBoundingClientRect",
      ).mockReturnValue(testRect({ top: 42, bottom: 74 }));

      act(() => {
        vi.runOnlyPendingTimers();
      });

      expect(list.scrollTop).toBe(42);
      expect(scrollIntoViewMock).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("routes verification CTA with parent and child session ids", () => {
    const onViewVerification = vi.fn();

    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "verification",
          summaryLabel: "Verifying",
          items: [
            {
              source: "verification",
              contextType: "ideation",
              contextId: "child-session",
              label: "Verifying",
              title: "Verification run",
              agentStatus: "generating",
              taskId: null,
              internalStatus: null,
              runningProcess: null,
              ideationSession: null,
              parentSessionId: "parent-session",
              childSessionId: "child-session",
              conversationId: null,
            },
          ],
        })}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={onViewVerification}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "View Verification" }));

    expect(onViewVerification).toHaveBeenCalledWith(
      "parent-session",
      "child-session",
    );
  });

  it("routes ideation, review, merge, and workspace CTA variants", () => {
    const onViewWorkspace = vi.fn();
    const onViewIdeation = vi.fn();
    const onViewTaskRuntime = vi.fn();

    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "merge",
          summaryLabel: "Merging tasks",
          items: [
            {
              source: "ideation",
              contextType: "ideation",
              contextId: "session-1",
              label: "Ideation running",
              title: "Plan chat",
              agentStatus: "generating",
              taskId: null,
              internalStatus: null,
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: null,
            },
            {
              source: "review",
              contextType: "review",
              contextId: "task-2",
              label: "Reviewing",
              title: "Review task",
              agentStatus: "generating",
              taskId: "task-2",
              internalStatus: "reviewing",
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: null,
            },
            {
              source: "merge",
              contextType: "merge",
              contextId: "task-3",
              label: "Merging",
              title: "Merge task",
              agentStatus: "generating",
              taskId: "task-3",
              internalStatus: "pending_merge",
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: null,
            },
            {
              source: "workspace",
              contextType: "project",
              contextId: "conversation-1",
              label: "Agent running",
              title: "Workspace chat",
              agentStatus: "waiting_for_input",
              taskId: null,
              internalStatus: null,
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: "conversation-1",
            },
          ],
        })}
        onViewWorkspace={onViewWorkspace}
        onViewIdeation={onViewIdeation}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={onViewTaskRuntime}
      />,
    );

    expect(screen.getByText("4 active runtimes")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "View Ideation" }));
    fireEvent.click(screen.getAllByRole("button", { name: "View Task" })[0]);
    fireEvent.click(screen.getAllByRole("button", { name: "View Task" })[1]);
    fireEvent.click(screen.getByRole("button", { name: "View Workspace" }));

    expect(onViewIdeation).toHaveBeenCalledWith("session-1");
    expect(onViewTaskRuntime).toHaveBeenNthCalledWith(1, "task-2", "review");
    expect(onViewTaskRuntime).toHaveBeenNthCalledWith(2, "task-3", "merge");
    expect(onViewWorkspace).toHaveBeenCalledTimes(1);
  });

  it("shows a viewing indicator instead of a CTA for the current focus row", () => {
    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "ideation",
          summaryLabel: "Runtime activity",
          items: [
            runtimeItem({
              source: "workspace",
              contextType: "project",
              contextId: "conversation-1",
              label: "Agent running",
              title: "Workspace chat",
              agentStatus: "waiting_for_input",
              taskId: null,
              internalStatus: null,
              conversationId: "conversation-1",
            }),
            runtimeItem({
              source: "ideation",
              contextType: "ideation",
              contextId: "session-1",
              label: "Ideation running",
              title: "Plan chat",
              taskId: null,
              internalStatus: null,
            }),
          ],
        })}
        currentFocus={{ type: "workspace" }}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    expect(
      screen.getByTestId("agents-runtime-status-current-workspace"),
    ).toHaveAttribute("aria-label", "Currently viewing Workspace chat");
    expect(
      screen.queryByRole("button", { name: "View Workspace" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "View Ideation" }),
    ).toBeInTheDocument();
  });

  it("matches ideation, verification, task, and workspace Review focus rows", () => {
    const baseStatus = runtimeStatus({
      primarySource: "verification",
      summaryLabel: "Runtime activity",
      items: [
        runtimeItem({
          source: "ideation",
          contextType: "ideation",
          contextId: "session-1",
          label: "Ideation running",
          title: "Plan chat",
          taskId: null,
          internalStatus: null,
        }),
        runtimeItem({
          source: "verification",
          contextType: "ideation",
          contextId: "verification-child",
          label: "Verifying",
          title: "Verification run",
          taskId: null,
          internalStatus: null,
          parentSessionId: "parent-session",
          childSessionId: "verification-child",
        }),
        runtimeItem({
          source: "review",
          contextType: "review",
          contextId: "task-2",
          label: "Reviewing",
          title: "Review task",
          taskId: "task-2",
          internalStatus: "reviewing",
        }),
        runtimeItem({
          source: "workspace_review",
          contextType: "project",
          contextId: "review-conversation-1",
          label: "Reviewing",
          title: "Review workspace changes",
          taskId: null,
          internalStatus: "reviewing",
          conversationId: "review-conversation-1",
        }),
      ],
    });
    const defaultHandlers = {
      onViewWorkspace: vi.fn(),
      onViewIdeation: vi.fn(),
      onViewVerification: vi.fn(),
      onViewTaskRuntime: vi.fn(),
      onViewWorkspaceReview: vi.fn(),
    };

    const { rerender } = render(
      <AgentRuntimeStatusWidget
        status={baseStatus}
        currentFocus={{ type: "ideation", sessionId: "session-1" }}
        {...defaultHandlers}
      />,
    );

    expect(
      screen.getByTestId("agents-runtime-status-current-ideation"),
    ).toHaveAttribute("aria-label", "Currently viewing Plan chat");

    rerender(
      <AgentRuntimeStatusWidget
        status={baseStatus}
        currentFocus={{
          type: "verification",
          parentSessionId: "parent-session",
          childSessionId: "verification-child",
        }}
        {...defaultHandlers}
      />,
    );

    expect(
      screen.getByTestId("agents-runtime-status-current-verification"),
    ).toHaveAttribute("aria-label", "Currently viewing Verification run");

    rerender(
      <AgentRuntimeStatusWidget
        status={baseStatus}
        currentFocus={{
          type: "task_runtime",
          taskId: "task-2",
          contextType: "review",
        }}
        {...defaultHandlers}
      />,
    );

    expect(
      screen.getByTestId("agents-runtime-status-current-review"),
    ).toHaveAttribute("aria-label", "Currently viewing Review task");

    rerender(
      <AgentRuntimeStatusWidget
        status={baseStatus}
        currentFocus={{
          type: "workspace_review",
          conversationId: "review-conversation-1",
        }}
        {...defaultHandlers}
      />,
    );

    expect(
      screen.getByTestId("agents-runtime-status-current-workspace_review"),
    ).toHaveAttribute("aria-label", "Currently viewing Review workspace changes");
  });

  it("does not scroll when no runtime row is running or selected", async () => {
    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "workspace",
          summaryLabel: "Runtime waiting",
          items: [
            runtimeItem({
              source: "workspace",
              contextType: "project",
              contextId: "conversation-1",
              label: "Workspace waiting",
              title: "Workspace chat",
              agentStatus: "waiting_for_input",
              taskId: null,
              internalStatus: null,
              conversationId: "conversation-1",
            }),
            runtimeItem({
              source: "review",
              contextType: "review",
              contextId: "task-2",
              label: "Review waiting",
              title: "Review task",
              agentStatus: "waiting_for_input",
              taskId: "task-2",
              internalStatus: "reviewing",
            }),
          ],
        })}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    await new Promise((resolve) => window.setTimeout(resolve, 0));

    expect(scrollIntoViewMock).not.toHaveBeenCalled();
  });
});
