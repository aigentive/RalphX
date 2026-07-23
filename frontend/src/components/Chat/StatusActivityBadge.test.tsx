/**
 * StatusActivityBadge tests
 *
 * Tests for the "Last activity X ago" indicator with progressive color coding,
 * "Tool active" display during active tool calls and grace period,
 * and "Verifying..." label during verification child sessions.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import { StatusActivityBadge } from "./StatusActivityBadge";

const allFeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  automationsPage: false,
  atlassianOauth: false,
};

let mockFeatureFlags = allFeatureFlags;

// ============================================================================
// Store mocks
// ============================================================================

const mockLastAgentEventTimestamp: Record<string, number> = {};
const mockToolCallStartTimes: Record<string, Record<string, number>> = {};
const mockLastToolCallCompletionTimestamp: Record<string, number> = {};

vi.mock("@/stores/chatStore", () => ({
  useChatStore: vi.fn(
    (
      selector: (state: {
        lastAgentEventTimestamp: Record<string, number>;
        toolCallStartTimes: Record<string, Record<string, number>>;
        lastToolCallCompletionTimestamp: Record<string, number>;
      }) => unknown
    ) => {
      return selector({
        lastAgentEventTimestamp: mockLastAgentEventTimestamp,
        toolCallStartTimes: mockToolCallStartTimes,
        lastToolCallCompletionTimestamp: mockLastToolCallCompletionTimestamp,
      });
    }
  ),
  selectToolCallStartTimes:
    (contextKey: string) =>
    (state: { toolCallStartTimes: Record<string, Record<string, number>> }) =>
      state.toolCallStartTimes[contextKey] ?? {},
  selectLastToolCallCompletionTimestamp:
    (contextKey: string) =>
    (state: { lastToolCallCompletionTimestamp: Record<string, number> }) =>
      state.lastToolCallCompletionTimestamp[contextKey] ?? 0,
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: vi.fn(
    (
      selector: (state: {
        setActivityFilter: () => void;
        setCurrentView: () => void;
      }) => unknown
    ) => selector({ setActivityFilter: vi.fn(), setCurrentView: vi.fn() })
  ),
}));

const mockActiveVerificationChildId: Record<string, string | null> = {};

vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: vi.fn(
    (
      selector: (state: {
        activeVerificationChildId: Record<string, string | null>;
      }) => unknown
    ) => {
      return selector({ activeVerificationChildId: mockActiveVerificationChildId });
    }
  ),
}));

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: vi.fn(() => ({ data: mockFeatureFlags })),
}));

// ============================================================================
// Helpers
// ============================================================================

const baseProps = {
  isAgentActive: true,
  agentType: "agent" as const,
  contextType: "task_execution" as const,
  contextId: "task-123",
  agentStatus: "generating" as const,
};

function setLastEvent(storeKey: string, msAgo: number) {
  mockLastAgentEventTimestamp[storeKey] = Date.now() - msAgo;
}

function setToolCallActive(storeKey: string, toolId: string, msAgo = 1_000) {
  if (!mockToolCallStartTimes[storeKey]) {
    mockToolCallStartTimes[storeKey] = {};
  }
  mockToolCallStartTimes[storeKey][toolId] = Date.now() - msAgo;
}

function clearToolCalls(storeKey: string) {
  delete mockToolCallStartTimes[storeKey];
}

function setLastToolCompletion(storeKey: string, msAgo: number) {
  mockLastToolCallCompletionTimestamp[storeKey] = Date.now() - msAgo;
}

function clearLastToolCompletion(storeKey: string) {
  delete mockLastToolCallCompletionTimestamp[storeKey];
}

// ============================================================================
// Tests
// ============================================================================

describe("StatusActivityBadge", () => {
  beforeEach(() => {
    mockFeatureFlags = allFeatureFlags;
    for (const key of Object.keys(mockLastAgentEventTimestamp)) {
      delete mockLastAgentEventTimestamp[key];
    }
    for (const key of Object.keys(mockToolCallStartTimes)) {
      delete mockToolCallStartTimes[key];
    }
    for (const key of Object.keys(mockLastToolCallCompletionTimestamp)) {
      delete mockLastToolCallCompletionTimestamp[key];
    }
    for (const key of Object.keys(mockActiveVerificationChildId)) {
      delete mockActiveVerificationChildId[key];
    }
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  // --------------------------------------------------------------------------
  // Existing behavior preserved
  // --------------------------------------------------------------------------

  it("renders without 'Last activity' when no storeKey provided", () => {
    render(<StatusActivityBadge {...baseProps} />);
    expect(screen.queryByText(/Last:/)).toBeNull();
  });

  it("renders without 'Last activity' when idle (not generating)", () => {
    render(
      <StatusActivityBadge
        {...baseProps}
        agentStatus="idle"
        isAgentActive={false}
        hasActivity={true}
        storeKey="task_execution:task-123"
      />
    );
    expect(screen.queryByText(/Last:/)).toBeNull();
  });

  it("renders without 'Last activity' when storeKey provided but timestamp is 0", () => {
    render(<StatusActivityBadge {...baseProps} storeKey="task_execution:task-123" />);
    expect(screen.queryByText(/Last:/)).toBeNull();
  });

  it("renders 'Last: Xs ago' with green color when activity is less than 1 minute ago", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 30_000); // 30 seconds ago

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    const lastActivity = screen.getByText(/Last: 30s ago/);
    expect(lastActivity).toBeDefined();
    expect(lastActivity.className).toContain("text-status-success");
  });

  it("renders 'Last: Xm ago' with yellow color at 1-3 minutes", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 90_000); // 90 seconds = 1m 30s ago

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    const lastActivity = screen.getByText(/Last: 1m 30s ago/);
    expect(lastActivity).toBeDefined();
    expect(lastActivity.className).toContain("text-status-warning");
  });

  it("renders with yellow color at exactly 1 minute (60 seconds)", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 60_000); // exactly 60 seconds ago

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    const lastActivity = screen.getByText(/Last: 1m ago/);
    expect(lastActivity).toBeDefined();
    expect(lastActivity.className).toContain("text-status-warning");
  });

  it("renders with red color at 3-5 minutes", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 240_000); // 240 seconds = 4 minutes ago

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    const lastActivity = screen.getByText(/Last: 4m ago/);
    expect(lastActivity).toBeDefined();
    expect(lastActivity.className).toContain("text-status-error");
  });

  it("renders with red color at exactly 3 minutes (180 seconds)", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 180_000); // exactly 180 seconds ago

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    const lastActivity = screen.getByText(/Last: 3m ago/);
    expect(lastActivity).toBeDefined();
    expect(lastActivity.className).toContain("text-status-error");
  });

  it("updates elapsed time via setInterval every second", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 30_000); // 30 seconds ago

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    expect(screen.getByText(/Last: 30s ago/)).toBeDefined();

    act(() => {
      mockLastAgentEventTimestamp[storeKey] -= 10_000;
      vi.advanceTimersByTime(10_000);
    });

    expect(screen.getByText(/Last: 40s ago/)).toBeDefined();
  });

  it("returns null when idle with no activity", () => {
    const { container } = render(
      <StatusActivityBadge
        {...baseProps}
        isAgentActive={false}
        agentStatus="idle"
        hasActivity={false}
      />
    );
    expect(container.firstChild).toBeNull();
  });

  it("hides the activity button when the activity page flag is disabled", () => {
    mockFeatureFlags = { ...allFeatureFlags, activityPage: false };

    render(<StatusActivityBadge {...baseProps} modelDisplay={{ id: "gpt-5.4", label: "gpt-5.4" }} />);

    expect(screen.queryByLabelText("View activity")).toBeNull();
    expect(screen.getByText("Agent responding...")).toBeInTheDocument();
    expect(screen.getByText("gpt-5.4")).toBeInTheDocument();
  });

  it("shows status badge text during generating state", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 5_000); // 5 seconds ago

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    expect(screen.getByText("Agent responding...")).toBeDefined();
  });

  it("shows worker text for AGENT_WORKER type", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 5_000);

    render(<StatusActivityBadge {...baseProps} agentType="worker" storeKey={storeKey} />);

    expect(screen.getByText("Worker running...")).toBeDefined();
  });

  // --------------------------------------------------------------------------
  // Tool active display
  // --------------------------------------------------------------------------

  it("shows 'Tool active' with green color when toolCallStartTimes is non-empty", () => {
    const storeKey = "task_execution:task-123";
    setToolCallActive(storeKey, "tool-1");

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    const label = screen.getByText("Tool active");
    expect(label).toBeDefined();
    expect(label.className).toContain("text-status-success");
  });

  it("hides 'Last: X ago' when tool calls are active", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 30_000);
    setToolCallActive(storeKey, "tool-1");

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    expect(screen.queryByText(/Last:/)).toBeNull();
    expect(screen.getByText("Tool active")).toBeDefined();
  });

  it("shows 'Tool active' with multiple concurrent tool calls (static label)", () => {
    const storeKey = "task_execution:task-123";
    setToolCallActive(storeKey, "tool-1");
    setToolCallActive(storeKey, "tool-2");

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    // Single "Tool active" label, not per-tool
    const labels = screen.getAllByText("Tool active");
    expect(labels).toHaveLength(1);
  });

  // --------------------------------------------------------------------------
  // Grace period visual
  // --------------------------------------------------------------------------

  it("shows 'Tool active' during grace period (within 5s of last completion)", () => {
    const storeKey = "task_execution:task-123";
    clearToolCalls(storeKey);
    setLastToolCompletion(storeKey, 2_000); // 2 seconds ago

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    const label = screen.getByText("Tool active");
    expect(label).toBeDefined();
    expect(label.className).toContain("text-status-success");
  });

  it("shows 'Last: X ago' when no tool calls active AND grace period expired (>5s)", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 30_000);
    clearToolCalls(storeKey);
    setLastToolCompletion(storeKey, 10_000); // 10 seconds ago — grace expired

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    expect(screen.queryByText("Tool active")).toBeNull();
    expect(screen.getByText(/Last: 30s ago/)).toBeDefined();
  });

  it("shows 'Last: X ago' when no tool calls and no lastToolCallCompletionTimestamp", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 45_000);
    clearToolCalls(storeKey);
    clearLastToolCompletion(storeKey);

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    expect(screen.queryByText("Tool active")).toBeNull();
    expect(screen.getByText(/Last: 45s ago/)).toBeDefined();
  });

  // --------------------------------------------------------------------------
  // Verification child display
  // --------------------------------------------------------------------------

  it("shows 'Verifying...' with blue color when activeVerificationChildId is set", () => {
    const storeKey = "session:session-abc";
    mockActiveVerificationChildId["session-abc"] = "child-session-123";

    render(
      <StatusActivityBadge
        {...baseProps}
        contextType="ideation"
        contextId="session-abc"
        storeKey={storeKey}
      />
    );

    const label = screen.getByText("Verifying...");
    expect(label).toBeDefined();
    expect(label.className).toContain("text-status-info");
  });

  it("hides 'Last: X ago' when verification child is active", () => {
    const storeKey = "session:session-abc";
    setLastEvent(storeKey, 30_000);
    mockActiveVerificationChildId["session-abc"] = "child-session-123";

    render(
      <StatusActivityBadge
        {...baseProps}
        contextType="ideation"
        contextId="session-abc"
        storeKey={storeKey}
      />
    );

    expect(screen.queryByText(/Last:/)).toBeNull();
    expect(screen.getByText("Verifying...")).toBeDefined();
  });

  it("shows normal generating state when no verification child is active", () => {
    const storeKey = "session:session-abc";
    // No activeVerificationChildId set

    render(
      <StatusActivityBadge
        {...baseProps}
        contextType="ideation"
        contextId="session-abc"
        storeKey={storeKey}
      />
    );

    expect(screen.queryByText("Verifying...")).toBeNull();
    expect(screen.getByText("Agent responding...")).toBeDefined();
  });

  it("prefers 'Tool active' over 'Verifying...' when both conditions are true", () => {
    // Tool active takes priority over verification child per display logic order
    const storeKey = "session:session-abc";
    setToolCallActive(storeKey, "tool-1");
    mockActiveVerificationChildId["session-abc"] = "child-session-123";

    render(
      <StatusActivityBadge
        {...baseProps}
        contextType="ideation"
        contextId="session-abc"
        storeKey={storeKey}
      />
    );

    expect(screen.getByText("Tool active")).toBeDefined();
    expect(screen.queryByText("Verifying...")).toBeNull();
  });

  it("does not show 'Verifying...' for non-session storeKeys", () => {
    // activeVerificationChildId only applies to session: storeKeys
    const storeKey = "task_execution:task-123";
    // Even if we set something in the ideation store, it won't match a non-session key
    mockActiveVerificationChildId["task-123"] = "child-session-123";

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    expect(screen.queryByText("Verifying...")).toBeNull();
    expect(screen.getByText("Agent responding...")).toBeDefined();
  });

  // --------------------------------------------------------------------------
  // Post-stream "Completing..." label
  // --------------------------------------------------------------------------

  it("shows 'Completing merge...' when generating with no events for >3s in merge context", () => {
    const storeKey = "merge:task-123";
    setLastEvent(storeKey, 4_000); // 4 seconds ago — past the 3s threshold

    render(
      <StatusActivityBadge
        {...baseProps}
        contextType="merge"
        contextId="task-123"
        storeKey={storeKey}
      />
    );

    expect(screen.getByText("Completing merge...")).toBeDefined();
  });

  it("shows 'Completing review...' when generating with no events for >3s in review context", () => {
    const storeKey = "review:task-123";
    setLastEvent(storeKey, 5_000); // 5 seconds ago

    render(
      <StatusActivityBadge
        {...baseProps}
        contextType="review"
        contextId="task-123"
        storeKey={storeKey}
      />
    );

    expect(screen.getByText("Completing review...")).toBeDefined();
  });

  it("does not show 'Completing...' when last event is within 3s for merge context", () => {
    const storeKey = "merge:task-123";
    setLastEvent(storeKey, 1_000); // 1 second ago — within threshold

    render(
      <StatusActivityBadge
        {...baseProps}
        contextType="merge"
        contextId="task-123"
        storeKey={storeKey}
      />
    );

    expect(screen.queryByText(/Completing/)).toBeNull();
    expect(screen.getByText("Agent responding...")).toBeDefined();
  });

  it("does not show 'Completing...' for task_execution context even after 3s", () => {
    const storeKey = "task_execution:task-123";
    setLastEvent(storeKey, 4_000); // 4 seconds ago

    render(<StatusActivityBadge {...baseProps} storeKey={storeKey} />);

    expect(screen.queryByText(/Completing/)).toBeNull();
    expect(screen.getByText("Agent responding...")).toBeDefined();
  });

  it("prefers 'Tool active' over 'Completing merge...' when tool calls are active", () => {
    const storeKey = "merge:task-123";
    setLastEvent(storeKey, 4_000);
    setToolCallActive(storeKey, "tool-1");

    render(
      <StatusActivityBadge
        {...baseProps}
        contextType="merge"
        contextId="task-123"
        storeKey={storeKey}
      />
    );

    expect(screen.getByText("Tool active")).toBeDefined();
    expect(screen.queryByText(/Completing/)).toBeNull();
  });

  it("transitions to 'Completing merge...' after 3s timer fires", () => {
    const storeKey = "merge:task-123";
    // Set event to "just now"
    mockLastAgentEventTimestamp[storeKey] = Date.now();

    render(
      <StatusActivityBadge
        {...baseProps}
        contextType="merge"
        contextId="task-123"
        storeKey={storeKey}
      />
    );

    // Before threshold: still shows normal label
    expect(screen.queryByText(/Completing/)).toBeNull();

    // Advance time past the 3s threshold
    act(() => {
      mockLastAgentEventTimestamp[storeKey] -= 3_500;
      vi.advanceTimersByTime(3_500);
    });

    expect(screen.getByText("Completing merge...")).toBeDefined();
  });

  // --------------------------------------------------------------------------
  // Inline layout (compact stacked headers)
  // --------------------------------------------------------------------------

  it("renders inline waiting layout with Awaiting input copy", () => {
    render(
      <StatusActivityBadge
        {...baseProps}
        agentStatus="waiting_for_input"
        layout="inline"
      />
    );
    expect(screen.getByText("Awaiting input")).toBeDefined();
  });

  it("renders inline generating layout with status testid", () => {
    const storeKey = "task_execution:task-inline";
    setLastEvent(storeKey, 500);
    render(
      <StatusActivityBadge
        {...baseProps}
        agentStatus="generating"
        storeKey={storeKey}
        layout="inline"
      />
    );
    expect(screen.getByTestId("chat-session-status-inline")).toBeDefined();
  });
});
