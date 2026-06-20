/**
 * PlanQuickSwitcherPalette - Non-modal command palette for quick plan switching
 *
 * Features:
 * - Fixed positioned panel (top: 80px, centered)
 * - Glass morphism styling
 * - Keyboard-first interaction (up/down/enter/escape)
 * - Auto-focus search input on open
 * - Click outside to close
 * - No backdrop/modal overlay
 * - Uses framer-motion for enter/exit animations
 */

import { useState, useEffect } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { FileText, Loader2, AlertCircle, RefreshCw } from "lucide-react";
import { usePlanQuickSwitcher } from "@/hooks/usePlanQuickSwitcher";
import { PlanCandidateItem } from "./PlanCandidateItem";
import { PlanClearAction } from "./PlanClearAction";
import { QuickActionRow } from "./QuickActionRow";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";
import { Button } from "@/components/ui/button";
import type { SelectionSource } from "@/api/plan";

// ============================================================================
// Types
// ============================================================================

interface PlanQuickSwitcherPaletteProps {
  projectId: string;
  isOpen: boolean;
  onClose: () => void;
  /** Source attribution for selection analytics */
  selectionSource?: SelectionSource;
  /** Show clear active plan command at top of list when active plan exists */
  showClearAction?: boolean;
  /** Optional CSS selector used to anchor horizontal centering to a specific container */
  anchorSelector?: string;
}

// ============================================================================
// Component
// ============================================================================

export function PlanQuickSwitcherPalette({
  projectId,
  isOpen,
  onClose,
  selectionSource = "quick_switcher",
  showClearAction = true,
  anchorSelector,
}: PlanQuickSwitcherPaletteProps) {
  const switcher = usePlanQuickSwitcher({
    projectId,
    isOpen,
    onClose,
    selectionSource,
    showClearAction,
    ...(anchorSelector && { anchorSelector }),
  });

  // Destructure to separate refs from data (fixes ESLint refs-during-render false positive)
  const {
    inputRef,
    containerRef,
    highlightedItemRef,
    filteredCandidates,
    canClearPlan,
    showQuickAction,
    activePlanId,
    isLoading,
    error: storeError,
    searchQuery,
    highlightedIndex,
    anchorCenterX,
    quickAction,
    quickActionFlow,
    handleKeyDown,
    handleSelect,
    handleClear,
    handleQuickActionSelect,
    handleRetry,
    setSearchQuery,
  } = switcher;

  const hasItems = filteredCandidates.length > 0 || canClearPlan;

  // Local mouse hover state (hook manages keyboard navigation via highlightedIndex)
  const [mouseHighlightedIndex, setMouseHighlightedIndex] = useState<number | null>(null);

  // Use mouse index when available, otherwise fall back to keyboard index
  const effectiveHighlightedIndex = mouseHighlightedIndex ?? highlightedIndex;

  // Clear mouse highlight when keyboard navigation occurs
  useEffect(() => {
    setMouseHighlightedIndex(null);
  }, [highlightedIndex]);

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          initial="hidden"
          animate="visible"
          exit="exit"
          variants={{
            hidden: { opacity: 0, y: -20 },
            visible: {
              opacity: 1,
              y: 0,
              transition: {
                opacity: { duration: 0.15 },
                y: {
                  type: "spring",
                  stiffness: 400,
                  damping: 30,
                },
              },
            },
            exit: {
              opacity: 0,
              y: -20,
              transition: { duration: 0.1 },
            },
          }}
          className="fixed top-20 left-1/2 -translate-x-1/2 z-50 w-[420px]"
          ref={containerRef}
          data-quick-switcher-panel
          style={{
            left: anchorCenterX !== null ? `${anchorCenterX}px` : undefined,
          }}
        >
          <div
            className="text-popover-foreground"
            onClick={(e) => e.stopPropagation()}
            style={{
              borderRadius: "10px",
              background: withAlpha("var(--bg-surface)", 92),
              backdropFilter: "blur(20px) saturate(180%)",
              WebkitBackdropFilter: "blur(20px) saturate(180%)",
              border: "1px solid var(--overlay-weak)",
              boxShadow: "var(--shadow-lg)",
            }}
          >
            {/* Search input */}
            <div
              className="p-3"
              style={{ borderBottom: "1px solid var(--overlay-weak)" }}
            >
              <input
                ref={inputRef}
                type="text"
                placeholder="Search plans... (Cmd+Shift+P)"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={handleKeyDown}
                className={cn(
                  "w-full bg-transparent border-0 text-sm placeholder:text-muted-foreground",
                  "outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none",
                  "transition-colors"
                )}
                style={{
                  color: "var(--text-primary)",
                  boxShadow: "none",
                  outline: "none",
                }}
              />
            </div>

            {/* Results list */}
            {storeError ? (
              <div className="p-8 flex flex-col items-center justify-center gap-3 text-muted-foreground">
                <AlertCircle className="h-5 w-5 text-destructive" />
                <p className="text-center">{storeError}</p>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleRetry}
                  className="gap-2"
                >
                  <RefreshCw className="h-4 w-4" />
                  Retry
                </Button>
              </div>
            ) : isLoading ? (
              <div className="p-8 flex flex-col items-center justify-center gap-2 text-muted-foreground transition-colors">
                <Loader2 className="h-5 w-5 animate-spin" />
                <p>Loading plans...</p>
              </div>
            ) : quickActionFlow.isBlocking ? (
              /* Quick action flow is blocking - show single row replacing the list */
              <QuickActionRow
                action={quickAction}
                flowState={quickActionFlow.flowState}
                searchQuery={searchQuery}
                isHighlighted={false}
                onMouseEnter={() => {}}
                onSelect={() => {}}
                onConfirm={() => quickActionFlow.confirm(searchQuery)}
                onCancel={quickActionFlow.cancel}
                onViewEntity={quickActionFlow.viewEntity}
              />
            ) : hasItems ? (
              <div className="max-h-[400px] overflow-y-auto">
                {showQuickAction && (
                  <QuickActionRow
                    action={quickAction}
                    flowState={quickActionFlow.flowState}
                    searchQuery={searchQuery}
                    isHighlighted={effectiveHighlightedIndex === 0}
                    onMouseEnter={() => setMouseHighlightedIndex(0)}
                    onSelect={handleQuickActionSelect}
                    onConfirm={() => quickActionFlow.confirm(searchQuery)}
                    onCancel={quickActionFlow.cancel}
                    onViewEntity={quickActionFlow.viewEntity}
                    highlightedRef={
                      effectiveHighlightedIndex === 0
                        ? (highlightedItemRef as React.RefObject<HTMLButtonElement>)
                        : undefined
                    }
                  />
                )}
                {canClearPlan && (
                  <PlanClearAction
                    isHighlighted={effectiveHighlightedIndex === (showQuickAction ? 1 : 0)}
                    onMouseEnter={() => setMouseHighlightedIndex(showQuickAction ? 1 : 0)}
                    onClick={handleClear}
                    highlightedRef={
                      effectiveHighlightedIndex === (showQuickAction ? 1 : 0)
                        ? (highlightedItemRef as React.RefObject<HTMLButtonElement>)
                        : undefined
                    }
                  />
                )}
                {filteredCandidates.map((plan, index) => {
                  const offset = (showQuickAction ? 1 : 0) + (canClearPlan ? 1 : 0);
                  const itemIndex = index + offset;
                  const isActive = activePlanId === plan.sessionId;
                  const isHighlighted = effectiveHighlightedIndex === itemIndex;

                  return (
                    <PlanCandidateItem
                      key={plan.sessionId}
                      plan={plan}
                      isActive={isActive}
                      isHighlighted={isHighlighted}
                      onMouseEnter={() => setMouseHighlightedIndex(itemIndex)}
                      onClick={() => handleSelect(plan.sessionId)}
                      highlightedRef={
                        isHighlighted
                          ? (highlightedItemRef as React.RefObject<HTMLButtonElement>)
                          : undefined
                      }
                    />
                  );
                })}
              </div>
            ) : (
              /* Empty state */
              <div className="p-8 text-center text-muted-foreground transition-colors">
                <FileText className="h-8 w-8 mx-auto mb-2 opacity-50 transition-opacity" />
                <p>No accepted plans found</p>
              </div>
            )}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
