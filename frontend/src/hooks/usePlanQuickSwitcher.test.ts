/**
 * Tests for usePlanQuickSwitcher hook
 *
 * Tests:
 * - Item indexing logic (getItemAtIndex, getTotalItemCount)
 * - Keyboard navigation across item types
 * - Blocking behavior when quickActionFlow.isBlocking
 * - Handlers (select, clear, retry)
 * - Derived state (sortedCandidates, showQuickAction)
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { usePlanQuickSwitcher } from "./usePlanQuickSwitcher";
import type { PlanCandidate } from "@/stores/planStore";
import type { QuickAction, UseQuickActionFlowReturn } from "./useQuickActionFlow";

// ============================================================================
// Mocks
// ============================================================================

const mockLoadCandidates = vi.fn();
const mockSetActivePlan = vi.fn();
const mockClearActivePlan = vi.fn();


// Mock planStore
vi.mock("@/stores/planStore", () => ({
  usePlanStore: vi.fn((selector) => {
    const state = {
      activePlanByProject: { "project-1": "active-plan-1" },
      planCandidates: mockPlanCandidates,
      isLoading: false,
      error: null,
      loadCandidates: mockLoadCandidates,
      setActivePlan: mockSetActivePlan,
      clearActivePlan: mockClearActivePlan,
    };
    return selector ? selector(state) : state;
  }),
}));

// Mock hooks
const mockQuickAction: QuickAction = {
  id: "agent-conversation",
  label: "Start new agent conversation",
  icon: vi.fn() as unknown as QuickAction["icon"],
  description: (query: string) => `"${query}"`,
  isVisible: (query: string) => query.trim().length > 0,
  execute: vi.fn(),
  requiresConfirmation: false,
  creatingLabel: "Creating...",
  successLabel: "Created!",
  viewLabel: "View",
  navigateTo: vi.fn(),
};

const mockQuickActionFlow: UseQuickActionFlowReturn = {
  flowState: "idle",
  createdEntityId: null,
  error: null,
  startConfirmation: vi.fn(),
  confirm: vi.fn(),
  cancel: vi.fn(),
  viewEntity: vi.fn(),
  dismiss: vi.fn(),
  isBlocking: false,
};

vi.mock("./useAgentConversationQuickAction", () => ({
  useAgentConversationQuickAction: vi.fn(() => mockQuickAction),
}));

vi.mock("./useQuickActionFlow", () => ({
  useQuickActionFlow: vi.fn(() => mockQuickActionFlow),
}));

vi.mock("./usePlanCandidateSort", () => ({
  usePlanCandidateSort: vi.fn((candidates) => candidates),
}));

// Sample plan candidates
const mockPlanCandidates: PlanCandidate[] = [
  {
    sessionId: "plan-1",
    title: "Feature A",
    acceptedAt: "2026-02-14T10:00:00Z",
    taskStats: { total: 5, incomplete: 2, activeNow: 1 },
    interactionStats: { selectedCount: 3, lastSelectedAt: "2026-02-13T10:00:00Z" },
    score: 0.9,
  },
  {
    sessionId: "plan-2",
    title: "Feature B",
    acceptedAt: "2026-02-13T10:00:00Z",
    taskStats: { total: 3, incomplete: 1, activeNow: 0 },
    interactionStats: { selectedCount: 1, lastSelectedAt: "2026-02-12T10:00:00Z" },
    score: 0.7,
  },
];

// ============================================================================
// Tests
// ============================================================================

describe("usePlanQuickSwitcher", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset mock quick action flow state
    mockQuickActionFlow.flowState = "idle";
    mockQuickActionFlow.isBlocking = false;
  });

  describe("Item indexing logic", () => {
    it("should calculate total item count correctly with all items visible", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("Feature"); // Matches "Feature A" and "Feature B"
      });

      // Quick action (index 0) + Clear (index 1) + 2 candidates = 4 total
      expect(result.current.getTotalItemCount()).toBe(4);
    });

    it("should calculate total item count without clear action", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: false,
        })
      );

      act(() => {
        result.current.setSearchQuery("Feature"); // Matches "Feature A" and "Feature B"
      });

      // Quick action (index 0) + 2 candidates = 3 total
      expect(result.current.getTotalItemCount()).toBe(3);
    });

    it("should calculate total item count without quick action", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      // Hide quick action by setting empty search query
      act(() => {
        result.current.setSearchQuery("");
      });

      // Clear (index 0) + 2 candidates = 3 total (no quick action when query is empty)
      expect(result.current.getTotalItemCount()).toBe(3);
    });

    it("should get quick action item at index 0 when visible", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("test");
      });

      const item = result.current.getItemAtIndex(0);
      expect(item.type).toBe("quick-action");
    });

    it("should get clear item at correct index with quick action visible", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("test");
      });

      // With quick action: clear is at index 1
      const item = result.current.getItemAtIndex(1);
      expect(item.type).toBe("clear");
    });

    it("should get clear item at index 0 when quick action not visible", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("");
      });

      // Without quick action: clear is at index 0
      const item = result.current.getItemAtIndex(0);
      expect(item.type).toBe("clear");
    });

    it("should get candidate items at correct indices", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("Feature"); // Matches both candidates
      });

      // With quick action and clear: candidates start at index 2
      const item1 = result.current.getItemAtIndex(2);
      expect(item1.type).toBe("candidate");
      if (item1.type === "candidate") {
        expect(item1.candidate.sessionId).toBe("plan-1");
      }

      const item2 = result.current.getItemAtIndex(3);
      expect(item2.type).toBe("candidate");
      if (item2.type === "candidate") {
        expect(item2.candidate.sessionId).toBe("plan-2");
      }
    });

    it("should return quick-action for out of bounds index", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("test");
      });

      const item = result.current.getItemAtIndex(999);
      expect(item.type).toBe("quick-action");
    });
  });

  describe("Derived state", () => {
    it("should filter candidates by search query", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
        })
      );

      act(() => {
        result.current.setSearchQuery("Feature A");
      });

      expect(result.current.filteredCandidates).toHaveLength(1);
      expect(result.current.filteredCandidates[0].sessionId).toBe("plan-1");
    });

    it("should show all candidates when search query is empty", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
        })
      );

      act(() => {
        result.current.setSearchQuery("");
      });

      expect(result.current.filteredCandidates).toHaveLength(2);
    });

    it("should set canClearPlan to true when showClearAction and active plan exists", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      expect(result.current.canClearPlan).toBe(true);
    });

    it("should set canClearPlan to false when showClearAction is false", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: false,
        })
      );

      expect(result.current.canClearPlan).toBe(false);
    });

    it("should show quick action when query is non-empty and flow is idle", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
        })
      );

      act(() => {
        result.current.setSearchQuery("test");
      });

      expect(result.current.showQuickAction).toBe(true);
    });

    it("should hide quick action when query is empty", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
        })
      );

      act(() => {
        result.current.setSearchQuery("");
      });

      expect(result.current.showQuickAction).toBe(false);
    });

    it("should hide quick action when flow is not idle", () => {
      mockQuickActionFlow.flowState = "confirming";
      mockQuickActionFlow.isBlocking = true;

      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
        })
      );

      act(() => {
        result.current.setSearchQuery("test");
      });

      expect(result.current.showQuickAction).toBe(false);
    });
  });

  describe("Keyboard navigation", () => {
    it("should handle ArrowDown to increment highlighted index", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("test");
      });

      const event = new KeyboardEvent("keydown", { key: "ArrowDown" });
      Object.defineProperty(event, "preventDefault", {
        value: vi.fn(),
        writable: true,
      });

      act(() => {
        result.current.handleKeyDown(event as unknown as React.KeyboardEvent);
      });

      expect(result.current.highlightedIndex).toBe(1);
    });

    it("should handle ArrowUp to decrement highlighted index", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      // Manually set highlighted index to test ArrowUp
      act(() => {
        result.current.setSearchQuery("Feature");
      });

      // Move down twice
      act(() => {
        result.current.handleKeyDown({
          key: "ArrowDown",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      act(() => {
        result.current.handleKeyDown({
          key: "ArrowDown",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(result.current.highlightedIndex).toBe(2);

      // Move up once
      act(() => {
        result.current.handleKeyDown({
          key: "ArrowUp",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(result.current.highlightedIndex).toBe(1);
    });

    it("should handle Home to jump to first item", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("Feature");
      });

      // Move to last item
      act(() => {
        result.current.handleKeyDown({
          key: "End",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(result.current.highlightedIndex).toBe(3); // Last item

      // Jump back to first
      act(() => {
        result.current.handleKeyDown({
          key: "Home",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(result.current.highlightedIndex).toBe(0);
    });

    it("should handle End to jump to last item", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("Feature");
      });

      act(() => {
        result.current.handleKeyDown({
          key: "End",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(result.current.highlightedIndex).toBe(3); // Last item (quick-action + clear + 2 candidates)
    });

    it("should handle Escape to close", () => {
      const onClose = vi.fn();
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose,
        })
      );

      act(() => {
        result.current.handleKeyDown({
          key: "Escape",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(onClose).toHaveBeenCalled();
    });

    it("should block all navigation except Escape when quickActionFlow is blocking", () => {
      mockQuickActionFlow.isBlocking = true;
      const onClose = vi.fn();

      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose,
        })
      );

      // Try ArrowDown - should not change index
      act(() => {
        result.current.handleKeyDown({
          key: "ArrowDown",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });
      expect(result.current.highlightedIndex).toBe(0);

      // Try Enter - should not trigger selection
      act(() => {
        result.current.handleKeyDown({
          key: "Enter",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });
      expect(mockSetActivePlan).not.toHaveBeenCalled();

      // Escape should still work and call cancel
      act(() => {
        result.current.handleKeyDown({
          key: "Escape",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });
      expect(mockQuickActionFlow.cancel).toHaveBeenCalled();
    });
  });

  describe("Enter key handling", () => {
    it("should execute the agent conversation quick action directly when Enter is pressed", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("Feature");
      });

      // Highlighted index starts at 0 (quick action)
      act(() => {
        result.current.handleKeyDown({
          key: "Enter",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(mockQuickActionFlow.confirm).toHaveBeenCalledWith("Feature");
      expect(mockQuickActionFlow.startConfirmation).not.toHaveBeenCalled();
    });

    it("should call handleClear when Enter pressed on clear item", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("Feature");
      });

      // Move to clear item (index 1)
      act(() => {
        result.current.handleKeyDown({
          key: "ArrowDown",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      act(() => {
        result.current.handleKeyDown({
          key: "Enter",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(mockClearActivePlan).toHaveBeenCalledWith("project-1");
    });

    it("should call handleSelect when Enter pressed on candidate item", async () => {
      const onClose = vi.fn();
      mockSetActivePlan.mockResolvedValue(undefined);

      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose,
          showClearAction: true,
        })
      );

      act(() => {
        result.current.setSearchQuery("Feature");
      });

      // Move to first candidate (index 2)
      act(() => {
        result.current.handleKeyDown({
          key: "ArrowDown",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      act(() => {
        result.current.handleKeyDown({
          key: "ArrowDown",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      await act(async () => {
        result.current.handleKeyDown({
          key: "Enter",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      await waitFor(() => {
        expect(mockSetActivePlan).toHaveBeenCalledWith(
          "project-1",
          "plan-1",
          "quick_switcher"
        );
      });
    });
  });

  describe("Handlers", () => {
    it("should call setActivePlan and close on handleSelect", async () => {
      const onClose = vi.fn();
      mockSetActivePlan.mockResolvedValue(undefined);

      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose,
          selectionSource: "kanban_inline",
        })
      );

      await act(async () => {
        await result.current.handleSelect("plan-2");
      });

      expect(mockSetActivePlan).toHaveBeenCalledWith(
        "project-1",
        "plan-2",
        "kanban_inline"
      );
      expect(onClose).toHaveBeenCalled();
    });

    it("should call clearActivePlan and close on handleClear", async () => {
      const onClose = vi.fn();
      mockClearActivePlan.mockResolvedValue(undefined);

      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose,
        })
      );

      await act(async () => {
        await result.current.handleClear();
      });

      expect(mockClearActivePlan).toHaveBeenCalledWith("project-1");
      expect(onClose).toHaveBeenCalled();
    });

    it("should call loadCandidates on handleRetry", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
        })
      );

      act(() => {
        result.current.handleRetry();
      });

      expect(mockLoadCandidates).toHaveBeenCalledWith("project-1");
    });

    it("should not close on handleSelect error", async () => {
      const onClose = vi.fn();
      const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
      mockSetActivePlan.mockRejectedValue(new Error("Failed to set plan"));

      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose,
        })
      );

      await act(async () => {
        await result.current.handleSelect("plan-2");
      });

      expect(onClose).not.toHaveBeenCalled();
      expect(consoleError).toHaveBeenCalled();

      consoleError.mockRestore();
    });

    it("should not close on handleClear error", async () => {
      const onClose = vi.fn();
      const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
      mockClearActivePlan.mockRejectedValue(new Error("Failed to clear plan"));

      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose,
        })
      );

      await act(async () => {
        await result.current.handleClear();
      });

      expect(onClose).not.toHaveBeenCalled();
      expect(consoleError).toHaveBeenCalled();

      consoleError.mockRestore();
    });
  });

  describe("Effects and lifecycle", () => {
    it("should load candidates when opened", () => {
      renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
        })
      );

      expect(mockLoadCandidates).toHaveBeenCalledWith("project-1");
    });

    it("should not load candidates when closed", () => {
      renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: false,
          onClose: vi.fn(),
        })
      );

      expect(mockLoadCandidates).not.toHaveBeenCalled();
    });

    it("should reset search query and highlighted index when closed", () => {
      const { result, rerender } = renderHook(
        ({ isOpen }) =>
          usePlanQuickSwitcher({
            projectId: "project-1",
            isOpen,
            onClose: vi.fn(),
          }),
        { initialProps: { isOpen: true } }
      );

      act(() => {
        result.current.setSearchQuery("test");
      });

      act(() => {
        result.current.handleKeyDown({
          key: "ArrowDown",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(result.current.searchQuery).toBe("test");
      expect(result.current.highlightedIndex).toBe(1);

      // Close
      rerender({ isOpen: false });

      expect(result.current.searchQuery).toBe("");
      expect(result.current.highlightedIndex).toBe(0);
    });

    it("should reset highlighted index when search query changes", () => {
      const { result } = renderHook(() =>
        usePlanQuickSwitcher({
          projectId: "project-1",
          isOpen: true,
          onClose: vi.fn(),
        })
      );

      act(() => {
        result.current.setSearchQuery("test");
      });

      act(() => {
        result.current.handleKeyDown({
          key: "ArrowDown",
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent);
      });

      expect(result.current.highlightedIndex).toBe(1);

      act(() => {
        result.current.setSearchQuery("new query");
      });

      expect(result.current.highlightedIndex).toBe(0);
    });
  });
});
