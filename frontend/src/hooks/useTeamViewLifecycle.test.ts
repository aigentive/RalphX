/**
 * useTeamViewLifecycle hook tests
 *
 * Tests auto-switch to/from team view on lifecycle events:
 *   team:created → save current view, switch to "team"
 *   team:disbanded → restore previous view, reset splitPaneStore
 *
 * Verifies no-op when already on team view, no restore when user navigated away,
 * and splitPaneStore contextKey management.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useUiStore } from "@/stores/uiStore";
import { useSplitPaneStore } from "@/stores/splitPaneStore";

// ============================================================================
// Mock infrastructure — capture EventBus subscriptions
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
  }),
}));

vi.mock("@/lib/chat-context-registry", () => ({
  buildStoreKey: (contextType: string, contextId: string) => {
    const prefixes: Record<string, string> = {
      task_execution: "task_execution",
      ideation: "session",
      task: "task",
    };
    return `${prefixes[contextType] ?? contextType}:${contextId}`;
  },
}));

// ============================================================================
// Import hook under test (after mocks)
// ============================================================================

import { useTeamViewLifecycle } from "./useTeamViewLifecycle";

// ============================================================================
// Helpers
// ============================================================================

const CONTEXT_KEY = "task_execution:task-abc";
const CONTEXT_TYPE = "task_execution";
const CONTEXT_ID = "task-abc";

function makePayload(overrides?: Record<string, unknown>) {
  return {
    context_type: CONTEXT_TYPE,
    context_id: CONTEXT_ID,
    ...overrides,
  };
}

// ============================================================================
// Tests
// ============================================================================

describe("useTeamViewLifecycle", () => {
  beforeEach(() => {
    subscriptions.clear();
    // Reset stores
    useUiStore.getState().setCurrentView("kanban");
    useUiStore.getState().setPreviousView(null);
    useSplitPaneStore.getState().reset();
  });

  // --------------------------------------------------------------------------
  // 1. No subscriptions when contextKey is null
  // --------------------------------------------------------------------------
  it("should not subscribe when contextKey is null", () => {
    renderHook(() => useTeamViewLifecycle(null));
    expect(subscriptions.size).toBe(0);
  });

  // --------------------------------------------------------------------------
  // 2. team:created → switch to team view, save previous
  // --------------------------------------------------------------------------
  it("should switch to team view and save previous view on team:created", () => {
    useUiStore.getState().setCurrentView("kanban");
    renderHook(() => useTeamViewLifecycle(CONTEXT_KEY));

    act(() => {
      fireEvent("team:created", {
        ...makePayload(),
        team_name: "my-team",
      });
    });

    expect(useUiStore.getState().currentView).toBe("team");
    expect(useUiStore.getState().previousView).toBe("agents");
  });

  // --------------------------------------------------------------------------
  // 3. team:created → sets splitPaneStore contextKey
  // --------------------------------------------------------------------------
  it("should set splitPaneStore contextKey on team:created", () => {
    renderHook(() => useTeamViewLifecycle(CONTEXT_KEY));

    act(() => {
      fireEvent("team:created", {
        ...makePayload(),
        team_name: "my-team",
      });
    });

    expect(useSplitPaneStore.getState().contextKey).toBe(CONTEXT_KEY);
  });

  // --------------------------------------------------------------------------
  // 4. team:created → no-op if already on team view
  // --------------------------------------------------------------------------
  it("should not overwrite previousView if already on team view", () => {
    useUiStore.getState().setCurrentView("team");
    useUiStore.getState().setPreviousView("graph");
    renderHook(() => useTeamViewLifecycle(CONTEXT_KEY));

    act(() => {
      fireEvent("team:created", {
        ...makePayload(),
        team_name: "my-team",
      });
    });

    // previousView should remain "graph", not overwritten to "team"
    expect(useUiStore.getState().previousView).toBe("graph");
    expect(useUiStore.getState().currentView).toBe("team");
  });

  // --------------------------------------------------------------------------
  // 5. team:disbanded → restore previous view
  // --------------------------------------------------------------------------
  it("should restore previous view on team:disbanded", () => {
    useUiStore.getState().setCurrentView("kanban");
    renderHook(() => useTeamViewLifecycle(CONTEXT_KEY));

    // Create → switch to team
    act(() => {
      fireEvent("team:created", { ...makePayload(), team_name: "t" });
    });
    expect(useUiStore.getState().currentView).toBe("team");

    // Disband -> restore the normalized Agents view
    act(() => {
      fireEvent("team:disbanded", makePayload());
    });

    expect(useUiStore.getState().currentView).toBe("agents");
    expect(useUiStore.getState().previousView).toBeNull();
  });

  // --------------------------------------------------------------------------
  // 6. team:disbanded → reset splitPaneStore
  // --------------------------------------------------------------------------
  it("should reset splitPaneStore on team:disbanded", () => {
    renderHook(() => useTeamViewLifecycle(CONTEXT_KEY));

    // Set some splitPane state
    useSplitPaneStore.getState().setContextKey(CONTEXT_KEY);
    useSplitPaneStore.getState().setFocusedPane("worker-1");

    act(() => {
      fireEvent("team:created", { ...makePayload(), team_name: "t" });
    });
    act(() => {
      fireEvent("team:disbanded", makePayload());
    });

    expect(useSplitPaneStore.getState().contextKey).toBeNull();
    expect(useSplitPaneStore.getState().focusedPane).toBeNull();
  });

  // --------------------------------------------------------------------------
  // 7. team:disbanded → no view change if user navigated away from team
  // --------------------------------------------------------------------------
  it("should not change view on disband if user already navigated away", () => {
    useUiStore.getState().setCurrentView("kanban");
    renderHook(() => useTeamViewLifecycle(CONTEXT_KEY));

    // Create team → switch to team view
    act(() => {
      fireEvent("team:created", { ...makePayload(), team_name: "t" });
    });
    expect(useUiStore.getState().currentView).toBe("team");

    // User manually navigates to deprecated graph, which normalizes to Agents
    useUiStore.getState().setCurrentView("graph");

    // Disband → should NOT restore since user isn't on team view
    act(() => {
      fireEvent("team:disbanded", makePayload());
    });

    expect(useUiStore.getState().currentView).toBe("agents");
  });

  // --------------------------------------------------------------------------
  // 8. Ignores events with non-matching context
  // --------------------------------------------------------------------------
  it("should ignore events with non-matching context", () => {
    useUiStore.getState().setCurrentView("kanban");
    renderHook(() => useTeamViewLifecycle(CONTEXT_KEY));

    act(() => {
      fireEvent("team:created", {
        context_type: "task_execution",
        context_id: "other-task",
        team_name: "other-team",
      });
    });

    // Should remain on normalized Agents — event didn't match
    expect(useUiStore.getState().currentView).toBe("agents");
    expect(useUiStore.getState().previousView).toBeNull();
  });
});
