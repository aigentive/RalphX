/**
 * Tests for StateTimelineNav component
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { StateTimelineNav } from "./StateTimelineNav";
import type { StateTransition } from "@/api/tasks";
import type { InternalStatus } from "@/types/task";

// Mock useTaskStateTransitions hook
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
    <QueryClientProvider client={createQueryClient()}>{ui}</QueryClientProvider>
  );
}

function makeTransition(
  toStatus: InternalStatus,
  timestamp: string,
  extra: Partial<StateTransition> = {}
): StateTransition {
  return {
    fromStatus: null,
    toStatus,
    trigger: "system",
    timestamp,
    ...extra,
  };
}

const t0 = "2026-05-06T10:00:00Z";
const t1 = "2026-05-06T10:05:00Z";
const t2 = "2026-05-06T10:10:00Z";
const t3 = "2026-05-06T10:15:00Z";

describe("StateTimelineNav", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("loading state", () => {
    it("renders loading spinner when isLoading is true", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: undefined,
        isLoading: true,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="executing"
          onStateSelect={vi.fn()}
        />
      );

      expect(screen.getByTestId("timeline-loading")).toBeInTheDocument();
      expect(screen.getByText("Loading history...")).toBeInTheDocument();
    });
  });

  describe("error state", () => {
    it("renders error UI when error is present", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: undefined,
        isLoading: false,
        error: new Error("network down"),
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="executing"
          onStateSelect={vi.fn()}
        />
      );

      expect(screen.getByTestId("timeline-error")).toBeInTheDocument();
      expect(screen.getByText("Failed to load history")).toBeInTheDocument();
    });
  });

  describe("hides when too few entries", () => {
    it("returns null when transitions empty and currentStatus is transient", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: [],
        isLoading: false,
        error: null,
      });

      const { container } = renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="ready"
          onStateSelect={vi.fn()}
        />
      );

      expect(container.firstChild).toBeNull();
    });

    it("returns null when only one synthetic entry exists (no history)", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: [],
        isLoading: false,
        error: null,
      });

      const { container } = renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="executing"
          onStateSelect={vi.fn()}
        />
      );

      // Only 1 entry → component returns null
      expect(container.firstChild).toBeNull();
    });

    it("returns null when undefined transitions and single state", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: undefined,
        isLoading: false,
        error: null,
      });

      const { container } = renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="executing"
          onStateSelect={vi.fn()}
        />
      );

      expect(container.firstChild).toBeNull();
    });
  });

  describe("rendered timeline", () => {
    it("renders multiple entries with badges and connectors", () => {
      const transitions: StateTransition[] = [
        makeTransition("backlog", t0),
        makeTransition("executing", t1, { conversationId: "conv-1" }),
        makeTransition("reviewing", t2),
      ];

      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-42"
          currentStatus="reviewing"
          onStateSelect={vi.fn()}
        />
      );

      expect(screen.getByTestId("state-timeline-nav")).toBeInTheDocument();
      expect(screen.getByTestId("timeline-badge-backlog")).toBeInTheDocument();
      expect(
        screen.getByTestId("timeline-badge-executing")
      ).toBeInTheDocument();
      expect(
        screen.getByTestId("timeline-badge-reviewing")
      ).toBeInTheDocument();
      expect(screen.getByText("History")).toBeInTheDocument();
      // Current status flag
      const reviewingBadge = screen.getByTestId("timeline-badge-reviewing");
      expect(reviewingBadge).toHaveAttribute("data-current", "true");
    });

    it("passes taskId to the hook", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: [],
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-xyz"
          currentStatus="ready"
          onStateSelect={vi.fn()}
        />
      );

      expect(mockUseTaskStateTransitions).toHaveBeenCalledWith("task-xyz");
    });

    it("filters transient states like 'ready' and 'pending_review' from history", () => {
      const transitions: StateTransition[] = [
        makeTransition("backlog", t0),
        makeTransition("ready", t1),
        makeTransition("executing", t2),
        makeTransition("pending_review", t3),
        makeTransition("reviewing", "2026-05-06T10:20:00Z"),
      ];

      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="reviewing"
          onStateSelect={vi.fn()}
        />
      );

      expect(screen.queryByTestId("timeline-badge-ready")).toBeNull();
      expect(
        screen.queryByTestId("timeline-badge-pending_review")
      ).toBeNull();
      expect(screen.getByTestId("timeline-badge-backlog")).toBeInTheDocument();
      expect(
        screen.getByTestId("timeline-badge-executing")
      ).toBeInTheDocument();
    });

    it("keeps a transient state when it is the current status", () => {
      const transitions: StateTransition[] = [
        makeTransition("backlog", t0),
        makeTransition("executing", t1),
        makeTransition("ready", t2),
      ];

      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="ready"
          onStateSelect={vi.fn()}
        />
      );

      expect(screen.getByTestId("timeline-badge-ready")).toBeInTheDocument();
    });

    it("hides intermediate retry states once task is terminal", () => {
      const transitions: StateTransition[] = [
        makeTransition("backlog", t0),
        makeTransition("executing", t1),
        makeTransition("qa_failed", t2),
        makeTransition("merged", t3),
      ];

      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="merged"
          onStateSelect={vi.fn()}
        />
      );

      // qa_failed is intermediate retry — hidden when terminal status reached
      expect(screen.queryByTestId("timeline-badge-qa_failed")).toBeNull();
      expect(screen.getByTestId("timeline-badge-merged")).toBeInTheDocument();
    });

    it("dedupes repeated statuses, keeping the latest occurrence", () => {
      const transitions: StateTransition[] = [
        makeTransition("executing", t0),
        makeTransition("revision_needed", t1),
        makeTransition("executing", t2),
      ];

      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="executing"
          onStateSelect={vi.fn()}
        />
      );

      const executingBadges = screen.getAllByTestId(
        "timeline-badge-executing"
      );
      expect(executingBadges).toHaveLength(1);
    });
  });

  describe("badge click behavior", () => {
    const transitions: StateTransition[] = [
      makeTransition("backlog", t0),
      makeTransition("executing", t1, {
        conversationId: "conv-1",
        agentRunId: "run-1",
      }),
      makeTransition("reviewing", t2),
    ];

    it("calls onStateSelect(null) when current-status badge is clicked", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });
      const onStateSelect = vi.fn();

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="reviewing"
          onStateSelect={onStateSelect}
        />
      );

      fireEvent.click(screen.getByTestId("timeline-badge-reviewing"));
      expect(onStateSelect).toHaveBeenCalledWith(null);
    });

    it("calls onStateSelect with entry payload when historical badge clicked", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });
      const onStateSelect = vi.fn();

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="reviewing"
          onStateSelect={onStateSelect}
        />
      );

      fireEvent.click(screen.getByTestId("timeline-badge-executing"));
      expect(onStateSelect).toHaveBeenCalledWith({
        status: "executing",
        timestamp: t1,
        conversationId: "conv-1",
        agentRunId: "run-1",
      });
    });
  });

  describe("selectedState UI", () => {
    const transitions: StateTransition[] = [
      makeTransition("backlog", t0),
      makeTransition("executing", t1),
      makeTransition("reviewing", t2),
    ];

    it("shows 'Viewing past state' indicator when selectedState is set", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="reviewing"
          onStateSelect={vi.fn()}
          selectedState={{ status: "executing", timestamp: t1 }}
        />
      );

      expect(screen.getByText("Viewing past state")).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: /Back to Current/i })
      ).toBeInTheDocument();
    });

    it("calls onStateSelect(null) when 'Back to Current' clicked", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });
      const onStateSelect = vi.fn();

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="reviewing"
          onStateSelect={onStateSelect}
          selectedState={{ status: "executing", timestamp: t1 }}
        />
      );

      fireEvent.click(
        screen.getByRole("button", { name: /Back to Current/i })
      );
      expect(onStateSelect).toHaveBeenCalledWith(null);
    });

    it("marks selected badge with data-selected=true", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="reviewing"
          onStateSelect={vi.fn()}
          selectedState={{ status: "executing", timestamp: t1 }}
        />
      );

      expect(
        screen.getByTestId("timeline-badge-executing")
      ).toHaveAttribute("data-selected", "true");
      expect(
        screen.getByTestId("timeline-badge-reviewing")
      ).toHaveAttribute("data-selected", "false");
    });

    it("does not show 'Viewing past state' when selectedState is null", () => {
      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="reviewing"
          onStateSelect={vi.fn()}
          selectedState={null}
        />
      );

      expect(screen.queryByText("Viewing past state")).toBeNull();
    });
  });

  describe("currentStatus normalization", () => {
    it("marks last entry's matching-status entry as current when timeline tail differs", () => {
      // currentStatus differs from last transition's toStatus
      const transitions: StateTransition[] = [
        makeTransition("executing", t0),
        makeTransition("reviewing", t1),
      ];

      mockUseTaskStateTransitions.mockReturnValue({
        data: transitions,
        isLoading: false,
        error: null,
      });

      renderWithProviders(
        <StateTimelineNav
          taskId="task-1"
          currentStatus="executing"
          onStateSelect={vi.fn()}
        />
      );

      // executing matches currentStatus → it should carry data-current=true
      expect(
        screen.getByTestId("timeline-badge-executing")
      ).toHaveAttribute("data-current", "true");
    });
  });
});
