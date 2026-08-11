import { type ReactNode } from "react";
import {
  ArrowLeft,
  MoreHorizontal,
  Pause,
  Pencil,
  Play,
  PlayCircle,
  RotateCcw,
  SkipForward,
  Square,
  Trash2,
} from "lucide-react";

import type { Automation } from "@/api/automations";
import { isAutomationDeletable } from "@/components/automations/automationStage";
import { AUTOMATION_STATUS_LABELS } from "@/components/automations/automationGoalItems";
import { Button, type ButtonProps } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { StatusPill } from "@/components/ui/status-pill";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatDate } from "./automationDetailFormat";

function TooltipIconButton({
  label,
  tooltip,
  children,
  ...props
}: ButtonProps & { label: string; tooltip?: ReactNode }) {
  const tooltipContent = tooltip ?? label;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          size="icon-sm"
          aria-label={label}
          {...(typeof tooltipContent === "string" ? { title: tooltipContent } : {})}
          {...props}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{tooltipContent}</TooltipContent>
    </Tooltip>
  );
}

/**
 * Live "what's happening now" chip shown in the header while a run is open, so
 * the user sees run progress (e.g. "Run 1 in progress", "Judging") without
 * opening the Runs tab. Design-system StatusPill in accent tone with the
 * pulsing live dot.
 */
function RunStatusChip({
  label,
  testId = "automation-run-status-chip",
}: {
  label: string;
  testId?: string;
}) {
  return (
    <StatusPill
      label={label}
      tone="accent"
      variant="tinted"
      size="md"
      live
      testId={testId}
    />
  );
}

export interface AutomationDetailHeaderProps {
  automation: Automation;
  projectLabel: string;
  liveStageLabel: string | null;
  actionPending: boolean;
  runNowBlockedReason: string | null;
  skipJudgeRunId: string | null;
  canOpenSetupConversation: boolean;
  automationTerminal: boolean;
  onBack: () => void;
  onApprove: () => void;
  onRestart: () => void;
  onResume: () => void;
  onPause: () => void;
  onRunNow: () => void;
  onCancelAutomation: () => void;
  onSkipJudge: (runId: string) => void;
  onEdit: () => void;
  onDelete: () => void;
}

export function AutomationDetailHeader({
  automation,
  projectLabel,
  liveStageLabel,
  actionPending,
  runNowBlockedReason,
  skipJudgeRunId,
  canOpenSetupConversation,
  automationTerminal,
  onBack,
  onApprove,
  onRestart,
  onResume,
  onPause,
  onRunNow,
  onCancelAutomation,
  onSkipJudge,
  onEdit,
  onDelete,
}: AutomationDetailHeaderProps) {
  return (
    <header
      className="flex flex-wrap items-center justify-between gap-4 border-b px-6 py-4"
      style={{
        backgroundColor: "var(--app-content-bg)",
        borderBottomColor: "var(--border-subtle, #2e2e36)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
      }}
    >
      <div className="flex min-w-0 items-center gap-4">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={onBack}
          className="shrink-0 gap-2"
        >
          <ArrowLeft className="h-4 w-4" />
          Back
        </Button>
        <div className="min-w-0">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <h1 className="truncate text-xl font-semibold" style={{ color: "var(--text-primary)" }}>
              {automation.name}
            </h1>
            <StatusPill
              label={AUTOMATION_STATUS_LABELS[automation.status]}
              tone={automation.status === "active" || automation.status === "completed"
                ? "success"
                : automation.status === "paused"
                  ? "warning"
                  : automation.status === "stopped"
                    ? "error"
                    : "neutral"}
              variant="tinted"
              size="md"
              testId="automation-header-status"
            />
            {liveStageLabel && <RunStatusChip label={liveStageLabel} />}
          </div>
          <p className="mt-1 truncate text-xs" style={{ color: "var(--text-muted)" }}>
            {projectLabel} · created {formatDate(automation.createdAt)} · updated{" "}
            {formatDate(automation.updatedAt)}
          </p>
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-2">
        {automation.status === "draft" && (
          <Button
            type="button"
            variant="outline"
            className="gap-2"
            disabled={actionPending}
            onClick={onApprove}
          >
            <PlayCircle className="h-4 w-4" />
            Approve
          </Button>
        )}
        {automation.status === "stopped" ? (
          <Button
            type="button"
            variant="outline"
            className="gap-2"
            disabled={actionPending}
            onClick={onRestart}
          >
            <RotateCcw className="h-4 w-4" aria-hidden="true" />
            Restart automation
          </Button>
        ) : null}
        {automation.status === "paused" ? (
          <TooltipIconButton
            label="Resume automation"
            variant="outline"
            disabled={actionPending}
            onClick={onResume}
          >
            <Play className="h-4 w-4" />
          </TooltipIconButton>
        ) : (
          <TooltipIconButton
            label="Pause automation"
            variant="outline"
            disabled={actionPending || automation.status !== "active"}
            onClick={onPause}
          >
            <Pause className="h-4 w-4" />
          </TooltipIconButton>
        )}
        <TooltipIconButton
          label="Run now"
          {...(runNowBlockedReason ? { tooltip: runNowBlockedReason } : {})}
          variant="outline"
          disabled={actionPending || runNowBlockedReason !== null}
          onClick={onRunNow}
        >
          <PlayCircle className="h-4 w-4" />
        </TooltipIconButton>
        <TooltipIconButton
          label="Cancel automation"
          variant="outline"
          disabled={actionPending || automationTerminal}
          onClick={onCancelAutomation}
        >
          <Square className="h-4 w-4" />
        </TooltipIconButton>
        <DropdownMenu>
          <Tooltip>
            <TooltipTrigger asChild>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  size="icon-sm"
                  variant="outline"
                  aria-label="More automation actions"
                  disabled={actionPending}
                >
                  <MoreHorizontal className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
            </TooltipTrigger>
            <TooltipContent>More automation actions</TooltipContent>
          </Tooltip>
          <DropdownMenuContent align="end">
            <DropdownMenuItem
              disabled={!skipJudgeRunId}
              onSelect={(event) => {
                event.preventDefault();
                if (skipJudgeRunId) {
                  onSkipJudge(skipJudgeRunId);
                }
              }}
            >
              <SkipForward className="h-4 w-4" />
              Skip judge
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={!canOpenSetupConversation}
              onSelect={(event) => {
                event.preventDefault();
                onEdit();
              }}
            >
              <Pencil className="h-4 w-4" />
              Edit
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              disabled={!isAutomationDeletable(automation.status)}
              className="text-[var(--status-error)]"
              onSelect={(event) => {
                event.preventDefault();
                onDelete();
              }}
            >
              <Trash2 className="h-4 w-4" />
              Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </header>
  );
}
