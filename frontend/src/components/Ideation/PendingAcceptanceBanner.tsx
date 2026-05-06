/**
 * PendingAcceptanceBanner - Inline banner shown after "View Plan" closes the dialog.
 *
 * Follows AcceptedSessionBanner.tsx visual pattern.
 * Shown in PlanningView when session.acceptanceStatus === "pending".
 */

import { AlertTriangle, Check, X, Loader2 } from "lucide-react";
import { useAcceptFinalize, useRejectFinalize } from "@/hooks/useAcceptFinalize";
import { useUiStore } from "@/stores/uiStore";
import { withAlpha } from "@/lib/theme-colors";

interface PendingAcceptanceBannerProps {
  sessionId: string;
}

export function PendingAcceptanceBanner({ sessionId }: PendingAcceptanceBannerProps) {
  const removeFromQueue = useUiStore((s) => s.removeFromConfirmationQueue);
  const accept = useAcceptFinalize(sessionId);
  const reject = useRejectFinalize(sessionId);

  const isLoading = accept.isPending || reject.isPending;

  async function handleAccept() {
    try {
      await accept.mutateAsync();
      removeFromQueue(sessionId);
    } catch {
      // Error toast handled in useAcceptFinalize onError
    }
  }

  async function handleReject() {
    try {
      await reject.mutateAsync();
      removeFromQueue(sessionId);
    } catch {
      // Error toast handled in useRejectFinalize onError
    }
  }

  return (
    <div
      data-testid="pending-acceptance-banner"
      className="mb-4 rounded-xl overflow-hidden"
      style={{
        background: withAlpha("var(--bg-elevated)", 80),
        border: "1px solid var(--status-warning-border)",
        boxShadow: `0 0 24px ${withAlpha("var(--status-warning)", 4)}`,
      }}
    >
      {/* Top accent line */}
      <div
        className="h-[2px] w-full"
        style={{
          background: `linear-gradient(90deg, ${withAlpha("var(--status-warning)", 60)} 0%, ${withAlpha("var(--status-warning)", 10)} 100%)`,
        }}
      />

      <div className="px-4 py-3.5 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <div
            className="w-5 h-5 rounded-full flex items-center justify-center flex-shrink-0"
            style={{ background: withAlpha("var(--status-warning)", 15) }}
          >
            <AlertTriangle className="w-3 h-3" style={{ color: "var(--status-warning)" }} />
          </div>
          <span className="text-[0.8125rem] font-medium" style={{ color: "var(--text-primary)" }}>
            Plan pending your confirmation
          </span>
        </div>

        <div className="flex items-center gap-2">
          {/* Reject */}
          <button
            onClick={handleReject}
            disabled={isLoading}
            className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-[0.6875rem] font-medium transition-all duration-150 disabled:opacity-40 disabled:cursor-not-allowed"
            style={{
              color: "var(--status-error)",
              border: "1px solid var(--status-error-border)",
              background: "var(--status-error-muted)",
            }}
            onMouseEnter={(e) => {
              if (!isLoading) e.currentTarget.style.background = withAlpha("var(--status-error)", 12);
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "var(--status-error-muted)";
            }}
          >
            {reject.isPending ? (
              <Loader2 className="w-3 h-3 animate-spin" />
            ) : (
              <X className="w-3 h-3" />
            )}
            Reject
          </button>

          {/* Accept */}
          <button
            onClick={handleAccept}
            disabled={isLoading}
            className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-[0.6875rem] font-semibold transition-all duration-150 disabled:opacity-40 disabled:cursor-not-allowed"
            style={{
              color: "var(--text-on-accent)",
              background: accept.isPending ? withAlpha("var(--accent-primary)", 85) : "var(--accent-primary)",
              boxShadow: `0 1px 4px ${withAlpha("var(--accent-primary)", 30)}`,
            }}
            onMouseEnter={(e) => {
              if (!isLoading) e.currentTarget.style.background = withAlpha("var(--accent-primary)", 90);
            }}
            onMouseLeave={(e) => {
              if (!accept.isPending) e.currentTarget.style.background = "var(--accent-primary)";
            }}
          >
            {accept.isPending ? (
              <Loader2 className="w-3 h-3 animate-spin" />
            ) : (
              <Check className="w-3 h-3" />
            )}
            Accept
          </button>
        </div>
      </div>
    </div>
  );
}
