/**
 * ProposalsToolbar - macOS Tahoe styled action toolbar
 *
 * Design: Refined toolbar with subtle separators, icon-based actions,
 * and warm orange accent for primary actions.
 */

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { withAlpha } from "@/lib/theme-colors";
import {
  Trash2,
  Network,
  Check,
  AlertCircle,
  ShieldAlert,
} from "lucide-react";
import { useDependencyGraphValidation } from "@/hooks/useDependencyGraphComplete";
import { useVerificationGate } from "@/hooks/useVerificationGate";
import type { TaskProposal } from "@/types/ideation";
import type { DependencyGraphResponse } from "@/api/ideation.types";
import type { IdeationSessionResponse } from "@/api/ideation";

// ============================================================================
// Types
// ============================================================================

interface ProposalsToolbarProps {
  proposals: TaskProposal[];
  graph: DependencyGraphResponse | null | undefined;
  isReadOnly?: boolean;
  onClearAll: () => void;
  onAcceptPlan: () => void;
  onAnalyzeDependencies?: () => void;
  /** True after the 90s frontend safety timeout or after an analysis_failed event.
   *  When true, the accept button label changes to "Accept without dependencies" to
   *  signal that the plan can be accepted despite incomplete dependency analysis. */
  analysisTimedOut?: boolean;
  /** Session for verification gate — blocks accept when verification is required */
  session?: Pick<
    IdeationSessionResponse,
    "id" | "planArtifactId" | "sessionPurpose" | "verificationStatus" | "verificationInProgress"
  > | null;
  /** True when agent-initiated finalization is pending user confirmation */
  isPendingAcceptance?: boolean;
}

// ============================================================================
// Component
// ============================================================================

export function ProposalsToolbar({
  proposals,
  graph,
  isReadOnly = false,
  onClearAll,
  onAcceptPlan,
  onAnalyzeDependencies,
  analysisTimedOut = false,
  session = null,
  isPendingAcceptance = false,
}: ProposalsToolbarProps) {
  const totalCount = proposals.length;
  const validation = useDependencyGraphValidation(proposals, graph);
  const verificationGate = useVerificationGate(session);
  const canAccept =
    totalCount > 0 &&
    !isReadOnly &&
    validation.isComplete &&
    verificationGate.canAccept &&
    !isPendingAcceptance;
  const showAnalyzeButton = totalCount >= 2 && onAnalyzeDependencies && !isReadOnly;
  const acceptLabel = analysisTimedOut ? "Accept without dependencies" : `Accept Plan (${totalCount})`;
  const verificationBlocked = !isReadOnly && !verificationGate.canAccept && totalCount > 0;

  return (
    <div
      className="flex items-center justify-between px-4 h-11"
      style={{
        borderBottom: "1px solid var(--overlay-weak)",
        background: withAlpha("var(--bg-base)", 60),
      }}
    >
      {/* Left: Proposal count and analyzing status */}
      <div className="flex items-center gap-3">
        <span className="text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
          <span style={{ color: "var(--text-primary)" }} className="font-semibold">
            {totalCount}
          </span>
          {" "}{totalCount === 1 ? "proposal" : "proposals"}
        </span>

      </div>

      {/* Right: Actions */}
      <div className="flex items-center gap-1">
        <TooltipProvider>
          {/* Analyze Dependencies */}
          {showAnalyzeButton && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={onAnalyzeDependencies}
                  className="h-7 w-7 rounded-lg disabled:opacity-50 transition-colors duration-150"
                  style={{ color: "var(--text-muted)" }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.color = "var(--text-primary)";
                    e.currentTarget.style.background = "var(--overlay-weak)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.color = "var(--text-muted)";
                    e.currentTarget.style.background = "transparent";
                  }}
                >
                  <Network className="w-3.5 h-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Re-analyze dependencies</TooltipContent>
            </Tooltip>
          )}

          {/* Separator after analyze button */}
          {showAnalyzeButton && (
            <div
              className="w-px h-4 mx-1"
              style={{ background: "var(--overlay-moderate)" }}
            />
          )}

          {/* Clear All (only when not read-only) */}
          {!isReadOnly && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 rounded-lg"
                  onClick={onClearAll}
                  style={{ color: "var(--text-muted)" }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = "var(--status-error-muted)";
                    e.currentTarget.style.color = "var(--status-error)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = "transparent";
                    e.currentTarget.style.color = "var(--text-muted)";
                  }}
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Clear all proposals</TooltipContent>
            </Tooltip>
          )}

          {/* Separator before Accept Plan */}
          {!isReadOnly && (
            <div
              className="w-px h-4 mx-1"
              style={{ background: "var(--overlay-moderate)" }}
            />
          )}

          {/* Graph incomplete warning */}
          {!isReadOnly && totalCount > 0 && !validation.isComplete && (
            <Tooltip>
              <TooltipTrigger asChild>
                <div className="flex items-center px-1">
                  <AlertCircle
                    className="w-4 h-4"
                    style={{ color: "var(--status-warning)" }}
                  />
                </div>
              </TooltipTrigger>
              <TooltipContent className="max-w-xs">
                {validation.message}
              </TooltipContent>
            </Tooltip>
          )}

          {/* Pending acceptance warning */}
          {isPendingAcceptance && !isReadOnly && (
            <Tooltip>
              <TooltipTrigger asChild>
                <div className="flex items-center px-1">
                  <AlertCircle
                    className="w-4 h-4"
                    style={{ color: "var(--status-warning)" }}
                  />
                </div>
              </TooltipTrigger>
              <TooltipContent className="max-w-xs">
                Waiting for agent-initiated confirmation
              </TooltipContent>
            </Tooltip>
          )}

          {/* Verification gate warning */}
          {verificationBlocked && (
            <Tooltip>
              <TooltipTrigger asChild>
                <div className="flex items-center px-1">
                  <ShieldAlert
                    className="w-4 h-4"
                    style={{ color: "var(--status-error)" }}
                  />
                </div>
              </TooltipTrigger>
              <TooltipContent className="max-w-xs">
                {verificationGate.reason}
              </TooltipContent>
            </Tooltip>
          )}
        </TooltipProvider>

        {/* Accept Plan button (only when not read-only) */}
        {/* Status is determined automatically by backend based on dependencies */}
        {!isReadOnly && (
          <Button
            variant="ghost"
            size="sm"
            disabled={!canAccept}
            onClick={onAcceptPlan}
            className="h-7 px-3 text-[0.6875rem] font-semibold gap-1.5 rounded-lg transition-all duration-150"
            style={{
              color: canAccept ? "var(--accent-primary)" : "var(--text-muted)",
              background: canAccept ? withAlpha("var(--accent-primary)", 10) : "transparent",
              border: canAccept ? "1px solid var(--accent-border)" : "1px solid transparent",
            }}
            onMouseEnter={(e) => {
              if (canAccept) {
                e.currentTarget.style.background = "var(--accent-muted)";
              }
            }}
            onMouseLeave={(e) => {
              if (canAccept) {
                e.currentTarget.style.background = withAlpha("var(--accent-primary)", 10);
              }
            }}
          >
            <Check className="w-3 h-3" />
            {acceptLabel}
          </Button>
        )}
      </div>
    </div>
  );
}
