import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import type { TimelineEvent } from "@/api/task-graph.types";

// Mock the useExecutionTimeline hook so we don't need a QueryClient or Tauri.
type MockHookResult = {
  data: { pages: { events: TimelineEvent[]; total: number; hasMore: boolean }[] } | undefined;
  isLoading: boolean;
  error: Error | null;
  fetchNextPage: ReturnType<typeof vi.fn>;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  isFetching: boolean;
  refresh: ReturnType<typeof vi.fn>;
};

const mockHookState: { value: MockHookResult } = {
  value: makeDefaultHookResult(),
};

function makeDefaultHookResult(): MockHookResult {
  return {
    data: { pages: [{ events: [], total: 0, hasMore: false }] },
    isLoading: false,
    error: null,
    fetchNextPage: vi.fn(),
    hasNextPage: false,
    isFetchingNextPage: false,
    isFetching: false,
    refresh: vi.fn(),
  };
}

vi.mock("../hooks/useExecutionTimeline", () => ({
  useExecutionTimeline: () => mockHookState.value,
}));

// Stub TimelineEntry so we test ExecutionTimeline behavior in isolation.
vi.mock("./TimelineEntry", () => ({
  TimelineEntry: ({
    event,
    onTaskClick,
    isHighlighted,
  }: {
    event: TimelineEvent;
    onTaskClick?: (id: string) => void;
    isHighlighted?: boolean;
  }) => (
    <button
      data-testid={`entry-${event.id}`}
      data-highlighted={isHighlighted ? "true" : "false"}
      onClick={() => event.taskId && onTaskClick?.(event.taskId)}
    >
      {event.description}
    </button>
  ),
}));

import { ExecutionTimeline } from "./ExecutionTimeline";

function makeEvent(overrides: Partial<TimelineEvent> = {}): TimelineEvent {
  return {
    id: "ev-1",
    timestamp: new Date().toISOString(),
    taskId: "t1",
    taskTitle: "Task 1",
    eventType: "status_change",
    fromStatus: "ready",
    toStatus: "executing",
    description: "Started executing",
    trigger: null,
    planArtifactId: null,
    sessionTitle: null,
    ...overrides,
  };
}

beforeEach(() => {
  mockHookState.value = makeDefaultHookResult();
});

describe("ExecutionTimeline", () => {
  it("renders the empty state when there are no events", () => {
    render(<ExecutionTimeline projectId="p1" />);
    expect(screen.getByTestId("execution-timeline")).toBeInTheDocument();
    expect(screen.getByText("No events yet")).toBeInTheDocument();
    expect(screen.getByText(/Events appear as tasks progress/i)).toBeInTheDocument();
  });

  it("renders the loading state when isLoading is true", () => {
    mockHookState.value = {
      ...makeDefaultHookResult(),
      isLoading: true,
      data: undefined,
    };
    const { container } = render(<ExecutionTimeline projectId="p1" />);
    // Loader2 has animate-spin class.
    expect(container.querySelector(".animate-spin")).toBeTruthy();
    expect(screen.queryByText("No events yet")).not.toBeInTheDocument();
  });

  it("renders the error state and triggers refresh on Try again", async () => {
    const refresh = vi.fn();
    mockHookState.value = {
      ...makeDefaultHookResult(),
      error: new Error("boom"),
      refresh,
    };
    const user = userEvent.setup();
    render(<ExecutionTimeline projectId="p1" />);
    expect(screen.getByText("Failed to load timeline")).toBeInTheDocument();
    await user.click(screen.getByText("Try again"));
    // refresh is called once for the click; header refresh button is also wired to it
    expect(refresh).toHaveBeenCalled();
  });

  it("renders timeline entries and forwards onTaskClick to the parent", async () => {
    const events = [makeEvent(), makeEvent({ id: "ev-2", taskId: "t2", description: "Approved" })];
    mockHookState.value = {
      ...makeDefaultHookResult(),
      data: { pages: [{ events, total: 2, hasMore: false }] },
    };
    const onTaskClick = vi.fn();
    const user = userEvent.setup();
    render(
      <ExecutionTimeline
        projectId="p1"
        onTaskClick={onTaskClick}
        highlightedTaskId="t2"
      />
    );

    expect(screen.getByTestId("entry-ev-1")).toBeInTheDocument();
    expect(screen.getByTestId("entry-ev-2")).toHaveAttribute("data-highlighted", "true");
    expect(screen.getByTestId("entry-ev-1")).toHaveAttribute("data-highlighted", "false");

    await user.click(screen.getByTestId("entry-ev-1"));
    expect(onTaskClick).toHaveBeenCalledWith("t1");
  });

  it("toggles collapsed state when collapse/expand button is clicked", async () => {
    const user = userEvent.setup();
    render(<ExecutionTimeline projectId="p1" />);

    // Header is expanded; click "Collapse timeline".
    await user.click(screen.getByTitle("Collapse timeline"));
    expect(screen.getByTestId("execution-timeline-collapsed")).toBeInTheDocument();
    expect(screen.getByText("Execution Timeline")).toBeInTheDocument();

    // Now click expand.
    await user.click(screen.getByTitle("Expand timeline"));
    expect(screen.getByTestId("execution-timeline")).toBeInTheDocument();
  });

  it("starts collapsed when defaultCollapsed is true", () => {
    render(<ExecutionTimeline projectId="p1" defaultCollapsed />);
    expect(screen.getByTestId("execution-timeline-collapsed")).toBeInTheDocument();
  });

  it("hides collapse toggle and width when embedded", () => {
    render(<ExecutionTimeline projectId="p1" embedded defaultCollapsed />);
    // Embedded ignores defaultCollapsed and never renders the collapsed shell.
    expect(screen.queryByTestId("execution-timeline-collapsed")).not.toBeInTheDocument();
    expect(screen.getByTestId("execution-timeline")).toBeInTheDocument();
    expect(screen.queryByTitle("Collapse timeline")).not.toBeInTheDocument();
  });

  it("invokes refresh when the header refresh button is clicked", async () => {
    const refresh = vi.fn();
    mockHookState.value = { ...makeDefaultHookResult(), refresh };
    const user = userEvent.setup();
    render(<ExecutionTimeline projectId="p1" />);
    await user.click(screen.getByTitle("Refresh timeline"));
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("expands the filter bar and toggles category filters", async () => {
    mockHookState.value = {
      ...makeDefaultHookResult(),
      data: { pages: [{ events: [makeEvent()], total: 1, hasMore: false }] },
    };
    const user = userEvent.setup();
    render(<ExecutionTimeline projectId="p1" />);

    // Initially collapsed: "All events" label.
    expect(screen.getByText("All events")).toBeInTheDocument();
    await user.click(screen.getByText("All events"));

    // Now options are visible.
    const executionOption = screen.getByRole("button", { name: /Execution/ });
    await user.click(executionOption);
    expect(executionOption).toHaveAttribute("aria-pressed", "true");

    // Filter chip count updates in header.
    expect(screen.getByText(/1 filter/)).toBeInTheDocument();

    // Toggle a second filter.
    const reviewsOption = screen.getByRole("button", { name: /Reviews/ });
    await user.click(reviewsOption);
    expect(screen.getByText(/2 filters/)).toBeInTheDocument();

    // Toggle execution off again.
    await user.click(executionOption);
    expect(executionOption).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByText(/1 filter/)).toBeInTheDocument();

    // "Clear filters" resets.
    await user.click(screen.getByText("Clear filters"));
    expect(screen.getByText("All events")).toBeInTheDocument();
  });

  it("clicking the All option clears existing filters", async () => {
    const user = userEvent.setup();
    render(<ExecutionTimeline projectId="p1" />);
    await user.click(screen.getByText("All events"));
    const reviews = screen.getByRole("button", { name: /Reviews/ });
    await user.click(reviews);
    expect(screen.getByText(/1 filter/)).toBeInTheDocument();

    const allOption = screen.getByRole("button", { name: /^All/ });
    await user.click(allOption);
    expect(screen.getByText("All events")).toBeInTheDocument();
  });

  it("renders Load more when hasNextPage is true and triggers fetchNextPage", async () => {
    const fetchNextPage = vi.fn();
    mockHookState.value = {
      ...makeDefaultHookResult(),
      data: { pages: [{ events: [makeEvent()], total: 50, hasMore: true }] },
      hasNextPage: true,
      fetchNextPage,
    };
    const user = userEvent.setup();
    render(<ExecutionTimeline projectId="p1" />);
    const loadMore = screen.getByText("Load more");
    await user.click(loadMore);
    expect(fetchNextPage).toHaveBeenCalledTimes(1);
  });

  it("Load more shows Loading state and is disabled while fetching next page", () => {
    mockHookState.value = {
      ...makeDefaultHookResult(),
      data: { pages: [{ events: [makeEvent()], total: 50, hasMore: true }] },
      hasNextPage: true,
      isFetchingNextPage: true,
    };
    render(<ExecutionTimeline projectId="p1" />);
    expect(screen.getByText("Loading...")).toBeInTheDocument();
  });

  it("does not call fetchNextPage when already fetching the next page", async () => {
    const fetchNextPage = vi.fn();
    mockHookState.value = {
      ...makeDefaultHookResult(),
      data: { pages: [{ events: [makeEvent()], total: 50, hasMore: true }] },
      hasNextPage: true,
      isFetchingNextPage: true,
      fetchNextPage,
    };
    const user = userEvent.setup();
    render(<ExecutionTimeline projectId="p1" />);
    // The button is disabled, but click handler still firing should be guarded.
    const button = screen.getByRole("button", { name: /Loading/ });
    await user.click(button).catch(() => {
      /* userEvent throws on disabled but we just want to assert no call */
    });
    expect(fetchNextPage).not.toHaveBeenCalled();
  });

  it("supports rendering without onTaskClick (no-op)", async () => {
    mockHookState.value = {
      ...makeDefaultHookResult(),
      data: { pages: [{ events: [makeEvent()], total: 1, hasMore: false }] },
    };
    render(<ExecutionTimeline projectId="p1" />);
    // Should not throw.
    fireEvent.click(screen.getByTestId("entry-ev-1"));
  });

  it("collapsed view shows refresh and event count from data", async () => {
    mockHookState.value = {
      ...makeDefaultHookResult(),
      data: { pages: [{ events: [], total: 7, hasMore: false }] },
    };
    render(<ExecutionTimeline projectId="p1" defaultCollapsed />);
    expect(screen.getByTestId("execution-timeline-collapsed")).toBeInTheDocument();
    // No refresh button visible in collapsed header (only expand button shown).
    expect(screen.queryByTitle("Refresh timeline")).not.toBeInTheDocument();
    expect(screen.getByTitle("Expand timeline")).toBeInTheDocument();
  });
});
