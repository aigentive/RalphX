/**
 * QuickActionRow - Rendering component for QuickAction in command palette
 *
 * Displays a quick action in different states:
 * - idle: Button row with icon + label + description (query)
 * - confirming: Inline confirmation with query + Confirm/Cancel buttons
 * - creating: Spinner + creatingLabel
 * - success: Check icon + successLabel + "View" button
 *
 * Uses framer-motion for smooth state transitions and glass morphism styling
 * matching PlanCandidateItem patterns.
 */

import { AnimatePresence, motion } from "framer-motion";
import { Loader2, Check } from "lucide-react";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";
import type { QuickAction } from "@/hooks/useIdeationQuickAction";

// ============================================================================
// Types
// ============================================================================

/**
 * Flow state for quick action execution
 */
export type QuickActionFlowState = "idle" | "confirming" | "creating" | "success";

export interface QuickActionRowProps {
  /** The action configuration */
  action: QuickAction;
  /** Current flow state */
  flowState: QuickActionFlowState;
  /** Current search query */
  searchQuery: string;
  /** Whether this row is highlighted (keyboard nav) */
  isHighlighted: boolean;
  /** Callback when mouse enters the row */
  onMouseEnter: () => void;
  /** Callback when row is selected (clicked in idle state) */
  onSelect: () => void;
  /** Callback when action is confirmed */
  onConfirm: () => void;
  /** Callback when action is cancelled */
  onCancel: () => void;
  /** Callback when "View" button is clicked after success */
  onViewEntity: () => void;
  /** Ref to attach when highlighted (for keyboard scroll-into-view) */
  highlightedRef?: React.RefObject<HTMLButtonElement> | undefined;
}

// ============================================================================
// Component
// ============================================================================

export function QuickActionRow({
  action,
  flowState,
  searchQuery,
  isHighlighted,
  onMouseEnter,
  onSelect,
  onConfirm,
  onCancel,
  onViewEntity,
  highlightedRef,
}: QuickActionRowProps) {
  const Icon = action.icon;
  const description = action.description(searchQuery);

  return (
    <AnimatePresence mode="wait">
      {flowState === "idle" && (
        <motion.button
          key="idle"
          ref={isHighlighted ? highlightedRef : null}
          onClick={onSelect}
          onMouseEnter={onMouseEnter}
          initial={{ opacity: 0, height: 0 }}
          animate={{ opacity: 1, height: "auto" }}
          exit={{ opacity: 0, height: 0 }}
          transition={{ duration: 0.15 }}
          className={cn(
            "w-full text-left px-3 py-2 rounded-lg flex items-center gap-3",
            "transition-all duration-150 origin-center",
            "hover:scale-[1.01]",
            "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          )}
          style={{
            background: isHighlighted
              ? withAlpha("var(--accent-primary)", 16)
              : "transparent",
            border: isHighlighted
              ? "1px solid var(--accent-border)"
              : "1px solid transparent",
          }}
        >
          <Icon
            className="h-4 w-4 shrink-0"
            style={{ color: isHighlighted ? "var(--accent-primary)" : "var(--text-secondary)" }}
          />
          <div className="flex-1 min-w-0">
            <div
              className="text-[0.8125rem] font-medium leading-tight"
              style={{ color: isHighlighted ? "var(--accent-primary)" : "var(--text-primary)" }}
            >
              {action.label}
            </div>
            <div
              className="text-xs leading-tight mt-0.5 truncate"
              style={{ color: "var(--text-muted)" }}
            >
              {description}
            </div>
          </div>
        </motion.button>
      )}

      {flowState === "confirming" && (
        <motion.div
          key="confirming"
          initial={{ opacity: 0, height: 0 }}
          animate={{ opacity: 1, height: "auto" }}
          exit={{ opacity: 0, height: 0 }}
          transition={{ duration: 0.15 }}
          className="px-3 py-2"
        >
          <div className="flex flex-col gap-2">
            <div className="text-sm" style={{ color: "var(--text-primary)" }}>
              <span style={{ color: "var(--text-secondary)" }}>Create session for: </span>
              <span className="font-medium">{description}</span>
            </div>
            <div className="flex gap-2">
              <button
                onClick={onConfirm}
                className={cn(
                  "flex-1 px-3 py-1.5 rounded text-xs font-medium",
                  "transition-all duration-150",
                  "hover:scale-[1.02]",
                  "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                )}
                style={{
                  background: withAlpha("var(--accent-primary)", 16),
                  border: "1px solid var(--accent-border)",
                  color: "var(--accent-primary)",
                }}
              >
                Create Session
              </button>
              <button
                onClick={onCancel}
                className={cn(
                  "flex-1 px-3 py-1.5 rounded text-xs font-medium",
                  "transition-all duration-150",
                  "hover:scale-[1.02]",
                  "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                )}
                style={{
                  background: "var(--overlay-weak)",
                  border: "1px solid var(--overlay-moderate)",
                  color: "var(--text-secondary)",
                }}
              >
                Cancel
              </button>
            </div>
          </div>
        </motion.div>
      )}

      {flowState === "creating" && (
        <motion.div
          key="creating"
          initial={{ opacity: 0, height: 0 }}
          animate={{ opacity: 1, height: "auto" }}
          exit={{ opacity: 0, height: 0 }}
          transition={{ duration: 0.15 }}
          className="px-3 py-2 flex items-center gap-3"
        >
          <Loader2
            className="h-4 w-4 animate-spin shrink-0"
            style={{ color: "var(--accent-primary)" }}
          />
          <div
            className="text-[0.8125rem] font-medium"
            style={{ color: "var(--text-primary)" }}
          >
            {action.creatingLabel}
          </div>
        </motion.div>
      )}

      {flowState === "success" && (
        <motion.div
          key="success"
          initial={{ opacity: 0, height: 0 }}
          animate={{ opacity: 1, height: "auto" }}
          exit={{ opacity: 0, height: 0 }}
          transition={{ duration: 0.15 }}
          className="px-3 py-2"
        >
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-3">
              <Check
                className="h-4 w-4 shrink-0"
                style={{ color: "var(--status-success)" }}
              />
              <div
                className="text-[0.8125rem] font-medium"
                style={{ color: "var(--text-primary)" }}
              >
                {action.successLabel}
              </div>
            </div>
            <button
              onClick={onViewEntity}
              className={cn(
                "px-3 py-1.5 rounded text-xs font-medium shrink-0",
                "transition-all duration-150",
                "hover:scale-[1.02]",
                "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              )}
              style={{
                background: withAlpha("var(--accent-primary)", 16),
                border: "1px solid var(--accent-border)",
                color: "var(--accent-primary)",
              }}
            >
              {action.viewLabel}
            </button>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
