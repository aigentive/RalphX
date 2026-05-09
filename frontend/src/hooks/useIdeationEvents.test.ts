/**
 * useIdeationEvents tests
 *
 * Tests for ideation:session_created event subscription:
 * 1. Valid payload triggers invalidateQueries with ideationKeys.sessions()
 * 2. Malformed payload (missing sessionId) rejected gracefully by Zod without crash
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

// ============================================================================
// Hoisted mocks (must run before vi.mock factories)
// ============================================================================

const chatStoreMocks = vi.hoisted(() => ({
  setAgentStatus: vi.fn(),
  updateLastAgentEvent: vi.fn(),
}));

const ideationStoreMocks = vi.hoisted(() => ({
  updateSession: vi.fn(),
  clearVerificationNotification: vi.fn(),
  setVerificationNotification: vi.fn(),
  setActiveVerificationChildId: vi.fn(),
  setLastVerificationChildId: vi.fn(),
}));

// ============================================================================
// Mock infrastructure
// ============================================================================

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
    emit: vi.fn(),
  }),
}));

const mockInvalidateQueries = vi.fn().mockResolvedValue(undefined);
const mockSetQueryData = vi.fn();

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: mockInvalidateQueries,
    setQueryData: mockSetQueryData,
  }),
}));

vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: (selector: (s: object) => unknown) =>
    selector({
      updateSession: ideationStoreMocks.updateSession,
      clearVerificationNotification: ideationStoreMocks.clearVerificationNotification,
      setVerificationNotification: ideationStoreMocks.setVerificationNotification,
      setActiveVerificationChildId: ideationStoreMocks.setActiveVerificationChildId,
      setLastVerificationChildId: ideationStoreMocks.setLastVerificationChildId,
    }),
}));

vi.mock("@/stores/chatStore", () => ({
  useChatStore: Object.assign(vi.fn(), {
    getState: () => chatStoreMocks,
  }),
}));

vi.mock("@/hooks/useIdeation", () => ({
  ideationKeys: {
    sessions: () => ["sessions"],
    sessionWithData: (sessionId: string) => ["sessions", "detail", sessionId, "with-data"],
  },
}));

vi.mock("@/hooks/useDependencyGraph", () => ({
  dependencyKeys: {
    graphs: () => ["dependency-graphs"],
  },
}));

vi.mock("@/hooks/useTasks", () => ({
  taskKeys: {
    all: ["tasks"],
  },
}));

vi.mock("@/hooks/useProposals", () => ({
  proposalKeys: {
    list: (sessionId: string) => ["proposals", "list", sessionId],
  },
}));

// ============================================================================
// Import hook under test (after mocks)
// ============================================================================

import { useIdeationEvents } from "./useIdeationEvents";

// ============================================================================
// Tests
// ============================================================================

describe("useIdeationEvents — ideation:session_created", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    mockSetQueryData.mockClear();
  });

  it("(1) valid payload triggers invalidateQueries with ideationKeys.sessions()", () => {
    renderHook(() => useIdeationEvents());

    act(() => {
      fireEvent("ideation:session_created", { sessionId: "test-123", projectId: "proj-456" });
    });

    expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ["sessions"] });
  });

  it("(2) malformed payload (missing sessionId) rejected by Zod without crash", () => {
    renderHook(() => useIdeationEvents());

    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});

    act(() => {
      // Missing sessionId — Zod should reject this without throwing
      fireEvent("ideation:session_created", { projectId: "proj-456" });
    });

    expect(consoleError).toHaveBeenCalledWith(
      expect.stringContaining("Invalid ideation:session_created event:"),
      expect.any(String)
    );
    // invalidateQueries must NOT have been called for the malformed payload
    expect(mockInvalidateQueries).not.toHaveBeenCalledWith({ queryKey: ["sessions"] });

    consoleError.mockRestore();
  });
});

describe("useIdeationEvents — ideation:session_accepted", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
  });

  it("(1) valid payload invalidates sessions, sessionWithData, tasks, proposals, and plan-branch", () => {
    renderHook(() => useIdeationEvents());

    act(() => {
      fireEvent("ideation:session_accepted", { sessionId: "sess-123", projectId: "proj-456" });
    });

    expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ["sessions"] });
    expect(mockInvalidateQueries).toHaveBeenCalledWith({
      queryKey: ["sessions", "detail", "sess-123", "with-data"],
    });
    expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ["tasks"] });
    expect(mockInvalidateQueries).toHaveBeenCalledWith({
      queryKey: ["proposals", "list", "sess-123"],
    });
    expect(mockInvalidateQueries).toHaveBeenCalledWith({ queryKey: ["plan-branch"] });
  });

  it("(2) malformed payload (missing sessionId) rejected by Zod without crash", () => {
    renderHook(() => useIdeationEvents());

    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});

    act(() => {
      fireEvent("ideation:session_accepted", { projectId: "proj-456" });
    });

    expect(consoleError).toHaveBeenCalledWith(
      expect.stringContaining("Invalid ideation:session_accepted event:"),
      expect.any(String)
    );
    expect(mockInvalidateQueries).not.toHaveBeenCalled();

    consoleError.mockRestore();
  });
});

// ============================================================================
// Tests — parent synthetic status during verification child session
// ============================================================================

describe("useIdeationEvents — parent synthetic status during verification", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    mockSetQueryData.mockClear();
    chatStoreMocks.setAgentStatus.mockClear();
    chatStoreMocks.updateLastAgentEvent.mockClear();
    ideationStoreMocks.updateSession.mockClear();
    ideationStoreMocks.clearVerificationNotification.mockClear();
    ideationStoreMocks.setVerificationNotification.mockClear();
    ideationStoreMocks.setActiveVerificationChildId.mockClear();
    ideationStoreMocks.setLastVerificationChildId.mockClear();
  });

  it("(3) ideation:child_session_created with purpose=verification sets parent agentStatus to 'generating'", () => {
    renderHook(() => useIdeationEvents());

    act(() => {
      fireEvent("ideation:child_session_created", {
        sessionId: "child-session-123",
        parentSessionId: "parent-session-456",
        title: "Verification Session",
        purpose: "verification",
      });
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:parent-session-456",
      "generating"
    );
    expect(chatStoreMocks.updateLastAgentEvent).toHaveBeenCalledWith(
      "session:parent-session-456"
    );
    expect(ideationStoreMocks.setLastVerificationChildId).toHaveBeenCalledWith(
      "parent-session-456",
      "child-session-123"
    );
    expect(mockSetQueryData).toHaveBeenCalledWith(
      [
        "agents",
        "chat-focus",
        "latest-child-session-id",
        "parent-session-456",
        "verification",
      ],
      {
        sessionId: "parent-session-456",
        purpose: "verification",
        latestChildSessionId: "child-session-123",
      }
    );
    expect(mockInvalidateQueries).toHaveBeenCalledWith({
      queryKey: ["childSessions", "parent-session-456", "verification"],
    });
    expect(mockInvalidateQueries).toHaveBeenCalledWith({
      queryKey: ["child-session-status", "parent-session-456"],
    });
    expect(mockInvalidateQueries).toHaveBeenCalledWith({
      queryKey: ["child-session-status", "child-session-123"],
    });
    expect(mockInvalidateQueries).toHaveBeenCalledWith({
      queryKey: ["verification", "parent-session-456"],
    });
  });

  it("does not show verification-started state when verification child launch is deferred", () => {
    renderHook(() => useIdeationEvents());

    act(() => {
      fireEvent("ideation:child_session_created", {
        sessionId: "verification-child-queued",
        parentSessionId: "parent-session-456",
        title: "Auto-verification",
        purpose: "verification",
        orchestrationTriggered: false,
        pendingInitialPrompt: "verify",
      });
    });

    expect(ideationStoreMocks.setVerificationNotification).not.toHaveBeenCalled();
    expect(ideationStoreMocks.setActiveVerificationChildId).toHaveBeenCalledWith("parent-session-456", null);
    expect(ideationStoreMocks.setLastVerificationChildId).toHaveBeenCalledWith(
      "parent-session-456",
      "verification-child-queued"
    );
    expect(mockSetQueryData).toHaveBeenCalledWith(
      [
        "agents",
        "chat-focus",
        "latest-child-session-id",
        "parent-session-456",
        "verification",
      ],
      {
        sessionId: "parent-session-456",
        purpose: "verification",
        latestChildSessionId: "verification-child-queued",
      }
    );
    expect(ideationStoreMocks.updateSession).toHaveBeenCalledWith("parent-session-456", {
      verificationInProgress: false,
    });
    expect(ideationStoreMocks.clearVerificationNotification).toHaveBeenCalledWith("parent-session-456");
  });

  it("(4) non-verification child session does NOT set parent agentStatus", () => {
    renderHook(() => useIdeationEvents());

    act(() => {
      fireEvent("ideation:child_session_created", {
        sessionId: "child-session-789",
        parentSessionId: "parent-session-456",
        title: "Follow-up Session",
        purpose: "general",
      });
    });

    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalled();
    expect(chatStoreMocks.updateLastAgentEvent).not.toHaveBeenCalled();
    expect(ideationStoreMocks.setLastVerificationChildId).not.toHaveBeenCalled();
  });
});
