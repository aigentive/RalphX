/**
 * useVerificationEvents tests
 *
 * Tests for the verificationUpdateSeq feature (B1 fix):
 * 1. Tauri event increments verificationUpdateSeq in store
 * 2. seq > 0 → store overrides stale React Query data in resolvedSession merge logic
 * 3. seq === 0 → React Query data used as-is (no override)
 * 4. Session switch → seq resets (new session starts at 0)
 *
 * Tests for race condition fix (async IIFE, cancelQueries, round guard, conditional invalidation, planVersion):
 * 5. cancelQueries called before setQueryData
 * 6. Out-of-order event (round < cached) is rejected by round guard
 * 7. Newer generation bypasses stale round rejection
 * 8. Reset event (status=unverified) allowed even when round regresses
 * 9. Undefined round always accepted (no guard applied)
 * 10. Fallback path (no currentGaps/rounds) calls invalidateQueries for verification
 * 11. Fast path skips verification invalidateQueries unless generation changes or the update is terminal
 * 12. planVersion stamped from store onto setQueryData call
 * 13. planVersion omitted when store has no planArtifact
 *
 * Tests for toast notifications (terminal transitions):
 * 13. success toast on verified + in_progress=false
 * 14. warning toast on needs_revision + in_progress=false
 * 15. no toast on intermediate reviewing events
 * 16. no toast when in_progress=true (non-terminal)
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { IdeationSession } from "@/types/ideation";

// ============================================================================
// Hoisted mocks (must run before vi.mock factories)
// ============================================================================

const chatStoreMocks = vi.hoisted(() => ({
  setAgentStatus: vi.fn(),
}));

vi.mock("@/stores/chatStore", () => ({
  useChatStore: Object.assign(vi.fn(), {
    getState: () => chatStoreMocks,
  }),
}));

// ============================================================================
// Mock sonner toast
// ============================================================================

const mockToastSuccess = vi.fn();
const mockToastWarning = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => mockToastSuccess(...args),
    warning: (...args: unknown[]) => mockToastWarning(...args),
    error: vi.fn(),
  },
}));

// ============================================================================
// Mock infrastructure
// ============================================================================

// Capture event bus subscriptions so tests can fire events manually
const subscriptions = new Map<string, ((...args: unknown[]) => void)[]>();

function fireEvent<T>(event: string, payload: T) {
  const handlers = subscriptions.get(event);
  if (handlers) {
    for (const handler of handlers) {
      handler(payload);
    }
  }
}

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (...args: unknown[]) => void) => {
      if (!subscriptions.has(event)) subscriptions.set(event, []);
      subscriptions.get(event)!.push(handler);
      return () => {
        const handlers = subscriptions.get(event);
        if (handlers) {
          const idx = handlers.indexOf(handler);
          if (idx >= 0) handlers.splice(idx, 1);
        }
      };
    },
  }),
}));

const mockInvalidateQueries = vi.fn().mockResolvedValue(undefined);
const mockSetQueryData = vi.fn();
const mockCancelQueries = vi.fn().mockResolvedValue(undefined);
let mockGetQueryData = vi.fn().mockReturnValue(undefined);

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: mockInvalidateQueries,
    setQueryData: mockSetQueryData,
    cancelQueries: mockCancelQueries,
    getQueryData: (...args: unknown[]) => mockGetQueryData(...args),
  }),
}));

vi.mock("@/hooks/useIdeation", () => ({
  ideationKeys: {
    sessions: () => ["sessions"],
    sessionWithData: (id: string) => ["session", id],
  },
}));

// ============================================================================
// Import hook and store under test (after mocks)
// ============================================================================

import { useVerificationEvents } from "./useVerificationEvents";
import { useIdeationStore } from "@/stores/ideationStore";
import { verificationStatusKey } from "./useVerificationStatus";

// ============================================================================
// Helpers
// ============================================================================

const SESSION_ID = "session-abc";

const createTestSession = (overrides: Partial<IdeationSession> = {}): IdeationSession => ({
  id: SESSION_ID,
  projectId: "project-1",
  title: "Test Session",
  status: "active",
  createdAt: "2026-01-24T12:00:00Z",
  updatedAt: "2026-01-24T12:00:00Z",
  archivedAt: null,
  convertedAt: null,
  verificationStatus: "unverified",
  verificationInProgress: false,
  verificationUpdateSeq: 0,
  ...overrides,
});

const makeVerificationEvent = (overrides: Record<string, unknown> = {}) => ({
  session_id: SESSION_ID,
  status: "reviewing",
  in_progress: true,
  generation: 1,
  gap_score: 42,
  ...overrides,
});

const makeFullVerificationEvent = (overrides: Record<string, unknown> = {}) => ({
  session_id: SESSION_ID,
  status: "needs_revision",
  in_progress: false,
  generation: 1,
  gap_score: 55,
  round: 2,
  max_rounds: 5,
  current_gaps: [
    {
      severity: "high",
      category: "correctness",
      description: "Missing null check",
      why_it_matters: "Will crash at runtime",
    },
  ],
  rounds: [],
  ...overrides,
});

/**
 * Mirrors the resolvedSession merge logic from App.tsx.
 * Used to test the merge semantics without rendering App.
 */
function resolveSession(
  fetchedSession: IdeationSession | undefined,
  activeSession: IdeationSession | null
): IdeationSession | null {
  const isFetchedSessionCurrent = fetchedSession?.id === activeSession?.id;
  const base = isFetchedSessionCurrent && fetchedSession ? fetchedSession : activeSession;

  if (
    base &&
    activeSession &&
    activeSession.id === base.id &&
    (activeSession.verificationUpdateSeq ?? 0) > 0
  ) {
    return {
      ...base,
      verificationStatus: activeSession.verificationStatus ?? base.verificationStatus,
      verificationInProgress: activeSession.verificationInProgress ?? base.verificationInProgress,
      gapScore: activeSession.gapScore !== undefined ? activeSession.gapScore : base.gapScore,
    };
  }

  return base;
}

// ============================================================================
// Tests — verificationUpdateSeq (existing)
// ============================================================================

describe("useVerificationEvents — verificationUpdateSeq", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    mockSetQueryData.mockClear();
    mockCancelQueries.mockClear();
    mockToastSuccess.mockClear();
    mockToastWarning.mockClear();
    mockGetQueryData = vi.fn().mockReturnValue(undefined);
    useIdeationStore.setState({
      sessions: { [SESSION_ID]: createTestSession() },
      activeSessionId: SESSION_ID,
      isLoading: false,
      error: null,
      planArtifact: null,
    });
  });

  it("(1) increments verificationUpdateSeq in store when Tauri event fires", () => {
    renderHook(() => useVerificationEvents());

    expect(useIdeationStore.getState().sessions[SESSION_ID]?.verificationUpdateSeq).toBe(0);

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent());
    });

    expect(useIdeationStore.getState().sessions[SESSION_ID]?.verificationUpdateSeq).toBe(1);

    // Second event increments again
    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({ status: "needs_revision", in_progress: false }));
    });

    expect(useIdeationStore.getState().sessions[SESSION_ID]?.verificationUpdateSeq).toBe(2);
  });
});

describe("resolvedSession merge logic — verificationUpdateSeq guard", () => {
  const staleQuerySession = createTestSession({
    verificationStatus: "unverified",
    verificationInProgress: false,
    gapScore: null,
    verificationUpdateSeq: 0,
  });

  it("(2) seq > 0 → store fields override stale React Query data", () => {
    const storeSession = createTestSession({
      verificationStatus: "needs_revision",
      verificationInProgress: false,
      gapScore: 75,
      verificationUpdateSeq: 2,
    });

    const resolved = resolveSession(staleQuerySession, storeSession);

    expect(resolved?.verificationStatus).toBe("needs_revision");
    expect(resolved?.verificationInProgress).toBe(false);
    expect(resolved?.gapScore).toBe(75);
  });

  it("(3) seq === 0 → React Query data used as-is (no store override)", () => {
    const freshQuerySession = createTestSession({
      verificationStatus: "verified",
      verificationInProgress: false,
      gapScore: 10,
      verificationUpdateSeq: 0,
    });
    const storeSessionNoEvents = createTestSession({
      verificationStatus: "unverified",
      verificationInProgress: false,
      verificationUpdateSeq: 0,
    });

    const resolved = resolveSession(freshQuerySession, storeSessionNoEvents);

    // No seq override → fetchedSession is used, so verificationStatus from query
    expect(resolved?.verificationStatus).toBe("verified");
    expect(resolved?.gapScore).toBe(10);
  });

  it("(4) session switch → new session has seq === 0, React Query data used as-is", () => {
    const newSessionId = "session-new";
    const newSessionFromQuery = createTestSession({
      id: newSessionId,
      verificationStatus: "verified",
      verificationInProgress: false,
      gapScore: 5,
      verificationUpdateSeq: 0,
    });
    // After session switch, the new session in store has no events yet (seq=0)
    const newSessionInStore = createTestSession({
      id: newSessionId,
      verificationStatus: "unverified",
      verificationInProgress: false,
      verificationUpdateSeq: 0,
    });

    const resolved = resolveSession(newSessionFromQuery, newSessionInStore);

    // seq === 0 → no merge, base (fetchedSession) is returned as-is
    expect(resolved?.id).toBe(newSessionId);
    expect(resolved?.verificationStatus).toBe("verified");
    expect(resolved?.gapScore).toBe(5);
  });
});

// ============================================================================
// Tests — race condition fix (new)
// ============================================================================

describe("useVerificationEvents — race condition fix", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    mockSetQueryData.mockClear();
    mockCancelQueries.mockClear();
    mockToastSuccess.mockClear();
    mockToastWarning.mockClear();
    mockGetQueryData = vi.fn().mockReturnValue(undefined);
    useIdeationStore.setState({
      sessions: { [SESSION_ID]: createTestSession() },
      activeSessionId: SESSION_ID,
      isLoading: false,
      error: null,
      planArtifact: null,
    });
  });

  it("(5) cancelQueries called before setQueryData on fast path event", async () => {
    renderHook(() => useVerificationEvents());

    const callOrder: string[] = [];
    mockCancelQueries.mockImplementation(async () => { callOrder.push("cancel"); });
    mockSetQueryData.mockImplementation(() => { callOrder.push("set"); });

    await act(async () => {
      fireEvent("plan_verification:status_changed", makeFullVerificationEvent());
    });

    expect(callOrder).toEqual(["cancel", "set"]);
    expect(mockCancelQueries).toHaveBeenCalledWith({
      queryKey: verificationStatusKey(SESSION_ID),
    });
    expect(mockSetQueryData).toHaveBeenCalledWith(
      verificationStatusKey(SESSION_ID),
      expect.any(Object)
    );
  });

  it("(6) out-of-order event (round < cached.currentRound) is rejected by round guard", async () => {
    // Cached has round=3
    mockGetQueryData = vi.fn().mockReturnValue({ generation: 1, currentRound: 3, gaps: [], rounds: [] });
    renderHook(() => useVerificationEvents());

    await act(async () => {
      // Event with round=2 (stale)
      fireEvent("plan_verification:status_changed", makeFullVerificationEvent({ round: 2, status: "needs_revision" }));
    });

    expect(mockSetQueryData).not.toHaveBeenCalled();
  });

  it("(7) newer generation bypasses stale round rejection", async () => {
    mockGetQueryData = vi.fn().mockReturnValue({ generation: 1, currentRound: 3, gaps: [], rounds: [] });
    renderHook(() => useVerificationEvents());

    await act(async () => {
      fireEvent(
        "plan_verification:status_changed",
        makeFullVerificationEvent({ generation: 2, round: 1, status: "reviewing", in_progress: true })
      );
    });

    expect(mockSetQueryData).toHaveBeenCalledTimes(1);
    const [, cacheData] = mockSetQueryData.mock.calls[0] as [
      unknown,
      { generation?: number; currentRound?: number }
    ];
    expect(cacheData.generation).toBe(2);
    expect(cacheData.currentRound).toBe(1);
    expect(mockInvalidateQueries).toHaveBeenCalledWith({
      queryKey: ["verification", SESSION_ID],
      refetchType: "active",
    });
  });

  it("(8) reset event (status=unverified) allowed even when round regresses below cached", async () => {
    // Cached has round=3
    mockGetQueryData = vi.fn().mockReturnValue({ generation: 1, currentRound: 3, gaps: [], rounds: [] });
    renderHook(() => useVerificationEvents());

    await act(async () => {
      // Reset event — round undefined, status=unverified
      fireEvent("plan_verification:status_changed", makeFullVerificationEvent({ round: undefined, status: "unverified" }));
    });

    expect(mockSetQueryData).toHaveBeenCalledTimes(1);
  });

  it("(9) undefined round in event always accepted (no guard applied)", async () => {
    // Cached has a round
    mockGetQueryData = vi.fn().mockReturnValue({ currentRound: 2, gaps: [], rounds: [] });
    renderHook(() => useVerificationEvents());

    await act(async () => {
      // Event with no round field — should always pass guard
      fireEvent("plan_verification:status_changed", makeFullVerificationEvent({ round: undefined, status: "needs_revision" }));
    });

    expect(mockSetQueryData).toHaveBeenCalledTimes(1);
  });

  it("(10) fallback path (no currentGaps/rounds) calls invalidateQueries for verification", async () => {
    renderHook(() => useVerificationEvents());

    await act(async () => {
      // Event without current_gaps or rounds → fallback path
      fireEvent("plan_verification:status_changed", makeVerificationEvent());
    });

    const verificationInvalidation = mockInvalidateQueries.mock.calls.find(
      (call) => JSON.stringify(call[0]) === JSON.stringify({ queryKey: verificationStatusKey(SESSION_ID) })
    );
    expect(verificationInvalidation).toBeDefined();
    const childSessionsInvalidation = mockInvalidateQueries.mock.calls.find(
      (call) => JSON.stringify(call[0]) === JSON.stringify({ queryKey: ["childSessions", SESSION_ID, "verification"] })
    );
    expect(childSessionsInvalidation).toBeDefined();
    const childStatusInvalidation = mockInvalidateQueries.mock.calls.find(
      (call) => JSON.stringify(call[0]) === JSON.stringify({ queryKey: ["child-session-status", SESSION_ID] })
    );
    expect(childStatusInvalidation).toBeDefined();
    const agentsLatestChildInvalidation = mockInvalidateQueries.mock.calls.find(
      (call) =>
        JSON.stringify(call[0]) ===
        JSON.stringify({
          queryKey: [
            "agents",
            "chat-focus",
            "latest-child-session-id",
            SESSION_ID,
            "verification",
          ],
        })
    );
    expect(agentsLatestChildInvalidation).toBeDefined();
  });

  it("(11) fast path skips verification invalidateQueries when generation is unchanged on non-terminal updates", async () => {
    mockGetQueryData = vi.fn().mockReturnValue({ generation: 1, currentRound: 2, gaps: [], rounds: [] });
    renderHook(() => useVerificationEvents());

    await act(async () => {
      fireEvent(
        "plan_verification:status_changed",
        makeFullVerificationEvent({ status: "reviewing", in_progress: true })
      );
    });

    const verificationInvalidation = mockInvalidateQueries.mock.calls.find(
      (call) => JSON.stringify(call[0]) === JSON.stringify({ queryKey: ["verification", SESSION_ID] })
    );
    expect(verificationInvalidation).toBeUndefined();
    // But session invalidations still fire
    expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ["sessions"] });
  });

  it("(12) planVersion stamped from store onto setQueryData when planArtifact present", async () => {
    // Set planArtifact with just the fields the hook reads (metadata.version)
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    useIdeationStore.setState({ planArtifact: { id: "art-1", metadata: { version: 3 } } as any });

    renderHook(() => useVerificationEvents());

    await act(async () => {
      fireEvent("plan_verification:status_changed", makeFullVerificationEvent());
    });

    expect(mockSetQueryData).toHaveBeenCalledTimes(1);
    const [, cacheData] = mockSetQueryData.mock.calls[0] as [unknown, { planVersion?: number }];
    expect(cacheData.planVersion).toBe(3);
  });

  it("(13) planVersion omitted from setQueryData when store has no planArtifact", async () => {
    // planArtifact is null (default in beforeEach)
    renderHook(() => useVerificationEvents());

    await act(async () => {
      fireEvent("plan_verification:status_changed", makeFullVerificationEvent());
    });

    expect(mockSetQueryData).toHaveBeenCalledTimes(1);
    const [, cacheData] = mockSetQueryData.mock.calls[0] as [unknown, { planVersion?: number }];
    expect(cacheData.planVersion).toBeUndefined();
  });

  it("(14) terminal fast path marks the verification query stale without immediate refetch when generation is unchanged", async () => {
    mockGetQueryData = vi.fn().mockReturnValue({
      generation: 1,
      currentRound: 1,
      gaps: [],
      rounds: [],
      roundDetails: [],
    });
    renderHook(() => useVerificationEvents());

    await act(async () => {
      fireEvent(
        "plan_verification:status_changed",
        makeFullVerificationEvent({ status: "needs_revision", in_progress: false })
      );
    });

    expect(mockInvalidateQueries).toHaveBeenCalledWith({
      queryKey: verificationStatusKey(SESSION_ID),
      refetchType: "none",
    });
  });
});

// ============================================================================
// Tests — toast notifications (new)
// ============================================================================

describe("useVerificationEvents — toast notifications", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    mockSetQueryData.mockClear();
    mockCancelQueries.mockClear();
    mockToastSuccess.mockClear();
    mockToastWarning.mockClear();
    mockGetQueryData = vi.fn().mockReturnValue(undefined);
    useIdeationStore.setState({
      sessions: { [SESSION_ID]: createTestSession() },
      activeSessionId: SESSION_ID,
      isLoading: false,
      error: null,
      planArtifact: null,
    });
  });

  it("(13) fires success toast when status=verified and in_progress=false", () => {
    renderHook(() => useVerificationEvents());

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({
        status: "verified",
        in_progress: false,
      }));
    });

    expect(mockToastSuccess).toHaveBeenCalledTimes(1);
    expect(mockToastSuccess).toHaveBeenCalledWith(
      expect.stringContaining("verified"),
      expect.objectContaining({ duration: expect.any(Number) })
    );
    expect(mockToastWarning).not.toHaveBeenCalled();
  });

  it("(14) fires warning toast when status=needs_revision and in_progress=false", () => {
    renderHook(() => useVerificationEvents());

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({
        status: "needs_revision",
        in_progress: false,
      }));
    });

    expect(mockToastWarning).toHaveBeenCalledTimes(1);
    expect(mockToastWarning).toHaveBeenCalledWith(
      expect.stringContaining("gaps"),
      expect.objectContaining({ duration: expect.any(Number) })
    );
    expect(mockToastSuccess).not.toHaveBeenCalled();
  });

  it("(15) no toast for intermediate reviewing events", () => {
    renderHook(() => useVerificationEvents());

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({
        status: "reviewing",
        in_progress: true,
      }));
    });

    expect(mockToastSuccess).not.toHaveBeenCalled();
    expect(mockToastWarning).not.toHaveBeenCalled();
  });

  it("(16) no toast when in_progress=true even for terminal-looking status", () => {
    renderHook(() => useVerificationEvents());

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({
        status: "verified",
        in_progress: true,
      }));
    });

    expect(mockToastSuccess).not.toHaveBeenCalled();
    expect(mockToastWarning).not.toHaveBeenCalled();
  });
});

// ============================================================================
// Tests — parent synthetic status cleared on verification complete
// ============================================================================

describe("useVerificationEvents — synthetic parent status cleared on completion", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    mockSetQueryData.mockClear();
    mockCancelQueries.mockClear();
    mockToastSuccess.mockClear();
    mockToastWarning.mockClear();
    mockGetQueryData = vi.fn().mockReturnValue(undefined);
    chatStoreMocks.setAgentStatus.mockClear();
    useIdeationStore.setState({
      sessions: { [SESSION_ID]: createTestSession() },
      activeSessionId: SESSION_ID,
      isLoading: false,
      error: null,
      planArtifact: null,
      activeVerificationChildId: { [SESSION_ID]: "child-session-xyz" },
    });
  });

  it("(17) clears parent synthetic generating status when in_progress=false and verification child was active", () => {
    renderHook(() => useVerificationEvents());

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({
        status: "verified",
        in_progress: false,
      }));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:" + SESSION_ID,
      "idle"
    );
  });

  it("(18) does NOT clear parent status when in_progress=true (non-terminal)", () => {
    renderHook(() => useVerificationEvents());

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({
        status: "reviewing",
        in_progress: true,
      }));
    });

    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalled();
  });

  // PO3: verification complete → activeVerificationChildId cleared + parent idle
  it("(PO3) clears activeVerificationChildId before setting parent idle on terminal event", () => {
    renderHook(() => useVerificationEvents());

    expect(useIdeationStore.getState().activeVerificationChildId[SESSION_ID]).toBe("child-session-xyz");

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({
        status: "verified",
        in_progress: false,
      }));
    });

    // Child ref cleared
    expect(useIdeationStore.getState().activeVerificationChildId[SESSION_ID]).toBeNull();
    // Parent set to idle
    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("session:" + SESSION_ID, "idle");
  });

  // PO3: always sets parent idle even when no active child (unconditional)
  it("(PO3b) sets parent idle on terminal event even when no verification child was active", () => {
    useIdeationStore.setState({ activeVerificationChildId: {} });
    renderHook(() => useVerificationEvents());

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({
        status: "needs_revision",
        in_progress: false,
      }));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("session:" + SESSION_ID, "idle");
  });

  // PO7: terminal status → no stale indicator left after cleanup
  it("(PO7) after terminal verification event, activeVerificationChildId is null and parent is idle", () => {
    renderHook(() => useVerificationEvents());

    act(() => {
      fireEvent("plan_verification:status_changed", makeVerificationEvent({
        status: "needs_revision",
        in_progress: false,
      }));
    });

    expect(useIdeationStore.getState().activeVerificationChildId[SESSION_ID]).toBeNull();
    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("session:" + SESSION_ID, "idle");
  });
});
