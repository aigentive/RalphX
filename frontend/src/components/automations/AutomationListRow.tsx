import { ChevronRight } from "lucide-react";
import { memo, useCallback } from "react";

import type { Automation, AutomationRun } from "@/api/automations";
import { StatusPill, type StatusPillTone } from "@/components/ui/status-pill";
import { getAutomationRunView, latestRun } from "@/components/automations/automationStage";
import { statusDotColor } from "@/components/automations/automationDetailShared";
import { describePausedReason } from "@/components/automations/automationRunView";
import { useAutomationDetail } from "@/hooks/useAutomations";

const STATUS_LABELS: Record<Automation["status"], string> = {
  draft: "Draft",
  active: "Active",
  paused: "Paused",
  completed: "Completed",
  stopped: "Stopped",
};

const AUTOMATION_STATUS_TONES: Record<Automation["status"], StatusPillTone> = {
  draft: "neutral",
  active: "accent",
  paused: "warning",
  completed: "success",
  stopped: "neutral",
};

function parsePhaseCount(value: string | null): number {
  if (!value?.trim()) {
    return 0;
  }
  try {
    const parsed = JSON.parse(value) as unknown;
    return Array.isArray(parsed) ? parsed.length : 0;
  } catch {
    return 0;
  }
}

function formatSecondaryLine(automation: Automation, stageLabel: string): string {
  if (automation.status === "draft") {
    const phaseCount = parsePhaseCount(automation.goalItemsJson);
    return `Draft setup · ${automation.goalPrompt.trim() ? "Goal set" : "No goal"} · ${phaseCount || "No"} ${phaseCount === 1 ? "phase" : "phases"} · ${automation.firstRunPrompt?.trim() ? "First run ready" : "No first run"}`;
  }
  const phaseCount = parsePhaseCount(automation.goalItemsJson);
  const pausedReason = automation.status === "paused" && automation.pausedReasonCode
    ? describePausedReason(automation.pausedReasonCode)
    : null;
  return [
    pausedReason ?? (stageLabel === "Paused" || stageLabel === "Stopped" ? null : stageLabel),
    phaseCount > 0 ? `${phaseCount} ${phaseCount === 1 ? "phase" : "phases"}` : null,
    automation.runMode,
    automation.modelId,
  ].filter((part): part is string => Boolean(part)).join(" · ");
}

function formatLastRun(automation: Automation, run: AutomationRun | null): string {
  if (!run) {
    return automation.status === "draft" ? "Not started" : "No runs yet";
  }
  return `Run ${run.runIndex} · ${getAutomationRunView(automation, run).statusLabel}`;
}

function progressColor(status: Automation["status"]): string {
  if (status === "active") return "var(--accent-primary)";
  if (status === "completed") return "var(--status-success, #2eb867)";
  if (status === "paused") return "var(--status-warning, #e8a33d)";
  return "var(--text-subtle, #6a6a72)";
}

export const AutomationListRow = memo(function AutomationListRow({
  automation,
  divided,
  onOpenAutomation,
}: {
  automation: Automation;
  divided: boolean;
  onOpenAutomation?: (automationId: string) => void;
}) {
  const detail = useAutomationDetail(automation.id);
  const runs = detail.data?.runs ?? [];
  const run = latestRun(runs);
  const runView = getAutomationRunView(automation, run);
  const pillTone = run ? runView.statusTone : AUTOMATION_STATUS_TONES[automation.status];
  const runsCount = runs.length;
  const showProgress = automation.status !== "draft" && automation.maxRuns > 0 && automation.maxRuns <= 200;
  const progressPercent = Math.min(100, Math.max(0, (runsCount / Math.max(automation.maxRuns, 1)) * 100));
  const handleOpen = useCallback(() => onOpenAutomation?.(automation.id), [automation.id, onOpenAutomation]);

  return (
    <button
      type="button"
      className="group grid w-full gap-x-4 gap-y-2 px-4 py-3 text-left transition-colors hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--accent-primary)] disabled:cursor-default disabled:hover:bg-transparent md:grid-cols-[minmax(13rem,1fr)_6.5rem_7rem_10rem_1rem] md:items-center"
      style={divided ? {
        borderTopColor: "var(--border-subtle, #2e2e36)",
        borderTopStyle: "solid",
        borderTopWidth: "1px",
      } : undefined}
      disabled={!onOpenAutomation}
      onClick={handleOpen}
      data-testid={`automation-row-${automation.id}`}
    >
      <div className="flex min-w-0 items-start gap-2">
        <span
          aria-hidden="true"
          className={automation.status === "active" && runView.isOpen ? "mt-1.5 h-2 w-2 shrink-0 animate-pulse rounded-full" : "mt-1.5 h-2 w-2 shrink-0 rounded-full"}
          style={{ backgroundColor: statusDotColor(automation.status) }}
          data-testid={`automation-row-${automation.id}-status-dot`}
        />
        <div className="min-w-0">
          <div
            className="truncate text-sm font-medium"
            style={{ color: automation.name.trim() ? "var(--text-primary, #f2f2f4)" : "var(--text-muted, #8e8e96)" }}
          >
            {automation.name.trim() || "Untitled automation"}
          </div>
          <div
            className="mt-0.5 truncate text-xs"
            style={{ color: "var(--text-muted, #8e8e96)" }}
            data-testid={`automation-row-${automation.id}-metadata`}
          >
            {formatSecondaryLine(automation, runView.stageLabel)}
          </div>
        </div>
      </div>
      <StatusPill
        label={STATUS_LABELS[automation.status]}
        tone={pillTone}
        variant="tinted"
        live={automation.status === "active" && runView.isOpen}
      />
      <div className="flex min-w-0 items-center gap-2">
        {automation.status === "draft" ? (
          <span className="font-mono text-xs" style={{ color: "var(--text-muted, #8e8e96)" }}>—</span>
        ) : (
          <>
            <span className="font-mono text-xs tabular-nums" style={{ color: "var(--text-secondary, #c7c7cc)" }}>
              {runsCount}/{automation.maxRuns}
            </span>
            {showProgress ? (
              <span
                className="h-1 min-w-10 flex-1 overflow-hidden rounded-full"
                style={{ backgroundColor: "var(--bg-hover, #2a2a31)" }}
                aria-hidden="true"
                data-testid={`automation-row-${automation.id}-runs-progress`}
              >
                <span
                  className="block h-full rounded-full"
                  style={{ backgroundColor: progressColor(automation.status), width: `${progressPercent}%` }}
                  data-testid={`automation-row-${automation.id}-runs-progress-fill`}
                />
              </span>
            ) : null}
          </>
        )}
      </div>
      <div className="flex min-w-0 items-center gap-2 md:justify-end">
        <span
          className="truncate text-xs"
          style={{ color: runView.statusTone === "error" ? "var(--status-error, #dd3c3c)" : "var(--text-muted, #8e8e96)" }}
        >
          {detail.isLoading ? "Loading runs…" : formatLastRun(automation, run)}
        </span>
        {run?.prNumber && runView.statusTone === "success" ? (
          <StatusPill
            label={`PR #${run.prNumber}`}
            tone="success"
            variant="tinted"
            className="shrink-0 font-mono tabular-nums"
          />
        ) : null}
      </div>
      <ChevronRight
        className="hidden h-4 w-4 text-[var(--text-subtle)] transition-colors group-hover:text-[var(--text-secondary)] md:block"
        aria-hidden="true"
      />
    </button>
  );
});
