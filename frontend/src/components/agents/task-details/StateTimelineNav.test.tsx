/**
 * Tests for the Agents task detail state timeline.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
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
});
