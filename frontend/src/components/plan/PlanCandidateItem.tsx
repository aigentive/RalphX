/**
 * PlanCandidateItem - Reusable plan row button for quick switcher
 *
 * Displays a single plan candidate with:
 * - Title and task stats
 * - Progress bar (if tasks incomplete)
 * - Active indicator checkmark
 * - Hover/highlight states with accent styling
 */

import { Check } from "lucide-react";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { useChatStore, selectAgentStatus } from "@/stores/chatStore";
import type { PlanCandidate } from "@/stores/planStore";

// ============================================================================
// Types
// ============================================================================

interface PlanCandidateItemProps {
  plan: PlanCandidate;
  isActive: boolean;
  isHighlighted: boolean;
  onMouseEnter: () => void;
  onClick: () => void;
  highlightedRef?: React.RefObject<HTMLButtonElement> | undefined;
}

// ============================================================================
// Utilities
// ============================================================================

function formatIncompleteSummary(incomplete: number, total: number): string {
  if (total <= 0) return "No tasks yet";
  if (incomplete <= 0) {
    return total === 1 ? "1 task complete" : `${total} tasks complete`;
  }
  return `${incomplete} of ${total} incomplete`;
}

function getCompletionPercent(incomplete: number, total: number): number {
  if (total <= 0) return 0;
  const completed = Math.max(0, total - Math.max(0, incomplete));
  return Math.round((completed / total) * 100);
}

// ============================================================================
// Component
// ============================================================================

export function PlanCandidateItem({
  plan,
  isActive,
  isHighlighted,
  onMouseEnter,
  onClick,
  highlightedRef,
}: PlanCandidateItemProps) {
  const storeKey = buildStoreKey("ideation", plan.sessionId);
  const agentStatus = useChatStore(selectAgentStatus(storeKey));
  const isIdeationActive = agentStatus === "generating";
  const isIdeationWaiting = agentStatus === "waiting_for_input";

  const completionPercent = getCompletionPercent(
    plan.taskStats.incomplete,
    plan.taskStats.total
  );
  const showProgressBar =
    plan.taskStats.total > 0 && plan.taskStats.incomplete > 0;

  return (
    <button
      ref={highlightedRef}
      onClick={onClick}
      onMouseEnter={onMouseEnter}
      className={cn(
        "w-full text-left px-3 py-2 rounded-lg flex items-center justify-between",
        "transition-all duration-150 origin-center",
        "hover:scale-[1.01]",
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        isHighlighted && "bg-accent",
        isActive && "bg-accent/50"
      )}
      style={{
        background:
          isHighlighted
            ? withAlpha("var(--accent-primary)", 16)
            : isActive
              ? withAlpha("var(--accent-primary)", 10)
              : "transparent",
        border: isHighlighted
          ? "1px solid var(--accent-border)"
          : "1px solid transparent",
      }}
    >
      <div className="flex-1 min-w-0">
        <div
          className="text-[0.8125rem] font-medium leading-tight"
          style={{ color: isHighlighted ? "var(--accent-primary)" : "var(--text-primary)" }}
        >
          {plan.title || "Untitled Plan"}
        </div>
        <div className="text-xs leading-tight mt-0.5" style={{ color: "var(--text-muted)" }}>
          {formatIncompleteSummary(plan.taskStats.incomplete, plan.taskStats.total)}
          {plan.taskStats.activeNow > 0 && " • Active work"}
          {isIdeationActive && (
            <span style={{ color: "var(--accent-primary)" }}>
              {plan.taskStats.total > 0 ? " • Session active" : "Session active"}
            </span>
          )}
          {isIdeationWaiting && (
            <span>
              {plan.taskStats.total > 0 ? " • Awaiting input" : "Awaiting input"}
            </span>
          )}
        </div>
      </div>

      <div className="flex items-center gap-2 ml-3 shrink-0">
        {showProgressBar && (
          <div className="flex items-center gap-1.5" aria-hidden="true">
            <div
              className="w-14 h-1 rounded-full overflow-hidden"
              style={{ backgroundColor: "var(--overlay-moderate)" }}
            >
              <div
                className="h-full rounded-full transition-all duration-300"
                style={{
                  width: `${completionPercent}%`,
                  backgroundColor: withAlpha("var(--accent-primary)", 70),
                }}
              />
            </div>
            <span
              className="text-[0.625rem] tabular-nums"
              style={{ color: "var(--text-muted)" }}
            >
              {completionPercent}%
            </span>
          </div>
        )}
        {isActive && <Check className="h-4 w-4" style={{ color: "var(--accent-primary)" }} />}
      </div>
    </button>
  );
}
