/**
 * TeamPlanApproval — Inline approval banner for team plans
 *
 * Shows when a team lead requests plan approval via request_team_plan.
 * Displays teammate composition and approve/reject buttons.
 * Auto-rejects after 15 minutes with countdown display.
 */

import React, { useCallback, useEffect, useRef, useState } from "react";
import { Check, X, Users } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { withAlpha } from "@/lib/theme-colors";
import { approveTeamPlan, rejectTeamPlan } from "@/api/team";
import { useTeamStore } from "@/stores/teamStore";
import type { PendingTeamPlan } from "@/stores/teamStore";
import type { ContextType } from "@/types/chat-conversation";

const PLAN_TIMEOUT_MS = 900_000; // 15 minutes
const EXPIRED_DISPLAY_MS = 2_000; // show "Expired" for 2s before clearing

interface TeamPlanApprovalProps {
  plan: PendingTeamPlan;
  contextKey: string;
}

function formatCountdown(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

export const TeamPlanApproval = React.memo(function TeamPlanApproval({
  plan,
  contextKey,
}: TeamPlanApprovalProps) {
  const clearPendingPlan = useTeamStore((s) => s.clearPendingPlan);
  const [isApproving, setIsApproving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expired, setExpired] = useState(false);
  const [remainingMs, setRemainingMs] = useState(() =>
    Math.max(0, PLAN_TIMEOUT_MS - (Date.now() - plan.createdAt)),
  );

  const expiredTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const clearTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Auto-reject timer and countdown
  useEffect(() => {
    const initialRemaining = Math.max(
      0,
      PLAN_TIMEOUT_MS - (Date.now() - plan.createdAt),
    );

    if (initialRemaining <= 0) {
      // Already expired on mount
      setRemainingMs(0);
      setExpired(true);
      rejectTeamPlan(plan.planId).catch(() => {
        // Best-effort
      });
      clearTimerRef.current = setTimeout(() => {
        clearPendingPlan(contextKey);
      }, EXPIRED_DISPLAY_MS);
      return;
    }

    // Countdown interval
    countdownRef.current = setInterval(() => {
      const remaining = Math.max(
        0,
        PLAN_TIMEOUT_MS - (Date.now() - plan.createdAt),
      );
      setRemainingMs(remaining);
    }, 1000);

    // Auto-reject after remaining time
    expiredTimerRef.current = setTimeout(async () => {
      if (countdownRef.current) {
        clearInterval(countdownRef.current);
        countdownRef.current = null;
      }
      setRemainingMs(0);
      setExpired(true);
      rejectTeamPlan(plan.planId).catch(() => {
        // Best-effort
      });
      clearTimerRef.current = setTimeout(() => {
        clearPendingPlan(contextKey);
      }, EXPIRED_DISPLAY_MS);
    }, initialRemaining);

    return () => {
      if (countdownRef.current) clearInterval(countdownRef.current);
      if (expiredTimerRef.current) clearTimeout(expiredTimerRef.current);
      if (clearTimerRef.current) clearTimeout(clearTimerRef.current);
    };
  }, [plan.planId, plan.createdAt, contextKey, clearPendingPlan]);

  const handleApprove = useCallback(async () => {
    setIsApproving(true);
    setError(null);
    try {
      await approveTeamPlan(
        plan.planId,
        plan.originContextType as ContextType,
        plan.originContextId,
      );
      clearPendingPlan(contextKey);
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Failed to approve team plan";
      const isExpiredError =
        msg.toLowerCase().includes("plan expired") ||
        msg.toLowerCase().includes("expired");
      if (isExpiredError) {
        toast.error(
          "Plan already expired — agent already received timeout response",
        );
        clearPendingPlan(contextKey);
      } else {
        setError(msg);
        setIsApproving(false);
      }
    }
  }, [
    plan.planId,
    plan.originContextType,
    plan.originContextId,
    contextKey,
    clearPendingPlan,
  ]);

  const handleReject = useCallback(async () => {
    try {
      await rejectTeamPlan(plan.planId);
    } catch {
      // Best-effort — clear UI regardless
    }
    clearPendingPlan(contextKey);
  }, [plan.planId, contextKey, clearPendingPlan]);

  // Expired state — show briefly before clearing
  if (expired) {
    return (
      <div
        className="mx-3 my-2 rounded-lg overflow-hidden"
        style={{
          backgroundColor: "var(--bg-surface)",
          border: "1px solid var(--bg-hover)",
        }}
      >
        <div
          className="flex items-center gap-2 px-3 py-2.5"
          style={{ backgroundColor: "var(--bg-surface)" }}
        >
          <Users className="w-3.5 h-3.5" style={{ color: "var(--text-muted)" }} />
          <span className="text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
            Team Plan — {plan.process}
          </span>
          <span
            className="ml-auto text-[0.625rem] px-1.5 py-0.5 rounded"
            style={{
              backgroundColor: "var(--bg-elevated)",
              color: "var(--text-muted)",
            }}
          >
            Expired
          </span>
        </div>
      </div>
    );
  }

  return (
    <div
      className="mx-3 my-2 rounded-lg overflow-hidden"
      style={{
        backgroundColor: "var(--bg-surface)",
        border: `1px solid ${withAlpha("var(--accent-primary)", 40)}`,
      }}
    >
      {/* Header */}
      <div
        className="flex items-center gap-2 px-3 py-2"
        style={{
          backgroundColor: withAlpha("var(--accent-primary)", 8),
          borderBottom: "1px solid var(--border-subtle)",
        }}
      >
        <Users className="w-3.5 h-3.5" style={{ color: "var(--accent-primary)" }} />
        <span className="text-[0.6875rem] font-medium" style={{ color: "var(--accent-primary)" }}>
          Team Plan — {plan.process}
        </span>
        <span
          className="ml-auto text-[0.625rem] px-1.5 rounded"
          style={{
            backgroundColor: "var(--bg-elevated)",
            color: "var(--text-secondary)",
          }}
        >
          {plan.teammates.length} teammate{plan.teammates.length !== 1 ? "s" : ""}
        </span>
      </div>

      {/* Teammate list */}
      <div className="px-3 py-2 space-y-1.5">
        {plan.teammates.map((mate, i) => (
          <div
            key={`${mate.role}-${i}`}
            className="flex items-center gap-2"
          >
            <span
              className="w-1.5 h-1.5 rounded-full shrink-0"
              style={{ backgroundColor: "var(--text-muted)" }}
            />
            <span className="text-[0.6875rem] font-medium" style={{ color: "var(--text-secondary)" }}>
              {mate.role}
            </span>
            <span
              className="text-[0.625rem] px-1 rounded"
              style={{
                backgroundColor: "var(--bg-elevated)",
                color: "var(--text-muted)",
              }}
            >
              {mate.model}
            </span>
            {mate.prompt_summary && (
              <span
                className="text-[0.625rem] truncate flex-1"
                style={{ color: "var(--text-muted)" }}
              >
                {mate.prompt_summary}
              </span>
            )}
          </div>
        ))}
      </div>

      {/* Error */}
      {error && (
        <div className="px-3 pb-1">
          <span className="text-[0.625rem]" style={{ color: "var(--status-error)" }}>
            {error}
          </span>
        </div>
      )}

      {/* Actions */}
      <div
        className="flex items-center justify-between gap-2 px-3 py-2"
        style={{ borderTop: "1px solid var(--border-subtle)" }}
      >
        {/* Countdown */}
        <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
          Expires in {formatCountdown(remainingMs)}
        </span>

        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={handleReject}
            disabled={isApproving}
            className="text-[0.6875rem] h-7 gap-1"
          >
            <X className="w-3 h-3" />
            Reject
          </Button>
          <Button
            size="sm"
            onClick={handleApprove}
            disabled={isApproving}
            className="text-[0.6875rem] h-7 gap-1"
            style={{
              backgroundColor: "var(--accent-primary)",
              color: "white",
            }}
          >
            <Check className="w-3 h-3" />
            {isApproving ? "Approving..." : "Approve"}
          </Button>
        </div>
      </div>
    </div>
  );
});
