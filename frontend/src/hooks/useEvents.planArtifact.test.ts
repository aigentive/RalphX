/**
 * usePlanArtifactEvents hook tests
 *
 * Tests schema validation, all 3 handler tiers, dedup behavior, and rapid event sequences.
 * The hook has 3-tier matching for plan_artifact:updated:
 *   Tier 1 — sessionId matches active session (most reliable)
 *   Tier 2 — planArtifactId fallback (for old backend events without sessionId)
 *   Tier 3 — safety net: invalidate active session query when neither tier matched
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { PlanArtifactEventSchema } from "@/types/events";

// ============================================================================
// Mock infrastructure
// ============================================================================

// Capture subscriptions so tests can fire events manually
const subscriptions = new Map<string, ((...args: unknown[]) => void)[]>();

function fireEvent<T>(event: string, payload: T): void {
  const handlers = subscriptions.get(event);
  if (handlers) {
    for (const handler of handlers) {
      handler(payload);
    }
  }
}

const mockInvalidateQueries = vi.fn();

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

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: mockInvalidateQueries,
  }),
}));

vi.mock("./useIdeation", () => ({
  ideationKeys: {
    sessionDetail: (sessionId: string) => ["ideation", "session", sessionId],
    sessionWithData: (sessionId: string) => ["ideation", "session", sessionId, "with-data"],
  },
}));

// Mutable store state — tests configure these before renderHook()
let mockActiveSessionId: string | null = null;
let mockSessions: Record<
  string,
  {
    id: string;
    planArtifactId: string | null;
    inheritedPlanArtifactId?: string | null;
    planUpdateSeq?: number;
  }
> = {};
const mockSetPlanArtifact = vi.fn();
const mockUpdateSession = vi.fn();

vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: (selector: (s: unknown) => unknown) => {
    return selector({
      activeSessionId: mockActiveSessionId,
      sessions: mockSessions,
      setPlanArtifact: mockSetPlanArtifact,
      updateSession: mockUpdateSession,
    });
  },
}));

// Import AFTER mocks are registered
import { usePlanArtifactEvents } from "./useEvents.planArtifact";

// ============================================================================
// Test helpers
// ============================================================================

function makeArtifact(id: string, version = 1) {
  return { id, name: "Plan", content: `content v${version}`, version };
}

// ============================================================================
// Tests
// ============================================================================

describe("usePlanArtifactEvents", () => {
  beforeEach(() => {
    subscriptions.clear();
    vi.clearAllMocks();
    mockActiveSessionId = null;
    mockSessions = {};
  });

  // ==========================================================================
  // Zod schema validation
  // ==========================================================================

  describe("PlanArtifactEventSchema — updated variant", () => {
    it("accepts updated event with sessionId", () => {
      const result = PlanArtifactEventSchema.safeParse({
        type: "updated",
        sessionId: "session-1",
        artifactId: "artifact-2",
        previousArtifactId: "artifact-1",
        artifact: makeArtifact("artifact-2", 2),
      });
      expect(result.success).toBe(true);
    });

    it("accepts updated event without sessionId (backward compat — old backend events)", () => {
      const result = PlanArtifactEventSchema.safeParse({
        type: "updated",
        artifactId: "artifact-2",
        previousArtifactId: "artifact-1",
        artifact: makeArtifact("artifact-2", 2),
      });
      expect(result.success).toBe(true);
    });

    it("accepts updated event with sessionId: null (orphaned artifact)", () => {
      const result = PlanArtifactEventSchema.safeParse({
        type: "updated",
        sessionId: null,
        artifactId: "artifact-2",
        previousArtifactId: "artifact-1",
        artifact: makeArtifact("artifact-2", 2),
      });
      expect(result.success).toBe(true);
    });
  });

  // ==========================================================================
  // Tier 1 — sessionId match
  // ==========================================================================

  describe("tier 1 — sessionId-based match", () => {
    it("created event increments planUpdateSeq when linking a session to a new plan", () => {
      mockActiveSessionId = "session-1";
      mockSessions = {
        "session-1": { id: "session-1", planArtifactId: null, planUpdateSeq: 0 },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:created", {
          sessionId: "session-1",
          artifact: makeArtifact("artifact-1", 1),
        });
      });

      expect(mockUpdateSession).toHaveBeenCalledWith("session-1", {
        planArtifactId: "artifact-1",
        planUpdateSeq: 1,
      });
    });

    it("sessionId matches active session → setPlanArtifact called and query invalidated", () => {
      mockActiveSessionId = "session-1";

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          sessionId: "session-1",
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(1);
      expect(mockSetPlanArtifact).toHaveBeenCalledWith(
        expect.objectContaining({ id: "artifact-2" })
      );
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["ideation", "session", "session-1"],
      });
    });

    it("tier 1 match returns early — does not also process tier 2 or 3", () => {
      mockActiveSessionId = "session-1";
      // Session has a matching planArtifactId — tier 2 would also match if tier 1 didn't return
      mockSessions = { "session-1": { id: "session-1", planArtifactId: "artifact-1", planUpdateSeq: 3 } };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          sessionId: "session-1",
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      // Tier 1 returned early — one store update, plus the session and artifact
      // cache invalidations owned by that tier only.
      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(1);
      expect(mockInvalidateQueries).toHaveBeenCalledTimes(3);
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["ideation", "session", "session-1"],
      });
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["agents", "artifact", "artifact-2"],
      });
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["agents", "artifact", "artifact-1"],
      });
    });

    it("updated event increments planUpdateSeq when linking the session to the new version", () => {
      mockActiveSessionId = "session-1";
      mockSessions = {
        "session-1": { id: "session-1", planArtifactId: "artifact-1", planUpdateSeq: 3 },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          sessionId: "session-1",
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      expect(mockUpdateSession).toHaveBeenCalledWith("session-1", {
        planArtifactId: "artifact-2",
        planUpdateSeq: 4,
      });
    });
  });

  // ==========================================================================
  // Tier 2 — planArtifactId fallback
  // ==========================================================================

  describe("tier 2 — planArtifactId fallback", () => {
    it("no sessionId + session has matching previousArtifactId → setPlanArtifact called", () => {
      mockActiveSessionId = "session-1";
      mockSessions = {
        "session-1": { id: "session-1", planArtifactId: "artifact-1" },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(1);
      expect(mockSetPlanArtifact).toHaveBeenCalledWith(
        expect.objectContaining({ id: "artifact-2" })
      );
      expect(mockUpdateSession).toHaveBeenCalledWith("session-1", {
        planArtifactId: "artifact-2",
        planUpdateSeq: 1,
      });
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["ideation", "session", "session-1"],
      });
    });

    it("matches session by artifactId when planArtifactId already updated to new value", () => {
      mockActiveSessionId = "session-1";
      // Session's planArtifactId is already updated to artifact-2
      mockSessions = {
        "session-1": { id: "session-1", planArtifactId: "artifact-2" },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 3),
        });
      });

      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(1);
    });

    it("does not call setPlanArtifact when matching session is not the active one", () => {
      mockActiveSessionId = "session-active";
      mockSessions = {
        "session-other": { id: "session-other", planArtifactId: "artifact-1" },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      // Tier 2 matched session-other but it's not active — no setPlanArtifact
      expect(mockSetPlanArtifact).not.toHaveBeenCalled();
      // But updateSession and invalidateQueries still fire for the matched session
      expect(mockUpdateSession).toHaveBeenCalledWith("session-other", {
        planArtifactId: "artifact-2",
        planUpdateSeq: 1,
      });
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["ideation", "session", "session-other"],
      });
    });
  });

  // ==========================================================================
  // Tier 3 — safety net
  // ==========================================================================

  describe("tier 3 — safety net invalidation", () => {
    it("neither tier matches → safety net invalidates active session query", () => {
      mockActiveSessionId = "session-active";
      mockSessions = {
        "session-active": { id: "session-active", planArtifactId: "artifact-unrelated" },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      expect(mockSetPlanArtifact).not.toHaveBeenCalled();
      expect(mockUpdateSession).not.toHaveBeenCalled();
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["ideation", "session", "session-active"],
      });
    });

    it("tier 3 does not fire when there is no active session", () => {
      mockActiveSessionId = null;
      mockSessions = {};

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      expect(mockSetPlanArtifact).not.toHaveBeenCalled();
      expect(mockInvalidateQueries).not.toHaveBeenCalled();
    });
  });

  // ==========================================================================
  // Dedup guard
  // ==========================================================================

  describe("dedup guard", () => {
    it("same updated event emitted twice → only processed once", () => {
      mockActiveSessionId = "session-1";

      renderHook(() => usePlanArtifactEvents());

      const payload = {
        sessionId: "session-1",
        artifactId: "artifact-2",
        previousArtifactId: "artifact-1",
        artifact: makeArtifact("artifact-2", 2),
      };

      act(() => {
        fireEvent("plan_artifact:updated", payload);
        fireEvent("plan_artifact:updated", payload);
      });

      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(1);
      expect(mockInvalidateQueries).toHaveBeenCalledTimes(3);
    });

    it("same created event emitted twice → only processed once", () => {
      mockActiveSessionId = "session-1";
      mockSessions = { "session-1": { id: "session-1", planArtifactId: null } };

      renderHook(() => usePlanArtifactEvents());

      const payload = {
        sessionId: "session-1",
        artifact: makeArtifact("artifact-1"),
      };

      act(() => {
        fireEvent("plan_artifact:created", payload);
        fireEvent("plan_artifact:created", payload);
      });

      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(1);
    });

    it("different artifact versions are processed separately (different dedup keys)", () => {
      mockActiveSessionId = "session-1";

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          sessionId: "session-1",
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
        fireEvent("plan_artifact:updated", {
          sessionId: "session-1",
          artifactId: "artifact-3",
          previousArtifactId: "artifact-2",
          artifact: makeArtifact("artifact-3", 3),
        });
      });

      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(2);
    });
  });

  // ==========================================================================
  // Tier 2 — inheritedPlanArtifactId fallback (followup sessions)
  // ==========================================================================

  describe("tier 2 — inheritedPlanArtifactId fallback", () => {
    it("followup session matches on inheritedPlanArtifactId === previousArtifactId → setPlanArtifact called", () => {
      mockActiveSessionId = "session-followup";
      mockSessions = {
        "session-followup": {
          id: "session-followup",
          planArtifactId: null,
          inheritedPlanArtifactId: "artifact-1",
        },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(1);
      expect(mockSetPlanArtifact).toHaveBeenCalledWith(
        expect.objectContaining({ id: "artifact-2" })
      );
    });

    it("match on inheritedPlanArtifactId updates inheritedPlanArtifactId — not planArtifactId", () => {
      mockActiveSessionId = "session-followup";
      mockSessions = {
        "session-followup": {
          id: "session-followup",
          planArtifactId: null,
          inheritedPlanArtifactId: "artifact-1",
        },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      expect(mockUpdateSession).toHaveBeenCalledWith("session-followup", {
        inheritedPlanArtifactId: "artifact-2",
        planUpdateSeq: 1,
      });
      expect(mockUpdateSession).not.toHaveBeenCalledWith(
        "session-followup",
        expect.objectContaining({ planArtifactId: expect.anything() })
      );
    });

    it("followup session matches on inheritedPlanArtifactId === artifactId (rapid event — already updated)", () => {
      mockActiveSessionId = "session-followup";
      mockSessions = {
        "session-followup": {
          id: "session-followup",
          planArtifactId: null,
          inheritedPlanArtifactId: "artifact-2",
        },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 3),
        });
      });

      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(1);
    });

    it("session with own planArtifactId matched on own field — does not update inheritedPlanArtifactId", () => {
      mockActiveSessionId = "session-1";
      mockSessions = {
        "session-1": {
          id: "session-1",
          planArtifactId: "artifact-1",
          inheritedPlanArtifactId: null,
        },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      expect(mockUpdateSession).toHaveBeenCalledWith("session-1", {
        planArtifactId: "artifact-2",
        planUpdateSeq: 1,
      });
      expect(mockUpdateSession).not.toHaveBeenCalledWith(
        "session-1",
        expect.objectContaining({ inheritedPlanArtifactId: expect.anything() })
      );
    });

    it("non-active followup session matched on inheritedPlanArtifactId → no setPlanArtifact, but updateSession and invalidateQueries fire", () => {
      mockActiveSessionId = "session-active";
      mockSessions = {
        "session-followup": {
          id: "session-followup",
          planArtifactId: null,
          inheritedPlanArtifactId: "artifact-1",
        },
      };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        fireEvent("plan_artifact:updated", {
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      expect(mockSetPlanArtifact).not.toHaveBeenCalled();
      expect(mockUpdateSession).toHaveBeenCalledWith("session-followup", {
        inheritedPlanArtifactId: "artifact-2",
        planUpdateSeq: 1,
      });
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["ideation", "session", "session-followup"],
      });
    });
  });

  // ==========================================================================
  // Rapid event sequences
  // ==========================================================================

  describe("rapid event sequences", () => {
    it("plan_artifact:created then plan_artifact:updated both processed correctly", () => {
      mockActiveSessionId = "session-1";
      mockSessions = { "session-1": { id: "session-1", planArtifactId: null } };

      renderHook(() => usePlanArtifactEvents());

      act(() => {
        // created uses key "created:artifact-1:1"
        fireEvent("plan_artifact:created", {
          sessionId: "session-1",
          artifact: makeArtifact("artifact-1"),
        });
        // updated uses key "updated:artifact-2:2" — different prefix, no dedup collision
        fireEvent("plan_artifact:updated", {
          sessionId: "session-1",
          artifactId: "artifact-2",
          previousArtifactId: "artifact-1",
          artifact: makeArtifact("artifact-2", 2),
        });
      });

      // Both events processed: different dedup key prefixes ("created:" vs "updated:")
      expect(mockSetPlanArtifact).toHaveBeenCalledTimes(2);
      expect(mockSetPlanArtifact).toHaveBeenLastCalledWith(
        expect.objectContaining({ id: "artifact-2" })
      );
    });
  });
});
