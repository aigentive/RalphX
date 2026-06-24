/**
 * usePlanQuickSwitcher - Orchestration hook for PlanQuickSwitcherPalette
 *
 * Extracts ALL state, effects, and handlers from the component.
 * Provides clean separation of concerns: hook manages logic, component renders UI.
 *
 * Features:
 * - Dynamic item indexing (quick-action | clear | candidates)
 * - Keyboard navigation with blocking support
 * - Quick action flow integration
 * - Auto-focus, auto-load, auto-reset lifecycle
 * - Anchor centering calculation
 * - Click-outside-to-close detection
 */

import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { usePlanStore } from "@/stores/planStore";
import { useAgentConversationQuickAction } from "./useAgentConversationQuickAction";
import { useQuickActionFlow } from "./useQuickActionFlow";
import { usePlanCandidateSort } from "./usePlanCandidateSort";
import type { PlanCandidate } from "@/stores/planStore";
import type { SelectionSource } from "@/api/plan";
import type { QuickAction, UseQuickActionFlowReturn } from "./useQuickActionFlow";

// ============================================================================
// Types
// ============================================================================

export interface UsePlanQuickSwitcherProps {
  projectId: string;
  isOpen: boolean;
  onClose: () => void;
  selectionSource?: SelectionSource;
  showClearAction?: boolean;
  anchorSelector?: string;
}

export type PaletteItem =
  | { type: "quick-action" }
  | { type: "clear" }
  | { type: "candidate"; candidate: PlanCandidate };

export interface UsePlanQuickSwitcherReturn {
  // State
  searchQuery: string;
  setSearchQuery: (query: string) => void;
  highlightedIndex: number;
  anchorCenterX: number | null;

  // Refs
  inputRef: React.RefObject<HTMLInputElement | null>;
  containerRef: React.RefObject<HTMLDivElement | null>;
  highlightedItemRef: React.RefObject<HTMLButtonElement | null>;

  // Store state
  activePlanId: string | null;
  planCandidates: PlanCandidate[];
  isLoading: boolean;
  error: string | null;

  // Derived state
  sortedCandidates: PlanCandidate[];
  filteredCandidates: PlanCandidate[];
  canClearPlan: boolean;
  showQuickAction: boolean;

  // Quick action
  quickAction: QuickAction;
  quickActionFlow: UseQuickActionFlowReturn;

  // Helpers
  getItemAtIndex: (index: number) => PaletteItem;
  getTotalItemCount: () => number;

  // Handlers
  handleKeyDown: (e: React.KeyboardEvent) => void;
  handleSelect: (sessionId: string) => Promise<void>;
  handleClear: () => Promise<void>;
  handleQuickActionSelect: () => void;
  handleRetry: () => void;
}

// ============================================================================
// Hook Implementation
// ============================================================================

export function usePlanQuickSwitcher({
  projectId,
  isOpen,
  onClose,
  selectionSource = "quick_switcher",
  showClearAction = true,
  anchorSelector,
}: UsePlanQuickSwitcherProps): UsePlanQuickSwitcherReturn {
  // ============================================================================
  // State
  // ============================================================================

  const [searchQuery, setSearchQuery] = useState("");
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const [anchorCenterX, setAnchorCenterX] = useState<number | null>(null);

  // ============================================================================
  // Refs
  // ============================================================================

  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const highlightedItemRef = useRef<HTMLButtonElement>(null);

  // ============================================================================
  // Store State
  // ============================================================================

  const activePlanId = usePlanStore((state) => state.activePlanByProject[projectId] ?? null);
  const planCandidates = usePlanStore((state) => state.planCandidates);
  const isLoading = usePlanStore((state) => state.isLoading);
  const error = usePlanStore((state) => state.error);
  const loadCandidates = usePlanStore((state) => state.loadCandidates);
  const setActivePlan = usePlanStore((state) => state.setActivePlan);
  const clearActivePlan = usePlanStore((state) => state.clearActivePlan);

  // ============================================================================
  // Quick Action Integration
  // ============================================================================

  const quickAction = useAgentConversationQuickAction(projectId, { onClose });
  const quickActionFlow = useQuickActionFlow(quickAction);

  // ============================================================================
  // Derived State
  // ============================================================================

  const sortedCandidates = usePlanCandidateSort(planCandidates);

  const filteredCandidates = useMemo(() => {
    if (!searchQuery) return sortedCandidates;
    const query = searchQuery.toLowerCase();
    return sortedCandidates.filter((plan) =>
      (plan.title || "Untitled Plan").toLowerCase().includes(query)
    );
  }, [searchQuery, sortedCandidates]);

  const canClearPlan = showClearAction && Boolean(activePlanId);

  const showQuickAction =
    quickAction.isVisible(searchQuery) && quickActionFlow.flowState === "idle";

  // ============================================================================
  // Item Indexing Helpers
  // ============================================================================

  const getTotalItemCount = useCallback((): number => {
    let count = 0;
    if (showQuickAction || quickActionFlow.flowState !== "idle") count++;
    if (canClearPlan) count++;
    count += filteredCandidates.length;
    return count;
  }, [showQuickAction, quickActionFlow.flowState, canClearPlan, filteredCandidates.length]);

  const getItemAtIndex = useCallback(
    (index: number): PaletteItem => {
      let currentIndex = 0;

      // Quick action is at index 0 when visible OR when flow is active
      if (showQuickAction || quickActionFlow.flowState !== "idle") {
        if (index === currentIndex) {
          return { type: "quick-action" };
        }
        currentIndex++;
      }

      // Clear action is next (if enabled)
      if (canClearPlan) {
        if (index === currentIndex) {
          return { type: "clear" };
        }
        currentIndex++;
      }

      // Candidates fill remaining indices
      const candidateIndex = index - currentIndex;
      const candidate = filteredCandidates[candidateIndex];
      if (candidateIndex >= 0 && candidate) {
        return {
          type: "candidate",
          candidate,
        };
      }

      // Fallback for out-of-bounds
      return { type: "quick-action" };
    },
    [showQuickAction, quickActionFlow.flowState, canClearPlan, filteredCandidates]
  );

  // ============================================================================
  // Handlers
  // ============================================================================

  const handleSelect = useCallback(
    async (sessionId: string) => {
      try {
        await setActivePlan(projectId, sessionId, selectionSource);
        onClose();
      } catch (error) {
        console.error("Failed to set active plan:", error);
      }
    },
    [projectId, setActivePlan, onClose, selectionSource]
  );

  const handleClear = useCallback(async () => {
    try {
      await clearActivePlan(projectId);
      onClose();
    } catch (error) {
      console.error("Failed to clear active plan:", error);
    }
  }, [clearActivePlan, onClose, projectId]);

  const handleRetry = useCallback(() => {
    loadCandidates(projectId);
  }, [projectId, loadCandidates]);

  const handleQuickActionSelect = useCallback(() => {
    if (quickAction.requiresConfirmation === false) {
      void quickActionFlow.confirm(searchQuery);
      return;
    }
    quickActionFlow.startConfirmation();
  }, [quickAction.requiresConfirmation, quickActionFlow, searchQuery]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      // When quick action flow is blocking, only Escape works
      if (quickActionFlow.isBlocking) {
        if (e.key === "Escape") {
          e.preventDefault();
          quickActionFlow.cancel();
        }
        return;
      }

      const itemCount = getTotalItemCount();

      // Prevent navigation if no interactive rows
      if (itemCount === 0 && ["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
        return;
      }

      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          if (e.shiftKey) {
            setHighlightedIndex(itemCount - 1);
          } else {
            setHighlightedIndex((i) => Math.min(i + 1, itemCount - 1));
          }
          break;

        case "ArrowUp":
          e.preventDefault();
          if (e.shiftKey) {
            setHighlightedIndex(0);
          } else {
            setHighlightedIndex((i) => Math.max(i - 1, 0));
          }
          break;

        case "Home":
          e.preventDefault();
          setHighlightedIndex(0);
          break;

        case "End":
          e.preventDefault();
          setHighlightedIndex(itemCount - 1);
          break;

        case "Enter": {
          e.preventDefault();
          const item = getItemAtIndex(highlightedIndex);

          switch (item.type) {
            case "quick-action":
              handleQuickActionSelect();
              break;

            case "clear":
              handleClear();
              break;

            case "candidate":
              handleSelect(item.candidate.sessionId);
              break;
          }
          break;
        }

        case "Escape":
          e.preventDefault();
          onClose();
          break;
      }
    },
    [
      quickActionFlow,
      getTotalItemCount,
      getItemAtIndex,
      highlightedIndex,
      handleClear,
      handleQuickActionSelect,
      handleSelect,
      onClose,
    ]
  );

  // ============================================================================
  // Effects
  // ============================================================================

  // Auto-focus input when opened
  useEffect(() => {
    if (isOpen && inputRef.current) {
      inputRef.current.focus();
    }
  }, [isOpen]);

  // Load candidates when opened
  useEffect(() => {
    if (isOpen) {
      loadCandidates(projectId);
    }
  }, [isOpen, projectId, loadCandidates]);

  // Reset state when closed
  useEffect(() => {
    if (!isOpen) {
      setSearchQuery("");
      setHighlightedIndex(0);
    }
  }, [isOpen]);

  // Reset highlighted index when search query changes
  useEffect(() => {
    setHighlightedIndex(0);
  }, [searchQuery]);

  // Scroll highlighted item into view
  useEffect(() => {
    if (
      highlightedItemRef.current &&
      typeof highlightedItemRef.current.scrollIntoView === "function"
    ) {
      highlightedItemRef.current.scrollIntoView({
        block: "nearest",
        behavior: "smooth",
      });
    }
  }, [highlightedIndex]);

  // Center to the requested anchor container
  useEffect(() => {
    if (!isOpen) return;

    const updateAnchorCenter = () => {
      if (!anchorSelector) {
        setAnchorCenterX(null);
        return;
      }
      const anchor = document.querySelector(anchorSelector);
      if (anchor instanceof HTMLElement) {
        const rect = anchor.getBoundingClientRect();
        setAnchorCenterX(rect.left + rect.width / 2);
      } else {
        setAnchorCenterX(null);
      }
    };

    updateAnchorCenter();

    const anchor = anchorSelector ? document.querySelector(anchorSelector) : null;
    const resizeObserver =
      anchor instanceof HTMLElement && typeof ResizeObserver !== "undefined"
        ? new ResizeObserver(() => updateAnchorCenter())
        : null;

    if (anchor instanceof HTMLElement && resizeObserver) {
      resizeObserver.observe(anchor);
    }

    window.addEventListener("resize", updateAnchorCenter);
    window.addEventListener("scroll", updateAnchorCenter, true);

    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener("resize", updateAnchorCenter);
      window.removeEventListener("scroll", updateAnchorCenter, true);
    };
  }, [isOpen, anchorSelector]);

  // Click outside to close
  useEffect(() => {
    if (!isOpen) return;

    const handleMouseDown = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        onClose();
      }
    };

    document.addEventListener("mousedown", handleMouseDown);
    return () => document.removeEventListener("mousedown", handleMouseDown);
  }, [isOpen, onClose]);

  // ============================================================================
  // Return
  // ============================================================================

  return {
    // State
    searchQuery,
    setSearchQuery,
    highlightedIndex,
    anchorCenterX,

    // Refs
    inputRef,
    containerRef,
    highlightedItemRef,

    // Store state
    activePlanId,
    planCandidates,
    isLoading,
    error,

    // Derived state
    sortedCandidates,
    filteredCandidates,
    canClearPlan,
    showQuickAction,

    // Quick action
    quickAction,
    quickActionFlow,

    // Helpers
    getItemAtIndex,
    getTotalItemCount,

    // Handlers
    handleKeyDown,
    handleSelect,
    handleClear,
    handleQuickActionSelect,
    handleRetry,
  };
}
