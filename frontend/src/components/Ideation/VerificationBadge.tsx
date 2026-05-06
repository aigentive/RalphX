/**
 * VerificationBadge — color-coded status badge for plan verification state.
 *
 * Design: macOS Tahoe style, warm orange accent (#ff6b35), SF Pro, no purple/blue.
 */

import { Loader2, RefreshCw } from "lucide-react";
import { useRef, useState } from "react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { VerificationStatus } from "@/types/ideation";

// ============================================================================
// Types
// ============================================================================

export interface VerificationBadgeProps {
  status: VerificationStatus;
  inProgress: boolean;
  currentRound?: number;
  maxRounds?: number;
  convergenceReason?: string;
  /** Source project name — used for imported_verified tooltip */
  sourceProjectName?: string;
  /** Called when user clicks Retry on a stuck session */
  onRetry?: () => void | Promise<void>;
}

// ============================================================================
// Config
// ============================================================================

const STATUS_CONFIG: Record<
  VerificationStatus,
  { label: string; bg: string; border: string; color: string }
> = {
  unverified: {
    label: "Unverified",
    bg: "var(--overlay-weak)",
    border: "var(--overlay-moderate)",
    color: "var(--text-secondary)",
  },
  reviewing: {
    label: "Reviewing",
    bg: "var(--accent-muted)",
    border: "var(--accent-border)",
    color: "var(--accent-primary)",
  },
  verified: {
    label: "Verified",
    bg: "var(--status-success-muted)",
    border: "var(--status-success-border)",
    color: "var(--status-success)",
  },
  needs_revision: {
    label: "Needs Revision",
    bg: "var(--status-error-muted)",
    border: "var(--status-error-border)",
    color: "var(--status-error)",
  },
  skipped: {
    label: "Skipped",
    bg: "var(--status-warning-muted)",
    border: "var(--status-warning-border)",
    color: "var(--status-warning)",
  },
  imported_verified: {
    label: "Verified (imported)",
    bg: "var(--status-success-muted)",
    border: "var(--status-success-border)",
    color: "var(--status-success)",
  },
};

const CONVERGENCE_REASON_LABELS: Record<string, string> = {
  zero_blocking: "No blocking gaps remain",
  jaccard_converged: "Gap list stabilized across rounds",
  max_rounds: "Maximum verification rounds reached",
  critic_parse_failure: "Critic output could not be parsed",
  user_skipped: "Manually skipped by user",
  user_reverted: "Plan reverted to original version",
  escalated_to_parent: "Escalated to orchestrator",
  user_stopped: "Stopped by user",
};

// ============================================================================
// Component
// ============================================================================

export function VerificationBadge({
  status,
  inProgress,
  currentRound,
  maxRounds,
  convergenceReason,
  sourceProjectName,
  onRetry,
}: VerificationBadgeProps) {
  const isRetryingRef = useRef(false);
  const [isRetrying, setIsRetrying] = useState(false);
  const config = STATUS_CONFIG[status];
  const showProgress = inProgress && currentRound !== undefined && maxRounds !== undefined;
  // A session is "stuck" when it is reviewing+inProgress but there's a retry callback
  const isStuck = status === "reviewing" && inProgress && onRetry !== undefined;
  const importedTooltip = status === "imported_verified"
    ? sourceProjectName
      ? `This plan was verified in ${sourceProjectName} and imported`
      : "This plan was verified in another project and imported"
    : undefined;
  const tooltipText = importedTooltip ?? (convergenceReason
    ? (CONVERGENCE_REASON_LABELS[convergenceReason] ?? convergenceReason)
    : undefined);

  const badge = (
    <span
      className="inline-flex items-center gap-1.5 text-[0.625rem] font-medium px-1.5 py-0.5 rounded-md flex-shrink-0 select-none"
      style={{
        background: config.bg,
        border: `1px solid ${config.border}`,
        color: config.color,
      }}
    >
      {/* Pulsing dot when actively reviewing */}
      {inProgress && (
        <span
          className="w-1.5 h-1.5 rounded-full animate-pulse flex-shrink-0"
          style={{ background: config.color }}
        />
      )}

      {config.label}

      {/* Round progress indicator */}
      {showProgress && (
        <span
          className="opacity-70"
          style={{ color: config.color }}
        >
          {currentRound}/{maxRounds}
        </span>
      )}

      {/* Retry button for stuck sessions */}
      {isStuck && onRetry && (
        <button
          type="button"
          disabled={isRetrying}
          onClick={async (e) => {
            e.stopPropagation();
            if (isRetryingRef.current) return;
            isRetryingRef.current = true;
            setIsRetrying(true);
            try {
              await onRetry?.();
            } finally {
              isRetryingRef.current = false;
              setIsRetrying(false);
            }
          }}
          className="ml-0.5 rounded transition-opacity hover:opacity-80 flex items-center disabled:opacity-50 disabled:cursor-not-allowed"
          title="Retry verification"
          style={{ color: config.color }}
        >
          {isRetrying ? (
            <Loader2 className="w-2.5 h-2.5 animate-spin" />
          ) : (
            <RefreshCw className="w-2.5 h-2.5" />
          )}
        </button>
      )}
    </span>
  );

  if (!tooltipText) {
    return badge;
  }

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>{badge}</TooltipTrigger>
        <TooltipContent className="max-w-xs text-[0.6875rem]">
          {tooltipText}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
