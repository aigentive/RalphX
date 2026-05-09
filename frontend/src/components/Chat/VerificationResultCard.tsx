import { AlertTriangle, ChevronRight, RotateCcw, Wrench } from "lucide-react";
import { useState } from "react";
import { withAlpha } from "@/lib/theme-colors";

interface VerificationResultBlocker {
  severity: string;
  description: string;
}

interface VerificationResultCardProps {
  summary: string;
  convergenceReason?: string | null;
  currentRound?: number | null;
  maxRounds?: number | null;
  recommendedNextAction?: string | null;
  blockers?: VerificationResultBlocker[];
  actionableForParent?: boolean;
}

function formatActionLabel(action?: string | null): string {
  switch (action) {
    case "revise_plan":
      return "Revise plan";
    case "explore_code_paths":
      return "Explore code paths";
    case "rerun_verification":
      return "Re-run verification";
    default:
      return "Review result";
  }
}

function formatReasonLabel(reason?: string | null): string | null {
  if (!reason) return null;
  switch (reason) {
    case "max_rounds":
      return "Max rounds";
    case "jaccard_converged":
      return "Stable blockers";
    case "gap_score_plateau":
      return "Gap plateau";
    case "agent_error":
      return "Verifier runtime issue";
    case "agent_crashed_mid_round":
      return "Verifier interrupted";
    case "agent_completed_without_update":
      return "No verifier output";
    case "critic_parse_failure":
      return "Critic parse failure";
    default:
      return reason.replace(/_/g, " ");
  }
}

export function VerificationResultCard({
  summary,
  convergenceReason,
  currentRound,
  maxRounds,
  recommendedNextAction,
  blockers = [],
  actionableForParent = false,
}: VerificationResultCardProps) {
  const [expanded, setExpanded] = useState(false);
  const reasonLabel = formatReasonLabel(convergenceReason);
  const actionLabel = formatActionLabel(recommendedNextAction);
  const leadIcon = actionableForParent ? RotateCcw : Wrench;
  const LeadIcon = leadIcon;

  return (
    <div className="flex flex-col items-center py-1">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1.5 px-2.5 py-[3px] rounded-xl transition-colors"
        style={{ background: expanded ? "var(--bg-surface)" : "transparent" }}
        onMouseEnter={(e) => {
          if (!expanded) e.currentTarget.style.background = "var(--bg-surface)";
        }}
        onMouseLeave={(e) => {
          if (!expanded) e.currentTarget.style.background = "transparent";
        }}
      >
        <LeadIcon
          className="w-[11px] h-[11px]"
          style={{ color: actionableForParent ? "var(--accent-primary)" : "var(--text-muted)" }}
        />
        <span
          className="text-[0.6875rem] leading-none"
          style={{ color: "var(--text-muted)", fontFamily: "var(--font-body)" }}
        >
          Verification result
        </span>
        <ChevronRight
          className="w-[11px] h-[11px] transition-transform"
          style={{
            color: "var(--text-muted)",
            transform: expanded ? "rotate(90deg)" : "rotate(0deg)",
          }}
        />
      </button>
      {expanded && (
        <div
          className="mt-1 mx-4 w-full max-w-[560px] rounded-lg px-3 py-2 text-[0.6875rem] leading-relaxed space-y-2"
          style={{
            background: "var(--bg-surface)",
            color: "var(--text-secondary)",
            fontFamily: "var(--font-body)",
            wordBreak: "break-word",
          }}
        >
          <div className="flex items-center gap-2 flex-wrap">
            <span
              className="px-2 py-0.5 rounded-full text-[0.625rem]"
              style={{
                background: actionableForParent ? withAlpha("var(--accent-primary)", 18) : "var(--overlay-weak)",
                color: actionableForParent ? "var(--accent-primary)" : "var(--text-muted)",
              }}
            >
              {actionableForParent ? "Actionable for plan" : "Infra/runtime issue"}
            </span>
            {currentRound != null && maxRounds != null && (
              <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
                Round {currentRound}/{maxRounds}
              </span>
            )}
            {reasonLabel && (
              <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
                {reasonLabel}
              </span>
            )}
          </div>

          <div style={{ color: "var(--text-primary)" }}>{summary}</div>

          {blockers.length > 0 && (
            <div className="space-y-1">
              {blockers.slice(0, 3).map((blocker, index) => (
                <div
                  key={`${blocker.severity}-${index}`}
                  className="flex items-start gap-2 rounded-md px-2 py-1.5"
                  style={{ background: "var(--overlay-faint)" }}
                >
                  <AlertTriangle
                    className="w-3 h-3 mt-[1px] shrink-0"
                    style={{ color: blocker.severity === "critical" ? "var(--accent-primary)" : "var(--text-muted)" }}
                  />
                  <div>
                    <div className="text-[0.625rem] uppercase tracking-[0.08em]" style={{ color: "var(--text-muted)" }}>
                      {blocker.severity}
                    </div>
                    <div>{blocker.description}</div>
                  </div>
                </div>
              ))}
            </div>
          )}

          <div className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
            Recommended next action: {actionLabel}
          </div>
        </div>
      )}
    </div>
  );
}
