/**
 * useTaskEvents hook tests
 *
 * Covers the review badge invalidation logic in useEvents.task.ts:
 *   - REVIEW_STATUSES guard on both from/to in handleStatusChange
 *   - Both call sites: task:event/status_changed and legacy task:status_changed
 *   - `updated` event path with internal_status change
 *   - `deleted` event path
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

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
const mockEmit = vi.fn();

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
    emit: mockEmit,
  }),
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: mockInvalidateQueries,
  }),
}));

const mockAddTask = vi.fn();
const mockUpdateTask = vi.fn();
const mockRemoveTask = vi.fn();

vi.mock("@/stores/taskStore", () => ({
  useTaskStore: (selector: (s: unknown) => unknown) =>
    selector({
      addTask: mockAddTask,
      updateTask: mockUpdateTask,
      removeTask: mockRemoveTask,
    }),
}));

vi.mock("@/hooks/useTasks", () => ({
  taskKeys: {
    lists: () => ["tasks", "list"],
    detail: (id: string) => ["tasks", "detail", id],
  },
}));

vi.mock("@/hooks/useInfiniteTasksQuery", () => ({
  infiniteTaskKeys: { all: ["tasks", "infinite"] },
}));

vi.mock("@/hooks/useTaskStateTransitions", () => ({
  stateTransitionKeys: { task: (id: string) => ["stateTransitions", id] },
}));

vi.mock("@/hooks/useReviews", () => ({
  reviewKeys: { all: ["reviews"] },
}));

vi.mock("@/types/task", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/types/task")>();
  return { ...actual, transformTask: (task: unknown) => task };
});

// Import AFTER mocks are registered
import { useTaskEvents } from "./useEvents.task";

// ============================================================================
// Test helpers
// ============================================================================

// Valid UUIDs for use in event payloads (Zod requires uuid format)
const TASK_UUID = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

function hasReviewInvalidation(): boolean {
  return mockInvalidateQueries.mock.calls.some(
    ([arg]) => JSON.stringify(arg?.queryKey) === JSON.stringify(["reviews"]),
  );
}

// ============================================================================
// Tests
// ============================================================================

describe("useTaskEvents — review badge invalidation", () => {
  beforeEach(() => {
    subscriptions.clear();
    vi.clearAllMocks();
  });

  function setup() {
    renderHook(() => useTaskEvents());
  }

  // ──────────────────────────────────────────────────────────────────────────
  // status_changed via task:event (call site 1)
  // ──────────────────────────────────────────────────────────────────────────

  it("invalidates reviewKeys.all when `from` is in REVIEW_STATUSES (task:event/status_changed)", () => {
    setup();

    act(() => {
      fireEvent("task:event", {
        type: "status_changed",
        taskId: TASK_UUID,
        from: "revision_needed", // in REVIEW_STATUSES
        to: "executing",         // NOT in REVIEW_STATUSES
        changedBy: "system",
      });
    });

    expect(hasReviewInvalidation()).toBe(true);
  });

  it("invalidates reviewKeys.all when `to` is in REVIEW_STATUSES (task:event/status_changed)", () => {
    setup();

    act(() => {
      fireEvent("task:event", {
        type: "status_changed",
        taskId: TASK_UUID,
        from: "executing",       // NOT in REVIEW_STATUSES
        to: "pending_review",    // in REVIEW_STATUSES
        changedBy: "system",
      });
    });

    expect(hasReviewInvalidation()).toBe(true);
  });

  it("does NOT invalidate reviewKeys.all when neither from nor to is in REVIEW_STATUSES", () => {
    setup();

    act(() => {
      fireEvent("task:event", {
        type: "status_changed",
        taskId: TASK_UUID,
        from: "backlog",  // NOT in REVIEW_STATUSES
        to: "ready",      // NOT in REVIEW_STATUSES
        changedBy: "user",
      });
    });

    expect(hasReviewInvalidation()).toBe(false);
  });

  // ──────────────────────────────────────────────────────────────────────────
  // updated event with internal_status (call site: task:event/updated)
  // ──────────────────────────────────────────────────────────────────────────

  it("invalidates reviewKeys.all when `updated` event contains internal_status change", () => {
    setup();

    act(() => {
      fireEvent("task:event", {
        type: "updated",
        taskId: TASK_UUID,
        changes: {
          internal_status: "reviewing",
        },
      });
    });

    expect(hasReviewInvalidation()).toBe(true);
  });

  it("does NOT invalidate reviewKeys.all when `updated` event has no internal_status", () => {
    setup();

    act(() => {
      fireEvent("task:event", {
        type: "updated",
        taskId: TASK_UUID,
        changes: {
          title: "New title",
          priority: 80,
        },
      });
    });

    expect(hasReviewInvalidation()).toBe(false);
  });

  // ──────────────────────────────────────────────────────────────────────────
  // deleted event
  // ──────────────────────────────────────────────────────────────────────────

  it("invalidates reviewKeys.all unconditionally on `deleted` event", () => {
    setup();

    act(() => {
      fireEvent("task:event", {
        type: "deleted",
        taskId: TASK_UUID,
      });
    });

    expect(hasReviewInvalidation()).toBe(true);
  });

  // ──────────────────────────────────────────────────────────────────────────
  // Legacy task:status_changed path (call site 2)
  // ──────────────────────────────────────────────────────────────────────────

  it("invalidates reviewKeys.all via legacy task:status_changed when old_status is in REVIEW_STATUSES", () => {
    setup();

    act(() => {
      fireEvent("task:status_changed", {
        task_id: TASK_UUID,
        old_status: "reviewing",  // in REVIEW_STATUSES → passes as `from`
        new_status: "cancelled",  // NOT in REVIEW_STATUSES
      });
    });

    expect(hasReviewInvalidation()).toBe(true);
  });

  it("invalidates reviewKeys.all via legacy task:status_changed when new_status is in REVIEW_STATUSES", () => {
    setup();

    act(() => {
      fireEvent("task:status_changed", {
        task_id: TASK_UUID,
        old_status: "executing",      // NOT in REVIEW_STATUSES
        new_status: "pending_review", // in REVIEW_STATUSES → passes as `to`
      });
    });

    expect(hasReviewInvalidation()).toBe(true);
  });

  it("does NOT invalidate reviewKeys.all via legacy task:status_changed when neither status is review-related", () => {
    setup();

    act(() => {
      fireEvent("task:status_changed", {
        task_id: TASK_UUID,
        old_status: "executing",
        new_status: "merged",
      });
    });

    expect(hasReviewInvalidation()).toBe(false);
  });

  // ──────────────────────────────────────────────────────────────────────────
  // Coverage for all REVIEW_STATUSES values
  // ──────────────────────────────────────────────────────────────────────────

  it.each([
    "pending_review",
    "reviewing",
    "review_passed",
    "escalated",
    "revision_needed",
  ])(
    "invalidates reviewKeys.all when `to` is '%s'",
    (status) => {
      setup();

      act(() => {
        fireEvent("task:event", {
          type: "status_changed",
          taskId: TASK_UUID,
          from: "executing",
          to: status,
          changedBy: "system",
        });
      });

      expect(hasReviewInvalidation()).toBe(true);
    },
  );

  it("does NOT trigger review invalidation on merge completion path (pending_merge → merged)", () => {
    setup();

    act(() => {
      fireEvent("task:event", {
        type: "status_changed",
        taskId: TASK_UUID,
        from: "pending_merge", // NOT in REVIEW_STATUSES
        to: "merged",          // NOT in REVIEW_STATUSES
        changedBy: "system",
      });
    });

    expect(hasReviewInvalidation()).toBe(false);
  });
});
