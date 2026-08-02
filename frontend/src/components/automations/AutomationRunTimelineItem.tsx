import { memo, useCallback, useState } from "react";
import { ChevronDown, ChevronRight, ChevronUp, FileText, Play, Trash2, XCircle } from "lucide-react";

import type { Automation, AutomationRun } from "@/api/automations";
import {
  describeRunFailure,
  isOpenAutomationRun,
} from "@/components/automations/automationStage";
import type { AutomationGoalItem } from "@/components/automations/automationGoalItems";
import { AutomationPlanDialog } from "@/components/automations/AutomationPlanDialog";
import { AutomationRunMilestoneList } from "@/components/automations/AutomationRunMilestoneList";
import { AutomationRunStatusHeader } from "@/components/automations/AutomationRunStatusHeader";
import { AutomationRunTaskLedger } from "@/components/automations/AutomationRunTaskLedger";
import type { AutomationRunOpenTarget } from "@/components/automations/automationRunNavigation";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useAgentGate } from "@/hooks/useAgentGate";
import { cn } from "@/lib/utils";

import {
  formatDate,
  formatDuration,
  numberField,
  parseRecord,
  stringField,
} from "./automationDetailFormat";
import {
  isAutomationRunDeletable,
  isAutomationRunResumable,
  runTimelineHighlight,
} from "./automationRunView";
import { RunFactsRow } from "./AutomationRunFacts";
import { ExpandableText, FieldLabel, Pill } from "./automationDetailShared";
function summaryTeaserLine(summary: string | null): string | null {
  const firstLine = summary?.trim().split(/\r?\n/)[0]?.trim();
  return firstLine ? firstLine : null;
}
function JudgeVerdictCard({ run }: { run: AutomationRun }) {
  const [expanded, setExpanded] = useState(false);
  const verdict = parseRecord(run.judgeVerdictJson);
  if (!verdict) {
    return null;
  }
  const decision = stringField(verdict, "decision") ?? "unknown";
  const reason = stringField(verdict, "reason");
  const confidence = numberField(verdict, "confidence");
  const nextRunPrompt = stringField(verdict, "nextRunPrompt");
  return (
    <div
      className="mt-3 max-w-[65ch] pl-3"
      style={{
        borderLeftColor: "var(--border-default, #393940)",
        borderLeftStyle: "solid",
        borderLeftWidth: "2px",
      }}
      data-testid={`automation-run-${run.id}-judge`}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Pill label={`Judge: ${decision}`} status={decision} />
        {confidence !== null && (
          <span className="text-xs" style={{ color: "var(--text-muted)" }}>
            Confidence {Math.round(confidence * 100)}%
          </span>
        )}
      </div>
      {reason && (
        <p className="mt-2 text-sm" style={{ color: "var(--text-secondary)" }}>
          {reason}
        </p>
      )}
      {nextRunPrompt && (
        <div className="mt-2">
          <button
            type="button"
            className="inline-flex items-center gap-1 border-0 bg-transparent p-0 text-xs font-normal text-[var(--text-muted)] outline-none transition-colors hover:text-[var(--text-secondary)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
            onClick={() => setExpanded((value) => !value)}
          >
            {expanded ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
            {expanded ? "Hide next prompt" : "Show next prompt"}
          </button>
          {expanded && (
            <div className="mt-2">
              <ExpandableText text={nextRunPrompt} maxLines={8} />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export const RunTimelineItem = memo(function RunTimelineItem({
  run,
  automation,
  projectId,
  defaultExpanded,
  activeGoalItem,
  isLatest, isLastInTimeline = false, onDeleteRun, onResumeRun,
  onOpenRunConversation,
  onOpenAutomationRun,
  setupConversationId,
}: {
  run: AutomationRun;
  automation: Automation;
  projectId: string | null;
  defaultExpanded: boolean;
  activeGoalItem: AutomationGoalItem | null;
  isLatest?: boolean;
  /** Suppresses the spine connector on the oldest card so the rail ends cleanly. */
  isLastInTimeline?: boolean;
  onDeleteRun?: (run: AutomationRun) => void;
  onResumeRun?: (run: AutomationRun) => void;
  onOpenRunConversation?: (projectId: string, conversationId: string) => void;
  onOpenAutomationRun?: (target: AutomationRunOpenTarget) => void;
  setupConversationId: string | null;
}) {
  const deleteRunGate = useAgentGate("automationDeleteRun");
  const resumeRunGate = useAgentGate("automationResumeRun");
  const [expanded, setExpanded] = useState(defaultExpanded);
  const [promptOpen, setPromptOpen] = useState(false);
  const [planOpen, setPlanOpen] = useState(false);
  const [ownerAgent, setOwnerAgent] = useState<string | null>(null);
  const openPlan = useCallback(() => setPlanOpen(true), []);
  const updateOwnerAgent = useCallback((nextOwnerAgent: string | null) => {
    setOwnerAgent((current) => current === nextOwnerAgent ? current : nextOwnerAgent);
  }, []);
  const ExpandIcon = expanded ? ChevronUp : ChevronDown;
  const canOpenConversation = Boolean(
    projectId && run.conversationId && (onOpenAutomationRun || onOpenRunConversation),
  );
  const failureReason = describeRunFailure(run);
  const highlight = runTimelineHighlight(run);
  // Settled runs only: a live run has no end bound, and we do not tick a clock here.
  const duration = formatDuration(run.startedAt, run.finishedAt);
  // The annotation prevents type-guard aliasing from narrowing the negated branch.
  const runIsOpen: boolean = isOpenAutomationRun(run);
  const summaryTeaser = !expanded && !runIsOpen ? summaryTeaserLine(run.agentSummary) : null;
  const openConversation = useCallback(() => {
    if (projectId && run.conversationId) {
      if (onOpenAutomationRun) {
        onOpenAutomationRun({
          projectId,
          automationId: run.automationId,
          runId: run.id,
          conversationId: run.conversationId,
          setupConversationId,
          runStatus: run.status,
          judgeState: run.judgeState,
          planPhase: run.planPhase,
          planArtifactId: run.planArtifactId,
          prNumber: run.prNumber,
          prUrl: run.prUrl,
        });
        return;
      }
      onOpenRunConversation?.(projectId, run.conversationId);
    }
  }, [
    onOpenAutomationRun,
    onOpenRunConversation,
    projectId,
    run.automationId,
    run.conversationId,
    run.id,
    run.judgeState,
    run.planArtifactId,
    run.planPhase,
    run.prNumber,
    run.prUrl,
    run.status,
    setupConversationId,
  ]);

  return (
    <div className="flex min-w-0 gap-3" data-testid={`automation-run-${run.id}`}>
      {/* Timeline spine: status marker plus the connector down to the next run. */}
      <div className="flex w-2.5 shrink-0 flex-col items-center" aria-hidden="true">
        <span
          className={cn(
            "mt-[1.0625rem] h-2.5 w-2.5 shrink-0 rounded-full",
            highlight.live && "animate-pulse",
          )}
          style={{ backgroundColor: highlight.markerColor }}
          data-testid={`automation-run-${run.id}-marker`}
        />
        {!isLastInTimeline ? (
          <span
            className="mt-1 w-px flex-1"
            style={{ backgroundColor: "var(--border-subtle, #2e2e36)" }}
            data-testid={`automation-run-${run.id}-connector`}
          />
        ) : null}
      </div>
      <div
        className={cn("min-w-0 flex-1 rounded-lg", expanded ? "p-4" : "px-4 py-3")}
        style={{
          // Per-side longhands: WKWebView drops mixed shorthand/longhand borders.
          backgroundColor: highlight.backgroundColor,
          borderStyle: "solid",
          borderTopColor: highlight.borderColor,
          borderRightColor: highlight.borderColor,
          borderBottomColor: highlight.borderColor,
          borderTopWidth: "1px",
          borderRightWidth: "1px",
          borderBottomWidth: "1px",
          borderLeftColor: highlight.accentColor,
          borderLeftWidth: "3px",
        }}
        data-testid={`automation-run-${run.id}-card`}
      >
        <div
          className={cn(
            "relative flex min-h-7 min-w-0 gap-3",
            expanded ? "items-start" : "flex-nowrap items-center",
          )}
          data-testid={`automation-run-${run.id}-header-row`}
        >
          <button
            type="button"
            onClick={() => setExpanded((value) => !value)}
            aria-expanded={expanded}
            aria-label={`${expanded ? "Collapse" : "Expand"} run ${run.runIndex}`}
            className="absolute inset-0 z-0 rounded-sm text-left outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
          >
            <span className="sr-only">{expanded ? "Collapse" : "Expand"} run {run.runIndex}</span>
          </button>
          <AutomationRunStatusHeader automation={automation} run={run} density="card"
            activeGoalItem={activeGoalItem} showPr
            prTestId={`automation-run-${run.id}-pr-link`}
            phaseTestId={`automation-run-${run.id}-phase`}
            className={cn(
              "pointer-events-none relative z-10 flex-1 gap-2",
              !expanded && "flex-nowrap overflow-hidden",
            )}
            testId={`automation-run-${run.id}-header`}
          />
          <span className="pointer-events-none relative z-10 ml-auto flex shrink-0 items-center gap-2.5">
            {duration ? (
              <span
                className={cn(
                  "whitespace-nowrap tabular-nums",
                  expanded ? "text-xs" : "text-[0.6875rem]",
                )}
                style={{ color: "var(--text-subtle, #6a6a72)" }}
                data-testid={`automation-run-${run.id}-duration`}
              >
                {duration}
              </span>
            ) : null}
            <span
              className={cn("whitespace-nowrap", expanded ? "text-xs" : "text-[0.6875rem]")}
              style={{ color: "var(--text-muted)" }}
              data-testid={`automation-run-${run.id}-updated-at`}
            >
              {formatDate(run.updatedAt)}
            </span>
            <ExpandIcon className="h-4 w-4" aria-hidden="true"
              style={{ color: "var(--text-muted)" }} />
          </span>
        </div>

        {!expanded && (failureReason || summaryTeaser) ? (
          <p
            className="mt-1.5 min-w-0 truncate text-xs"
            style={{ color: "var(--text-secondary, #c7c7cc)" }}
            data-testid={failureReason
              ? `automation-run-${run.id}-failure`
              : `automation-run-${run.id}-summary-teaser`}
          >
            {failureReason ? (
              <XCircle
                className="mr-1.5 inline h-3.5 w-3.5 align-[-0.125rem]"
                style={{ color: "var(--status-error, #d55e00)" }}
                aria-hidden="true"
              />
            ) : null}
            {failureReason}
            {failureReason && summaryTeaser ? ` · ${summaryTeaser}` : summaryTeaser}
          </p>
        ) : null}

        {expanded && (
          <div data-testid={`automation-run-${run.id}-body`}>
            {failureReason ? (
              <div
                className="mt-3 max-w-[65ch] pl-3"
                style={{
                  borderLeftColor: "var(--status-error, #d55e00)",
                  borderLeftStyle: "solid",
                  borderLeftWidth: "2px",
                }}
                data-testid={`automation-run-${run.id}-failure`}
              >
                <p className="text-sm" style={{ color: "var(--text-secondary, #c7c7cc)" }}>
                  <XCircle
                    className="mr-1.5 inline h-3.5 w-3.5 align-[-0.125rem]"
                    style={{ color: "var(--status-error, #d55e00)" }}
                    aria-hidden="true"
                  />
                  <span
                    className="font-semibold"
                    style={{ color: "var(--status-error, #d55e00)" }}
                  >
                    Failed
                  </span>
                  {" — "}
                  {failureReason}
                </p>
              </div>
            ) : null}
            <JudgeVerdictCard run={run} />
            {run.agentSummary?.trim() && (
              <div className="mt-3" data-testid={`automation-run-${run.id}-summary`}>
                <FieldLabel className="mb-2 block">Agent summary</FieldLabel>
                <ExpandableText text={run.agentSummary} maxLines={6} />
              </div>
            )}
            <RunFactsRow run={run} ownerAgent={ownerAgent} />
            <div className="mt-4">
              <FieldLabel>Progress</FieldLabel>
              <AutomationRunMilestoneList run={run} />
            </div>
            {run.conversationId && (
              <div className="mt-4">
                <AutomationRunTaskLedger
                  conversationId={run.conversationId}
                  projectId={projectId}
                  runStatus={run.status}
                  onOwnerAgentChange={updateOwnerAgent}
                />
              </div>
            )}
            <div
              className="mt-4 flex items-center justify-between gap-3 pt-3"
              style={{
                borderTopColor: "var(--border-subtle, #2e2e36)",
                borderTopStyle: "solid",
                borderTopWidth: "1px",
              }}
              data-testid={`automation-run-${run.id}-footer`}
            >
              <div className="flex min-w-0 items-center gap-1">
                {isLatest && onDeleteRun && isAutomationRunDeletable(run) ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`${run.status === "running" ? "Stop and delete" : "Delete"} run ${run.runIndex}`}
                        className="-ml-1 h-7 w-7 shrink-0 text-[var(--text-muted)] hover:bg-transparent hover:text-[var(--status-error)]"
                        disabled={deleteRunGate.gated}
                        title={deleteRunGate.reason ?? undefined}
                        onClick={() => onDeleteRun?.(run)}
                        data-testid={`automation-run-${run.id}-delete`}
                      >
                        <Trash2 className="h-4 w-4" aria-hidden="true" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>
                      {run.status === "running" ? "Stop & delete run" : "Delete run"}
                    </TooltipContent>
                  </Tooltip>
                ) : null}
                {run.runPrompt.trim() ? (
                  <button
                    type="button"
                    className="inline-flex items-center gap-1 border-0 bg-transparent p-0 text-xs font-medium text-[var(--text-muted)] outline-none transition-colors hover:text-[var(--text-secondary)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
                    aria-expanded={promptOpen}
                    onClick={() => setPromptOpen((value) => !value)}
                    data-testid={`automation-run-${run.id}-prompt-toggle`}
                  >
                    {promptOpen ? (
                      <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
                    ) : (
                      <ChevronRight className="h-3.5 w-3.5" aria-hidden="true" />
                    )}
                    Run prompt
                  </button>
                ) : null}
              </div>
              <div className="flex shrink-0 items-center gap-2">
                {run.planArtifactId ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        aria-label="View run plan"
                        className="h-7 w-7 shrink-0"
                        onClick={openPlan}
                        data-testid={`automation-run-${run.id}-plan-icon`}
                      >
                        <FileText className="h-4 w-4" aria-hidden="true" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>View run plan</TooltipContent>
                  </Tooltip>
                ) : null}
                {!run.conversationId ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span
                        className="inline-flex"
                        data-testid={`automation-run-${run.id}-conversation-disabled-trigger`}
                      >
                        <Button type="button" variant="outline" size="sm" disabled>
                          Open conversation
                        </Button>
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>Run has not started</TooltipContent>
                  </Tooltip>
                ) : (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={!canOpenConversation}
                    onClick={openConversation}
                  >
                    Open conversation
                  </Button>
                )}
                {isLatest && onResumeRun && isAutomationRunResumable(run) ? (
                  <Button
                    type="button"
                    variant="default"
                    size="sm"
                    aria-label={`Resume run ${run.runIndex}`}
                    disabled={resumeRunGate.gated}
                    title={resumeRunGate.reason ?? undefined}
                    onClick={() => onResumeRun?.(run)}
                    data-testid={`automation-run-${run.id}-resume`}
                  >
                    <Play className="h-3.5 w-3.5" aria-hidden="true" />
                    Resume
                  </Button>
                ) : null}
              </div>
            </div>
            {/* Prompt content stays unmounted until explicitly opened. */}
            {promptOpen && run.runPrompt.trim() ? (
              <div className="mt-3">
                <ExpandableText text={run.runPrompt} />
              </div>
            ) : null}
          </div>
        )}
      </div>
      {run.planArtifactId ? (
        <AutomationPlanDialog
          planArtifactId={run.planArtifactId}
          title={`Run ${run.runIndex}`}
          open={planOpen}
          onOpenChange={setPlanOpen}
        />
      ) : null}
    </div>
  );
});
