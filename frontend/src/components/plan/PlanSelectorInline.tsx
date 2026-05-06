/**
 * PlanSelectorInline Component
 *
 * Shared inline plan selector trigger used in Graph and Kanban toolbars.
 * Shows current plan title + task count badge and opens the global palette.
 */

import * as React from "react";
import { FileText, AlertCircle, ChevronDown } from "lucide-react";
import { usePlanStore } from "@/stores/planStore";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { SelectionSource } from "@/api/plan";

// ============================================================================
// Props Interface
// ============================================================================

interface PlanSelectorInlineProps {
  projectId: string;
  /** Icon-only mode for tight layouts */
  compact?: boolean;
  /** Selection source attributed when this launcher opens the palette */
  source: SelectionSource;
  /** Opens the global plan palette */
  onOpenPalette: (source: SelectionSource) => void;
}

// ============================================================================
// Component
// ============================================================================

export function PlanSelectorInline({
  projectId,
  compact = false,
  source,
  onOpenPalette,
}: PlanSelectorInlineProps) {
  // Store state
  const activePlanId = usePlanStore(
    (state) => state.activePlanByProject[projectId] ?? null
  );
  const planCandidates = usePlanStore((state) => state.planCandidates);
  const loadCandidates = usePlanStore((state) => state.loadCandidates);

  // Find active plan details
  const activePlan = React.useMemo(
    () => planCandidates.find((p) => p.sessionId === activePlanId),
    [planCandidates, activePlanId]
  );

  // Ensure inline selector can resolve active plan title/stats without requiring
  // the command palette to be opened first.
  React.useEffect(() => {
    if (!projectId) return;
    if (!activePlanId) return;
    if (activePlan) return;
    void loadCandidates(projectId);
  }, [projectId, activePlanId, activePlan, loadCandidates]);

  const handleOpenPalette = React.useCallback(() => {
    onOpenPalette(source);
  }, [onOpenPalette, source]);

  const hasActivePlan = Boolean(activePlanId);

  return (
    <Button
      variant="outline"
      size="sm"
      onClick={handleOpenPalette}
      className={cn(
        "h-[30px] min-w-0 max-w-[380px] gap-2 rounded-[6px] px-2.5 text-[0.7812rem] font-medium shadow-none",
        !hasActivePlan && "text-muted-foreground"
      )}
      style={{
        backgroundColor: hasActivePlan ? "var(--bg-hover)" : "transparent",
        borderColor: hasActivePlan ? "var(--border-default)" : "transparent",
        borderStyle: "solid",
        borderWidth: "1px",
        color: hasActivePlan ? "var(--text-primary)" : "var(--text-muted)",
      }}
      data-testid="plan-selector-inline-trigger"
    >
      {hasActivePlan ? (
        <>
          <FileText className="h-[13px] w-[13px] shrink-0" />
          {!compact && (
            <>
              <span className="min-w-0 max-w-[260px] truncate">
                {activePlan?.title || "Untitled Plan"}
              </span>
              {activePlan?.taskStats && (
                <span
                  className="shrink-0 text-[0.6875rem] font-medium"
                  style={{
                    color: "var(--text-muted)",
                    fontFamily:
                      "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                  }}
                >
                  {activePlan.taskStats.incomplete}/{activePlan.taskStats.total}
                </span>
              )}
            </>
          )}
          {compact && (
            <ChevronDown className="h-3.5 w-3.5 opacity-50" />
          )}
        </>
      ) : (
        <>
          <AlertCircle className="h-[13px] w-[13px] shrink-0" />
          {!compact && <span>Select plan</span>}
          <ChevronDown className="h-3.5 w-3.5 opacity-50" />
        </>
      )}
    </Button>
  );
}
