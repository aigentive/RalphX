/**
 * Tests for useExecutionEvents hook
 *
 * Verifies that the hook correctly:
 * - Listens for execution:status_changed events
 * - Listens for execution:queue_changed events
 * - Updates the UI store with event data
 * - Cleans up listeners on unmount
 */

import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { createContext, useContext, type ReactNode } from "react";
import { useExecutionEvents } from "./useExecutionEvents";
import { useUiStore } from "@/stores/uiStore";
import { MockEventBus, type EventBus } from "@/lib/event-bus";

// Create a shared MockEventBus instance for testing
let testEventBus: MockEventBus;

// Test context that provides the mock event bus
const TestEventBusContext = createContext<EventBus | null>(null);

// Mock useEventBus to return our test event bus
vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => {
    const bus = useContext(TestEventBusContext);
    if (!bus) throw new Error("useEventBus must be used within TestEventBusProvider");
    return bus;
  },
}));

function TestEventBusProvider({ children }: { children: ReactNode }) {
  return (
    <TestEventBusContext.Provider value={testEventBus}>
      {children}
    </TestEventBusContext.Provider>
  );
}

// Event payload types
interface ExecutionStatusEvent {
  isPaused: boolean;
  haltMode?: "running" | "paused" | "stopped";
  runningCount: number;
  maxConcurrent: number;
  projectId?: string;
  reason: string;
  timestamp: string;
}

interface ExecutionQueueEvent {
  queuedCount: number;
  queuedMessageCount?: number;
  projectId?: string;
  timestamp: string;
}

// Wrapper component that provides minimal EventBus context for testing
const wrapper = ({ children }: { children: ReactNode }) => (
  <TestEventBusProvider>{children}</TestEventBusProvider>
);

describe("useExecutionEvents", () => {
  beforeEach(() => {
    // Create fresh event bus for each test
    testEventBus = new MockEventBus();

    // Reset store to initial state
    useUiStore.setState({
      executionStatus: {
        isPaused: false,
        haltMode: "running",
        runningCount: 0,
        maxConcurrent: 10,
        queuedCount: 0,
        canStartTask: true,
      },
    });
  });

  describe("status_changed event", () => {
    it("registers listener for execution:status_changed", () => {
      renderHook(() => useExecutionEvents(), { wrapper });

      // Verify listener was registered by checking listener count
      expect(testEventBus.getListenerCount("execution:status_changed")).toBe(1);
    });

    it("updates store isPaused on status_changed event", async () => {
      renderHook(() => useExecutionEvents(), { wrapper });

      // Emit event through the mock bus
      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: true,
          haltMode: "paused",
          runningCount: 1,
          maxConcurrent: 10,
          reason: "paused",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.isPaused).toBe(true);
      expect(state.haltMode).toBe("paused");
    });

    it("updates store runningCount on status_changed event", async () => {
      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: false,
          haltMode: "running",
          runningCount: 2,
          maxConcurrent: 3,
          reason: "task_started",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.runningCount).toBe(2);
    });

    it("updates store maxConcurrent on status_changed event", async () => {
      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: false,
          haltMode: "running",
          runningCount: 0,
          maxConcurrent: 5,
          reason: "config_changed",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.maxConcurrent).toBe(5);
    });

    it("calculates canStartTask correctly when not paused and under limit", async () => {
      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: false,
          haltMode: "running",
          runningCount: 1,
          maxConcurrent: 10,
          reason: "task_started",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.canStartTask).toBe(true);
    });

    it("calculates canStartTask correctly when paused", async () => {
      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: true,
          haltMode: "paused",
          runningCount: 0,
          maxConcurrent: 10,
          reason: "paused",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.canStartTask).toBe(false);
    });

    it("calculates canStartTask correctly when at max capacity", async () => {
      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: false,
          haltMode: "running",
          runningCount: 10,
          maxConcurrent: 10,
          reason: "task_started",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.canStartTask).toBe(false);
    });

    it("preserves queuedCount when status_changed event fires", async () => {
      // Set initial queuedCount
      useUiStore.setState({
        executionStatus: {
          ...useUiStore.getState().executionStatus,
          queuedCount: 5,
        },
      });

      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: false,
          haltMode: "running",
          runningCount: 1,
          maxConcurrent: 10,
          reason: "task_started",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.queuedCount).toBe(5);
    });

    it("preserves stopped halt mode when a generic status event omits it", async () => {
      useUiStore.setState({
        executionStatus: {
          ...useUiStore.getState().executionStatus,
          isPaused: true,
          haltMode: "stopped",
        },
      });

      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: true,
          runningCount: 0,
          maxConcurrent: 10,
          reason: "runtime_registry_gc",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.haltMode).toBe("stopped");
      expect(state.isPaused).toBe(true);
    });

    it("ignores status events outside the scoped project override", async () => {
      renderHook(() => useExecutionEvents("project-2"), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: false,
          haltMode: "running",
          runningCount: 7,
          maxConcurrent: 10,
          projectId: "project-1",
          reason: "task_started",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.runningCount).toBe(0);
    });

    it("accepts status events matching the scoped project override", async () => {
      renderHook(() => useExecutionEvents("project-2"), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionStatusEvent>("execution:status_changed", {
          isPaused: false,
          haltMode: "running",
          runningCount: 4,
          maxConcurrent: 10,
          projectId: "project-2",
          reason: "task_started",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.runningCount).toBe(4);
    });
  });

  describe("queue_changed event", () => {
    it("registers listener for execution:queue_changed", () => {
      renderHook(() => useExecutionEvents(), { wrapper });

      // Verify listener was registered by checking listener count
      expect(testEventBus.getListenerCount("execution:queue_changed")).toBe(1);
    });

    it("updates only queuedCount on queue_changed event", async () => {
      // Set initial state
      useUiStore.setState({
        executionStatus: {
          isPaused: false,
          haltMode: "running",
          runningCount: 1,
          maxConcurrent: 10,
          queuedCount: 0,
          canStartTask: true,
        },
      });

      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionQueueEvent>("execution:queue_changed", {
          queuedCount: 3,
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      // queuedCount should be updated
      expect(state.queuedCount).toBe(3);
      // Other fields should be preserved
      expect(state.isPaused).toBe(false);
      expect(state.runningCount).toBe(1);
      expect(state.maxConcurrent).toBe(10);
      expect(state.canStartTask).toBe(true);
    });

    it("handles queuedCount decrement", async () => {
      useUiStore.setState({
        executionStatus: {
          ...useUiStore.getState().executionStatus,
          queuedCount: 5,
        },
      });

      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionQueueEvent>("execution:queue_changed", {
          queuedCount: 4,
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.queuedCount).toBe(4);
    });

    it("handles queuedCount set to zero", async () => {
      useUiStore.setState({
        executionStatus: {
          ...useUiStore.getState().executionStatus,
          queuedCount: 3,
        },
      });

      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionQueueEvent>("execution:queue_changed", {
          queuedCount: 0,
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.queuedCount).toBe(0);
    });

    it("updates queuedMessageCount when queue_changed includes it", async () => {
      useUiStore.setState({
        executionStatus: {
          ...useUiStore.getState().executionStatus,
          queuedCount: 1,
          queuedMessageCount: 0,
        },
      });

      renderHook(() => useExecutionEvents(), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionQueueEvent>("execution:queue_changed", {
          queuedCount: 1,
          queuedMessageCount: 3,
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.queuedCount).toBe(1);
      expect(state.queuedMessageCount).toBe(3);
    });

    it("ignores queue events outside the scoped project override", async () => {
      renderHook(() => useExecutionEvents("project-2"), { wrapper });

      await act(async () => {
        testEventBus.emit<ExecutionQueueEvent>("execution:queue_changed", {
          queuedCount: 6,
          projectId: "project-1",
          timestamp: new Date().toISOString(),
        });
      });

      const state = useUiStore.getState().executionStatus;
      expect(state.queuedCount).toBe(0);
    });
  });

  describe("cleanup on unmount", () => {
    it("cleans up status_changed listener on unmount", () => {
      const { unmount } = renderHook(() => useExecutionEvents(), { wrapper });

      // Verify listener exists
      expect(testEventBus.getListenerCount("execution:status_changed")).toBe(1);

      unmount();

      // Verify listener was cleaned up
      expect(testEventBus.getListenerCount("execution:status_changed")).toBe(0);
    });

    it("cleans up queue_changed listener on unmount", () => {
      const { unmount } = renderHook(() => useExecutionEvents(), { wrapper });

      // Verify listener exists
      expect(testEventBus.getListenerCount("execution:queue_changed")).toBe(1);

      unmount();

      // Verify listener was cleaned up
      expect(testEventBus.getListenerCount("execution:queue_changed")).toBe(0);
    });

    it("cleans up both listeners on unmount", () => {
      const { unmount } = renderHook(() => useExecutionEvents(), { wrapper });

      // Verify both listeners exist
      expect(testEventBus.getListenerCount("execution:status_changed")).toBe(1);
      expect(testEventBus.getListenerCount("execution:queue_changed")).toBe(1);

      unmount();

      // Verify both listeners were cleaned up
      expect(testEventBus.getListenerCount("execution:status_changed")).toBe(0);
      expect(testEventBus.getListenerCount("execution:queue_changed")).toBe(0);
    });
  });
});
