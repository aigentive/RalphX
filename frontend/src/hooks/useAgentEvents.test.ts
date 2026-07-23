/**
 * useAgentEvents — child termination reverse lookup tests
 *
 * PO6: child session crash/termination → reverse lookup clears parent's
 *      activeVerificationChildId and sets parent to idle. Multi-parent isolation
 *      ensures unrelated parents are not affected.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

// ============================================================================
// Hoisted mocks
// ============================================================================

const chatStoreMocks = vi.hoisted(() => ({
  setAgentStatus: vi.fn(),
  agentStatus: {} as Record<string, string>,
  activeAgentRunIds: {} as Record<string, string>,
  activeConversationIds: {} as Record<string, string | null>,
  lastAgentEventTimestamp: {} as Record<string, number>,
  toolCallStartTimes: {} as Record<string, Record<string, number>>,
  lastToolCallCompletionTimestamp: {} as Record<string, number>,
  clearToolCallStartTimes: vi.fn(),
  updateLastAgentEvent: vi.fn(),
  deleteQueuedMessage: vi.fn(),
  queueMessage: vi.fn(),
  setActiveConversation: vi.fn(),
  setActiveAgentRun: vi.fn(),
  clearActiveAgentRun: vi.fn(),
}));

vi.mock("@/stores/chatStore", () => ({
  useChatStore: Object.assign(
    vi.fn((selector: (s: typeof chatStoreMocks) => unknown) => selector(chatStoreMocks)),
    { getState: () => chatStoreMocks }
  ),
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: Object.assign(vi.fn((selector: (s: { clearActiveQuestion: () => void }) => unknown) =>
    selector({ clearActiveQuestion: vi.fn() })), {
    getState: () => ({ clearActiveQuestion: vi.fn() }),
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

// Capture event bus subscriptions so tests can fire events manually
const subscriptions = new Map<string, ((...args: unknown[]) => void)[]>();

function fireEvent<T>(event: string, payload: T) {
  const handlers = subscriptions.get(event);
  if (handlers) for (const h of handlers) h(payload);
}

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (...args: unknown[]) => void) => {
      if (!subscriptions.has(event)) subscriptions.set(event, []);
      subscriptions.get(event)!.push(handler);
      return () => {
        const hs = subscriptions.get(event);
        if (hs) { const i = hs.indexOf(handler); if (i >= 0) hs.splice(i, 1); }
      };
    },
  }),
}));

const mockInvalidateQueries = vi.fn().mockResolvedValue(undefined);
const mockGetQueryData = vi.fn().mockReturnValue(undefined);

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: mockInvalidateQueries,
    setQueryData: vi.fn(),
    cancelQueries: vi.fn().mockResolvedValue(undefined),
    getQueryData: (...args: unknown[]) => mockGetQueryData(...args),
  }),
}));

vi.mock("@/lib/logger", () => ({
  logger: { debug: vi.fn(), warn: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

// ============================================================================
// Import hook and store under test (after mocks)
// ============================================================================

import { useAgentEvents } from "./useAgentEvents";
import { useIdeationStore } from "@/stores/ideationStore";

// ============================================================================
// Helpers
// ============================================================================

const PARENT_SESSION_ID = "parent-session-abc";
const CHILD_SESSION_ID = "child-session-xyz";

function makeRunCompletedEvent(contextId: string) {
  return {
    context_type: "ideation",
    context_id: contextId,
    conversation_id: "conv-" + contextId,
    status: "completed",
  };
}

function makeErrorEvent(contextId: string) {
  return {
    context_type: "ideation",
    context_id: contextId,
    conversation_id: "conv-" + contextId,
    error: "process crashed",
  };
}

function makeStoppedEvent(contextId: string) {
  return {
    context_type: "ideation",
    context_id: contextId,
    conversation_id: "conv-" + contextId,
    agent_run_id: "run-" + contextId,
  };
}

// ============================================================================
// Tests — PO6: child termination reverse lookup
// ============================================================================

describe("useAgentEvents — child termination reverse lookup (PO6)", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    mockGetQueryData.mockReturnValue(undefined);
    chatStoreMocks.setAgentStatus.mockClear();
    chatStoreMocks.updateLastAgentEvent.mockClear();
    chatStoreMocks.setActiveAgentRun.mockClear();
    chatStoreMocks.clearActiveAgentRun.mockClear();
    chatStoreMocks.agentStatus = {};
    chatStoreMocks.activeAgentRunIds = {};
    chatStoreMocks.activeConversationIds = {};
    chatStoreMocks.lastAgentEventTimestamp = {};
    chatStoreMocks.toolCallStartTimes = {};
    chatStoreMocks.lastToolCallCompletionTimestamp = {};
    useIdeationStore.setState({
      sessions: {},
      activeSessionId: null,
      isLoading: false,
      error: null,
      planArtifact: null,
      activeVerificationChildId: {
        [PARENT_SESSION_ID]: CHILD_SESSION_ID,
      },
    });
  });

  it("(PO6-run_completed) child run_completed clears parent activeVerificationChildId and sets parent idle", () => {
    renderHook(() => useAgentEvents(null));

    act(() => {
      fireEvent("agent:run_completed", makeRunCompletedEvent(CHILD_SESSION_ID));
    });

    expect(useIdeationStore.getState().activeVerificationChildId[PARENT_SESSION_ID]).toBeNull();
    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:" + PARENT_SESSION_ID,
      "idle"
    );
    expect(mockInvalidateQueries).toHaveBeenCalledWith({
      queryKey: ["chat", "conversation-stats", "conv-" + CHILD_SESSION_ID],
    });
  });

  it("(PO6-error) child error event clears parent activeVerificationChildId and sets parent idle", () => {
    renderHook(() => useAgentEvents(null));

    act(() => {
      fireEvent("agent:error", makeErrorEvent(CHILD_SESSION_ID));
    });

    expect(useIdeationStore.getState().activeVerificationChildId[PARENT_SESSION_ID]).toBeNull();
    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:" + PARENT_SESSION_ID,
      "idle"
    );
  });

  it("(PO6-stopped) child stopped event clears parent activeVerificationChildId and sets parent idle", () => {
    renderHook(() => useAgentEvents(null));

    act(() => {
      fireEvent("agent:stopped", makeStoppedEvent(CHILD_SESSION_ID));
    });

    expect(useIdeationStore.getState().activeVerificationChildId[PARENT_SESSION_ID]).toBeNull();
    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:" + PARENT_SESSION_ID,
      "idle"
    );
  });

  it("(PO6-isolation) only the matching parent is cleared; unrelated parent is untouched", () => {
    const OTHER_PARENT = "other-parent-session";
    const OTHER_CHILD = "other-child-session";
    useIdeationStore.setState({
      activeVerificationChildId: {
        [PARENT_SESSION_ID]: CHILD_SESSION_ID,
        [OTHER_PARENT]: OTHER_CHILD,
      },
    });

    renderHook(() => useAgentEvents(null));

    act(() => {
      fireEvent("agent:run_completed", makeRunCompletedEvent(CHILD_SESSION_ID));
    });

    // Matching parent cleared
    expect(useIdeationStore.getState().activeVerificationChildId[PARENT_SESSION_ID]).toBeNull();
    // Unrelated parent untouched
    expect(useIdeationStore.getState().activeVerificationChildId[OTHER_PARENT]).toBe(OTHER_CHILD);
    // Only matching parent's key gets idle
    const idleCalls = chatStoreMocks.setAgentStatus.mock.calls.filter(
      (c: unknown[]) => c[1] === "idle"
    );
    expect(idleCalls.some((c: unknown[]) => c[0] === "session:" + PARENT_SESSION_ID)).toBe(true);
    expect(idleCalls.some((c: unknown[]) => c[0] === "session:" + OTHER_PARENT)).toBe(false);
  });

  it("(PO6-no-match) unrelated context termination does not modify activeVerificationChildId", () => {
    renderHook(() => useAgentEvents(null));

    act(() => {
      fireEvent("agent:run_completed", makeRunCompletedEvent("unrelated-session-999"));
    });

    // Nothing changed for PARENT_SESSION_ID
    expect(useIdeationStore.getState().activeVerificationChildId[PARENT_SESSION_ID]).toBe(CHILD_SESSION_ID);
  });
});

// ============================================================================
// Tests — abnormal termination detection (child crash while in_progress)
// ============================================================================

describe("useAgentEvents — abnormal termination detection", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    chatStoreMocks.setAgentStatus.mockClear();
    chatStoreMocks.agentStatus = {};
    chatStoreMocks.activeConversationIds = {};
    chatStoreMocks.lastAgentEventTimestamp = {};
    chatStoreMocks.toolCallStartTimes = {};
    chatStoreMocks.lastToolCallCompletionTimestamp = {};
    useIdeationStore.setState({
      sessions: {},
      activeSessionId: null,
      isLoading: false,
      error: null,
      planArtifact: null,
      activeVerificationChildId: {
        [PARENT_SESSION_ID]: CHILD_SESSION_ID,
      },
    });
  });

  it("invalidates verification query when child terminates and verification still in_progress", () => {
    mockGetQueryData.mockReturnValue({ inProgress: true });
    renderHook(() => useAgentEvents(null));

    act(() => {
      fireEvent("agent:run_completed", makeRunCompletedEvent(CHILD_SESSION_ID));
    });

    expect(mockInvalidateQueries).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: ["verification", PARENT_SESSION_ID] })
    );
  });

  it("does NOT invalidate verification when child terminates and verification is NOT in_progress", () => {
    mockGetQueryData.mockReturnValue({ inProgress: false });
    renderHook(() => useAgentEvents(null));

    act(() => {
      fireEvent("agent:run_completed", makeRunCompletedEvent(CHILD_SESSION_ID));
    });

    const verificationInvalidation = mockInvalidateQueries.mock.calls.find(
      (c: unknown[]) => JSON.stringify(c[0]) === JSON.stringify({ queryKey: ["verification", PARENT_SESSION_ID] })
    );
    expect(verificationInvalidation).toBeUndefined();
  });
});
