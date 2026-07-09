/**
 * Tests for the Agents task detail state timeline.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { StateTransition } from "@/api/tasks";
import type { InternalStatus } from "@/types/task";

import { StateTimelineNav } from "./StateTimelineNav";

const mockUseTaskStateTransitions = vi.fn();
vi.mock("@/hooks/useTaskStateTransitions", () => ({
  useTaskStateTransitions: (...args: unknown[]) =>
    mockUseTaskStateTransitions(...args),
}));

function createQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
}

function renderWithProviders(ui: ReactElement) {
  return render(
    <QueryClientProvider client={createQueryClient()}>{ui}</QueryClientProvider>,
  );
}

function makeTransition(
  toStatus: InternalStatus,
  timestamp: string,
  extra: Partial<StateTransition> = {},
): StateTransition {
  return {
    fromStatus: null,
    toStatus,
    trigger: "system",
    timestamp,
    ...extra,
  };
}

const t0 = "2026-07-07T10:00:00Z";
const t1 = "2026-07-07T10:05:00Z";
const t2 = "2026-07-07T10:10:00Z";
const t3 = "2026-07-07T10:15:00Z";
const t4 = "2026-07-07T10:20:00Z";
const t5 = "2026-07-07T10:25:00Z";
const t6 = "2026-07-07T10:30:00Z";

describe("Agents StateTimelineNav", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders repeated execution, review, and merge runtime attempts as distinct stages", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, { conversationId: "exec-1" }),
        makeTransition("reviewing", t1, { conversationId: "review-1" }),
        makeTransition("revision_needed", t2),
        makeTransition("re_executing", t3, { conversationId: "exec-2" }),
        makeTransition("reviewing", t4, { conversationId: "review-2" }),
        makeTransition("pending_merge", t5),
        makeTransition("merged", t6, { conversationId: "merge-1" }),
      ],
      isLoading: false,
      error: null,
    });

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="merged"
        onStateSelect={vi.fn()}
      />,
    );

    expect(screen.getAllByTestId("timeline-badge-executing")).toHaveLength(1);
    expect(screen.getAllByTestId("timeline-badge-re_executing")).toHaveLength(1);
    expect(screen.getAllByTestId("timeline-badge-reviewing")).toHaveLength(2);
    expect(screen.getByTestId("timeline-badge-merged")).toHaveTextContent(
      "Merge attempt 1",
    );
  });

  it("keeps a normal merge flow on one merge attempt", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("merging", t0, { conversationId: "merge-1" }),
        makeTransition("waiting_on_pr", t1, { conversationId: "merge-1" }),
        makeTransition("merged", t2, { conversationId: "merge-1" }),
      ],
      isLoading: false,
      error: null,
    });

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="merged"
        onStateSelect={vi.fn()}
      />,
    );

    expect(screen.getByTestId("timeline-badge-merging")).toHaveTextContent(
      "Merge attempt 1",
    );
    expect(screen.getByTestId("timeline-badge-waiting_on_pr")).toHaveTextContent(
      "Merge attempt 1",
    );
    expect(screen.getByTestId("timeline-badge-merged")).toHaveTextContent(
      "Merge attempt 1",
    );
  });

  it("renders loading and error feedback for the history query", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: undefined,
      isLoading: true,
      error: null,
    });
    const { rerender } = renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="executing"
        onStateSelect={vi.fn()}
      />,
    );

    expect(screen.getByTestId("timeline-loading")).toHaveTextContent(
      "Loading history",
    );

    mockUseTaskStateTransitions.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: new Error("history failed"),
    });

    rerender(
      <QueryClientProvider client={createQueryClient()}>
        <StateTimelineNav
          taskId="task-1"
          currentStatus="executing"
          onStateSelect={vi.fn()}
        />
      </QueryClientProvider>,
    );

    expect(screen.getByTestId("timeline-error")).toHaveTextContent(
      "Failed to load history",
    );
  });

  it("hides empty transient history", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [],
      isLoading: false,
      error: null,
    });

    const { container } = renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="pending_merge"
        onStateSelect={vi.fn()}
      />,
    );

    expect(container.firstChild).toBeNull();
  });

  it("emits runtime stage metadata when a historical attempt is selected", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, {
          conversationId: "exec-1",
          agentRunId: "run-1",
        }),
        makeTransition("reviewing", t1, {
          conversationId: "review-1",
          agentRunId: "review-run-1",
        }),
        makeTransition("revision_needed", t2),
        makeTransition("re_executing", t3, {
          conversationId: "exec-2",
          agentRunId: "run-2",
        }),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="re_executing"
        onStateSelect={onStateSelect}
      />,
    );

    fireEvent.click(screen.getByTestId("timeline-badge-executing"));

    expect(onStateSelect).toHaveBeenCalledWith({
      status: "executing",
      timestamp: t0,
      conversationId: "exec-1",
      agentRunId: "run-1",
      contextType: "task_execution",
      attemptIndex: 1,
      transitionId: "executing-2026-07-07T10:00:00Z",
      hasConversation: true,
    });
  });

  it("respects explicit runtime context and stable transition ids", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, {
          conversationId: "merge-owned-conversation",
          contextType: "merge",
          transitionId: "transition-merge-owned-execution",
        }),
        makeTransition("approved", t1),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="approved"
        onStateSelect={onStateSelect}
      />,
    );

    const executingBadge = screen.getByTestId("timeline-badge-executing");
    expect(executingBadge).toHaveAttribute("data-context-type", "merge");
    expect(screen.getByTestId("timeline-badge-approved")).toHaveTextContent(
      "Approved",
    );

    fireEvent.click(executingBadge);

    expect(onStateSelect).toHaveBeenCalledWith({
      status: "executing",
      timestamp: t0,
      conversationId: "merge-owned-conversation",
      contextType: "merge",
      transitionId: "transition-merge-owned-execution",
      attemptIndex: 1,
      hasConversation: true,
    });
  });

  it("keeps non-runtime statuses as fallback labels without context metadata", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("backlog", t0),
        makeTransition("cancelled", t1),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="cancelled"
        onStateSelect={onStateSelect}
      />,
    );

    const backlogBadge = screen.getByTestId("timeline-badge-backlog");
    expect(backlogBadge).toHaveTextContent("Backlog");
    expect(backlogBadge).not.toHaveAttribute("data-context-type");
    expect(backlogBadge).not.toHaveAttribute("data-attempt-index");

    fireEvent.click(backlogBadge);

    expect(onStateSelect).toHaveBeenCalledWith({
      status: "backlog",
      timestamp: t0,
      transitionId: "backlog-2026-07-07T10:00:00Z",
      hasConversation: false,
    });
  });

  it("shows day-old relative timestamps in timeline tooltips", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-07T12:00:00Z"));
    try {
      mockUseTaskStateTransitions.mockReturnValue({
        data: [
          makeTransition("backlog", "2026-07-05T12:00:00Z"),
          makeTransition("cancelled", t1),
        ],
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="cancelled"
          onStateSelect={vi.fn()}
        />,
      );

      fireEvent.focus(screen.getByTestId("timeline-badge-backlog"));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(250);
      });

      expect(screen.getAllByText("2d ago").length).toBeGreaterThan(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("marks stages without a conversation as unavailable instead of borrowing another transcript", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, { conversationId: "exec-1" }),
        makeTransition("reviewing", t1),
        makeTransition("merged", t2),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="merged"
        onStateSelect={onStateSelect}
      />,
    );

    const reviewingBadge = screen.getByTestId("timeline-badge-reviewing");
    expect(reviewingBadge).toHaveAttribute("data-has-conversation", "false");

    fireEvent.click(reviewingBadge);

    expect(onStateSelect).toHaveBeenCalledWith({
      status: "reviewing",
      timestamp: t1,
      contextType: "review",
      attemptIndex: 1,
      transitionId: "reviewing-2026-07-07T10:05:00Z",
      hasConversation: false,
    });
  });

  it("adds the current stage when transition history is missing it", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, {
          conversationId: "exec-1",
        }),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="review_passed"
        onStateSelect={onStateSelect}
      />,
    );

    const currentBadge = screen.getByTestId("timeline-badge-review_passed");
    expect(currentBadge).toHaveAttribute("data-current", "true");
    expect(currentBadge).toHaveAttribute("data-context-type", "review");
    expect(currentBadge).toHaveAttribute("data-has-conversation", "false");
    expect(currentBadge).toHaveTextContent("Review Passed");

    fireEvent.click(currentBadge);

    expect(onStateSelect).toHaveBeenCalledWith(null);
  });

  it("keeps a transient stage when it is the current stage", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, {
          conversationId: "exec-1",
        }),
        makeTransition("pending_merge", t1),
      ],
      isLoading: false,
      error: null,
    });

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="pending_merge"
        onStateSelect={vi.fn()}
      />,
    );

    const currentBadge = screen.getByTestId("timeline-badge-pending_merge");
    expect(currentBadge).toHaveAttribute("data-current", "true");
    expect(currentBadge).toHaveAttribute("data-context-type", "merge");
    expect(currentBadge).toHaveTextContent("Pending Merge");
  });

  it("clears historical mode when the selected current stage is clicked", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, { conversationId: "exec-1" }),
        makeTransition("reviewing", t1, { conversationId: "review-1" }),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="reviewing"
        onStateSelect={onStateSelect}
        selectedState={{
          status: "reviewing",
          timestamp: t1,
          conversationId: "review-1",
          contextType: "review",
          attemptIndex: 1,
          hasConversation: true,
        }}
      />,
    );

    fireEvent.click(screen.getByTestId("timeline-badge-reviewing"));

    expect(onStateSelect).toHaveBeenCalledWith(null);
  });

  it("clears historical mode from the back-to-current control", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, { conversationId: "exec-1" }),
        makeTransition("reviewing", t1, { conversationId: "review-1" }),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <StateTimelineNav
        taskId="task-1"
        currentStatus="reviewing"
        onStateSelect={onStateSelect}
        selectedState={{
          status: "executing",
          timestamp: t0,
          conversationId: "exec-1",
          contextType: "task_execution",
          attemptIndex: 1,
          hasConversation: true,
        }}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Back to Current" }));

    expect(onStateSelect).toHaveBeenCalledWith(null);
  });
});
