import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { StateTransition } from "@/api/tasks";
import type { InternalStatus } from "@/types/task";

import { TaskHistoryDropdown } from "./TaskHistoryDropdown";

const mockUseTaskStateTransitions = vi.fn();
vi.mock("@/hooks/useTaskStateTransitions", () => ({
  useTaskStateTransitions: (...args: unknown[]) => mockUseTaskStateTransitions(...args),
}));

const t0 = "2026-07-07T10:00:00Z";
const t1 = "2026-07-07T10:05:00Z";
const t2 = "2026-07-07T10:10:00Z";

function makeTransition(
  toStatus: InternalStatus,
  timestamp: string,
  extra: Partial<StateTransition> = {},
): StateTransition {
  return { fromStatus: null, toStatus, trigger: "system", timestamp, ...extra };
}

function renderWithProviders(ui: ReactElement) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      {ui}
    </QueryClientProvider>,
  );
}

function openHistoryMenu() {
  fireEvent.pointerDown(screen.getByTestId("task-history-dropdown-trigger"), {
    button: 0,
    ctrlKey: false,
  });
}

describe("TaskHistoryDropdown", () => {
  beforeEach(() => vi.clearAllMocks());

  it("lists Current first and historical stages newest-first", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, { conversationId: "execution-1" }),
        makeTransition("reviewing", t1, { conversationId: "review-1" }),
        makeTransition("merged", t2, { conversationId: "merge-1" }),
      ],
      isLoading: false,
      error: null,
    });

    renderWithProviders(
      <TaskHistoryDropdown taskId="task-1" currentStatus="merged" onStateSelect={vi.fn()} />,
    );
    openHistoryMenu();

    expect(screen.getAllByRole("menuitemradio").map((item) => item.textContent)).toEqual([
      expect.stringContaining("Current — Merge attempt 1"),
      expect.stringContaining("Review attempt 1"),
      expect.stringContaining("Execution attempt 1"),
    ]);
  });

  it("selects a historical entry with its complete runtime metadata", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, { conversationId: "execution-1", agentRunId: "run-1" }),
        makeTransition("reviewing", t1, { conversationId: "review-1", agentRunId: "review-run-1" }),
        makeTransition("merged", t2, { conversationId: "merge-1" }),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <TaskHistoryDropdown taskId="task-1" currentStatus="merged" onStateSelect={onStateSelect} />,
    );
    openHistoryMenu();
    fireEvent.click(screen.getByTestId(`task-history-dropdown-item-reviewing-${t1}`));

    expect(onStateSelect).toHaveBeenCalledWith({
      status: "reviewing",
      timestamp: t1,
      conversationId: "review-1",
      agentRunId: "review-run-1",
      contextType: "review",
      transitionId: `reviewing-${t1}`,
      attemptIndex: 1,
      hasConversation: true,
    });
  });

  it("keeps no-conversation stages selectable without inventing transcript metadata", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, { conversationId: "execution-1" }),
        makeTransition("reviewing", t1),
        makeTransition("merged", t2, { conversationId: "merge-1" }),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <TaskHistoryDropdown taskId="task-1" currentStatus="merged" onStateSelect={onStateSelect} />,
    );
    openHistoryMenu();
    fireEvent.click(screen.getByTestId(`task-history-dropdown-item-reviewing-${t1}`));

    expect(onStateSelect).toHaveBeenCalledWith({
      status: "reviewing",
      timestamp: t1,
      contextType: "review",
      transitionId: `reviewing-${t1}`,
      attemptIndex: 1,
      hasConversation: false,
    });
  });

  it("marks the selected history entry and returns to Current", () => {
    mockUseTaskStateTransitions.mockReturnValue({
      data: [
        makeTransition("executing", t0, { conversationId: "execution-1" }),
        makeTransition("reviewing", t1, { conversationId: "review-1" }),
      ],
      isLoading: false,
      error: null,
    });
    const onStateSelect = vi.fn();

    renderWithProviders(
      <TaskHistoryDropdown
        taskId="task-1"
        currentStatus="reviewing"
        onStateSelect={onStateSelect}
        selectedState={{
          status: "executing",
          timestamp: t0,
          conversationId: "execution-1",
          contextType: "task_execution",
          transitionId: `executing-${t0}`,
          attemptIndex: 1,
          hasConversation: true,
        }}
      />,
    );

    expect(screen.getByTestId("task-history-dropdown-trigger")).toHaveTextContent("Execution attempt 1");
    openHistoryMenu();
    expect(screen.getByTestId(`task-history-dropdown-item-executing-${t0}`)).toHaveAttribute(
      "data-state",
      "checked",
    );
    fireEvent.click(screen.getByRole("menuitemradio", { name: /Current — Review attempt 1/i }));

    expect(onStateSelect).toHaveBeenCalledWith(null);
  });

  it("renders relative timestamps for recent history entries", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-07T10:20:00Z"));
    try {
      mockUseTaskStateTransitions.mockReturnValue({
        data: [
          makeTransition("executing", "2026-07-07T09:20:00Z"),
          makeTransition("reviewing", "2026-07-07T10:19:30Z"),
          makeTransition("review_passed", "2026-07-07T10:19:00Z"),
          makeTransition("merged", "2026-07-07T10:20:00Z"),
        ],
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <TaskHistoryDropdown taskId="task-1" currentStatus="merged" onStateSelect={vi.fn()} />,
      );
      openHistoryMenu();

      expect(screen.getByTestId("task-history-dropdown-content")).toHaveTextContent("Just now");
      expect(screen.getByTestId("task-history-dropdown-content")).toHaveTextContent("1m ago");
      expect(screen.getByTestId("task-history-dropdown-content")).toHaveTextContent("1h ago");
    } finally {
      vi.useRealTimers();
    }
  });

  it("renders compact loading and error feedback and hides a single entry", () => {
    mockUseTaskStateTransitions.mockReturnValue({ data: undefined, isLoading: true, error: null });
    const { rerender } = renderWithProviders(
      <TaskHistoryDropdown taskId="task-1" currentStatus="executing" onStateSelect={vi.fn()} />,
    );
    expect(screen.getByTestId("task-history-loading")).toHaveTextContent("Loading history");

    mockUseTaskStateTransitions.mockReturnValue({ data: undefined, isLoading: false, error: new Error("nope") });
    rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <TaskHistoryDropdown taskId="task-1" currentStatus="executing" onStateSelect={vi.fn()} />
      </QueryClientProvider>,
    );
    expect(screen.getByTestId("task-history-error")).toHaveTextContent("Failed to load history");

    mockUseTaskStateTransitions.mockReturnValue({
      data: [makeTransition("executing", t0)],
      isLoading: false,
      error: null,
    });
    rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <TaskHistoryDropdown taskId="task-1" currentStatus="executing" onStateSelect={vi.fn()} />
      </QueryClientProvider>,
    );
    expect(screen.queryByTestId("task-history-dropdown")).not.toBeInTheDocument();
  });
});
