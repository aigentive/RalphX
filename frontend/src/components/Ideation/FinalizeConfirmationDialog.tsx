/**
 * FinalizeConfirmationDialog - Custom modal for agent-initiated plan finalization gate.
 *
 * Shows when the backend emits ideation:finalize_pending_confirmation.
 * Mounted at App root level so it appears regardless of the active view.
 * Manages a queue of pending confirmations: first item shown, rest wait.
 *
 * Design: Custom modal (NOT AlertDialog) — 3-action layout needs more than binary confirm/cancel.
 */

import { useCallback, useState } from "react";
import { Check, X, Eye, Loader2 } from "lucide-react";
import { useUiStore } from "@/stores/uiStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { useAcceptFinalize, useRejectFinalize } from "@/hooks/useAcceptFinalize";
import { useAgentGate } from "@/hooks/useAgentGate";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { withAlpha } from "@/lib/theme-colors";
import { navigateToIdeationSession } from "@/lib/navigation";

// ============================================================================
// Inner dialog — rendered only when there's an active session in queue
// ============================================================================

interface ActiveDialogProps {
  sessionId: string;
  onViewPlan: () => void;
}

function ActiveDialog({ sessionId, onViewPlan }: ActiveDialogProps) {
  const dequeue = useUiStore((s) => s.dequeueConfirmation);
  const addAutoAcceptSession = useUiStore((s) => s.addAutoAcceptSession);
  const sessions = useIdeationStore((s) => s.sessions);
  const sessionTitle = sessions[sessionId]?.title ?? null;

  const [autoAcceptSession, setAutoAcceptSession] = useState(false);

  const accept = useAcceptFinalize(sessionId);
  const reject = useRejectFinalize(sessionId);
  const acceptGate = useAgentGate("ideationAcceptFinalize");
  const rejectGate = useAgentGate("ideationRejectFinalize");

  const isLoading = accept.isPending || reject.isPending;

  const handleAccept = useCallback(async () => {
    if (acceptGate.gated) return;
    try {
      await accept.mutateAsync();
      if (autoAcceptSession) {
        addAutoAcceptSession(sessionId);
      }
      dequeue();
    } catch {
      // Error toast handled in useAcceptFinalize onError; keep dialog open for retry
    }
  }, [accept, acceptGate.gated, autoAcceptSession, addAutoAcceptSession, sessionId, dequeue]);

  const handleReject = useCallback(async () => {
    if (rejectGate.gated) return;
    try {
      await reject.mutateAsync();
      dequeue();
    } catch {
      // Error toast handled in useRejectFinalize onError; keep dialog open for retry
    }
  }, [reject, rejectGate.gated, dequeue]);

  const handleViewPlan = useCallback(() => {
    dequeue();
    onViewPlan();
  }, [dequeue, onViewPlan]);

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 z-[200]"
        style={{ background: "var(--overlay-scrim-deep)", backdropFilter: "blur(4px)" }}
      />

      {/* Dialog */}
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="finalize-dialog-title"
        className="fixed z-[201] left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 w-full max-w-md rounded-2xl overflow-hidden shadow-2xl"
        style={{
          background: "var(--bg-surface)",
          border: "1px solid var(--overlay-moderate)",
          boxShadow: "var(--shadow-lg)",
        }}
      >
        {/* Top accent */}
        <div
          className="h-[2px] w-full"
          style={{
            background: `linear-gradient(90deg, ${withAlpha("var(--accent-primary)", 80)} 0%, ${withAlpha("var(--accent-primary)", 10)} 100%)`,
          }}
        />

        <div className="px-6 py-5">
          {/* Header */}
          <div className="mb-4">
            <h2
              id="finalize-dialog-title"
              className="text-[0.9375rem] font-semibold mb-1"
              style={{ color: "var(--text-primary)" }}
            >
              Plan Ready for Acceptance
            </h2>
            <p className="text-[0.8125rem]" style={{ color: "var(--text-muted)" }}>
              The agent has finalized proposals and is waiting for your confirmation.
            </p>
          </div>

          {/* Session title */}
          {sessionTitle && (
            <div
              className="mb-5 px-3 py-2.5 rounded-lg text-[0.8125rem] font-medium truncate"
              style={{
                background: "var(--overlay-weak)",
                border: "1px solid var(--overlay-weak)",
                color: "var(--text-primary)",
              }}
            >
              {sessionTitle}
            </div>
          )}

          {/* Per-session auto-accept checkbox */}
          <div className="flex items-center gap-2 mb-5">
            <Checkbox
              id="auto-accept-session"
              checked={autoAcceptSession}
              onCheckedChange={(checked) => { setAutoAcceptSession(checked === true); }}
              disabled={isLoading || rejectGate.gated}
              title={rejectGate.reason ?? undefined}
              className="data-[state=checked]:bg-[var(--accent-primary)] data-[state=checked]:border-[var(--accent-primary)]"
            />
            <Label
              htmlFor="auto-accept-session"
              className="text-[0.75rem] cursor-pointer select-none"
              style={{ color: "var(--text-secondary)" }}
            >
              Auto-accept plans for this session
            </Label>
          </div>

          {/* Actions */}
          <div className="flex items-center gap-2 justify-end">
            {/* View Plan */}
            <button
              onClick={handleViewPlan}
              disabled={isLoading || acceptGate.gated}
              title={acceptGate.reason ?? undefined}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[0.75rem] font-medium transition-all duration-150 disabled:opacity-40 disabled:cursor-not-allowed"
              style={{
                color: "var(--text-secondary)",
                border: "1px solid var(--overlay-moderate)",
                background: "transparent",
              }}
              onMouseEnter={(e) => {
                if (!isLoading) {
                  e.currentTarget.style.background = "var(--overlay-weak)";
                  e.currentTarget.style.color = "var(--text-primary)";
                }
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "transparent";
                e.currentTarget.style.color = "var(--text-secondary)";
              }}
            >
              <Eye className="w-3.5 h-3.5" />
              View Plan
            </button>

            {/* Reject */}
            <button
              onClick={handleReject}
              disabled={isLoading}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[0.75rem] font-medium transition-all duration-150 disabled:opacity-40 disabled:cursor-not-allowed"
              style={{
                color: "var(--status-error)",
                border: "1px solid var(--status-error-border)",
                background: "var(--status-error-muted)",
              }}
              onMouseEnter={(e) => {
                if (!isLoading) {
                  e.currentTarget.style.background = withAlpha("var(--status-error)", 12);
                }
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "var(--status-error-muted)";
              }}
            >
              {reject.isPending ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <X className="w-3.5 h-3.5" />
              )}
              Reject
            </button>

            {/* Accept */}
            <button
              onClick={handleAccept}
              disabled={isLoading}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[0.75rem] font-semibold transition-all duration-150 disabled:opacity-40 disabled:cursor-not-allowed"
              style={{
                color: "var(--text-on-accent)",
                background: accept.isPending
                  ? withAlpha("var(--accent-primary)", 85)
                  : "var(--accent-primary)",
                boxShadow: `0 1px 4px ${withAlpha("var(--accent-primary)", 30)}`,
              }}
              onMouseEnter={(e) => {
                if (!isLoading) {
                  e.currentTarget.style.background = withAlpha("var(--accent-primary)", 90);
                }
              }}
              onMouseLeave={(e) => {
                if (!accept.isPending) {
                  e.currentTarget.style.background = "var(--accent-primary)";
                }
              }}
            >
              {accept.isPending ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <Check className="w-3.5 h-3.5" />
              )}
              Accept
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

// ============================================================================
// Public component — reads queue from uiStore
// ============================================================================

export function FinalizeConfirmationDialog() {
  const queue = useUiStore((s) => s.pendingConfirmationQueue);

  const activeSessionId = queue[0];

  const handleViewPlan = useCallback(() => {
    if (!activeSessionId) return;
    navigateToIdeationSession(activeSessionId);
  }, [activeSessionId]);

  if (!activeSessionId) return null;

  return (
    <ActiveDialog
      key={activeSessionId}
      sessionId={activeSessionId}
      onViewPlan={handleViewPlan}
    />
  );
}
