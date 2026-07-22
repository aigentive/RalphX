import { memo, type ReactNode, useCallback, useState } from "react";
import { ChevronDown, ChevronUp, FileText, Trash2, XCircle } from "lucide-react";

import type { Automation, AutomationRun } from "@/api/automations";
import {
  describeAutomationRunPrState,
  describeRunFailure,
  isOpenAutomationRun,
} from "@/components/automations/automationStage";
import type { AutomationGoalItem } from "@/components/automations/automationGoalItems";
import { AutomationPlanDialog } from "@/components/automations/AutomationPlanDialog";
import { AutomationRunPrLink } from "@/components/automations/AutomationRunPrLink";
import { AutomationRunStatusHeader } from "@/components/automations/AutomationRunStatusHeader";
import { AutomationRunTaskLedger } from "@/components/automations/AutomationRunTaskLedger";
import type { AutomationRunOpenTarget } from "@/components/automations/automationRunNavigation";
import { Button } from "@/components/ui/button";
import { CopyableRef } from "@/components/ui/copyable-ref";
import { NoticeBanner } from "@/components/ui/notice-banner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import { formatDate, numberField, parseRecord, stringField } from "./automationDetailFormat";
import { getAutomationRunJudgeLabel, isAutomationRunDeletable } from "./automationRunView";
import { ExpandableText, FieldLabel, Pill } from "./automationDetailShared";

const PROMPT_AUTHOR_LABELS: Record<AutomationRun["promptAuthor"], string> = {
  setup_agent: "Setup agent",
  judge: "Judge",
  skip_judge_template: "Skip-judge template",
};

interface RunTimelineHighlight {
  backgroundColor: string;
  borderColor: string;
  markerColor: string;
}

function runTimelineHighlight(automation: Automation, run: AutomationRun): RunTimelineHighlight {
  if (describeRunFailure(run)) {
    return {
      backgroundColor: "var(--status-error-muted, rgba(213, 94, 0, 0.1))",
      borderColor: "var(--status-error-border, rgba(213, 94, 0, 0.3))",
      markerColor: "var(--status-error, #d55e00)",
    };
  }
  if (run.status === "cancelled") {
    return {
      backgroundColor: "var(--bg-surface, #1e1e23)",
      borderColor: "var(--border-default, #393940)",
      markerColor: "var(--text-muted, #8e8e96)",
    };
  }
  if (automation.status === "paused" || run.status === "awaiting_plan_approval") {
    return {
      backgroundColor: "var(--status-warning-muted, rgba(224, 179, 65, 0.1))",
      borderColor: "var(--status-warning-border, rgba(224, 179, 65, 0.3))",
      markerColor: "var(--status-warning, #e0b341)",
    };
  }
  if (isOpenAutomationRun(run)) {
    return {
      backgroundColor: "var(--accent-muted, rgba(255, 106, 53, 0.1))",
      borderColor: "var(--accent-border, rgba(255, 106, 53, 0.28))",
      markerColor: "var(--accent-primary, #ff6a35)",
    };
  }
  return {
    backgroundColor: "var(--status-success-muted, rgba(63, 191, 127, 0.1))",
    borderColor: "var(--status-success-border, rgba(63, 191, 127, 0.3))",
    markerColor: "var(--status-success, #3fbf7f)",
  };
}

function formatDiffStats(value: string | null): string | null {
  const stats = parseRecord(value);
  const files = numberField(stats, "filesChanged") ?? numberField(stats, "files_changed");
  const additions = numberField(stats, "additions");
  const deletions = numberField(stats, "deletions");
  if (files === null && additions === null && deletions === null) {
    return null;
  }
  return `${files ?? 0} files, +${additions ?? 0} / -${deletions ?? 0}`;
}

function summaryTeaserLine(summary: string | null): string | null {
  const firstLine = summary?.trim().split(/\r?\n/)[0]?.trim();
  return firstLine ? firstLine : null;
}

interface RunFact { label: string; content: ReactNode; testId?: string }

function RunFactsRow({
  run,
  canOpenConversation,
  onOpenConversation,
  onOpenPlan,
}: {
  run: AutomationRun;
  canOpenConversation: boolean;
  onOpenConversation: () => void;
  onOpenPlan: (() => void) | null;
}) {
  // Only populated facts render; the conversation remains a persistent action.
  const facts: RunFact[] = [];
  if (run.prNumber || run.prUrl) {
    facts.push({
      label: "PR",
      content: describeAutomationRunPrState(run),
      testId: `automation-run-${run.id}-pr-state`,
    });
  }
  const diff = formatDiffStats(run.diffStatsJson);
  if (diff) {
    facts.push({ label: "Diff", content: diff });
  }
  if (run.finishedAt) {
    facts.push({ label: "Finished", content: formatDate(run.finishedAt) });
  }
  if (run.branchName) {
    facts.push({
      label: "Branch",
      content: (
        <CopyableRef
          value={run.branchName}
          ariaLabel="Copy branch"
          testId={`automation-run-${run.id}-branch`}
        />
      ),
    });
  }
  const base = run.baseRefUsed || run.baseRefKind;
  if (base) {
    facts.push({ label: "Base", content: base });
  }
  // Settled judge outcomes stay in facts instead of duplicating the status badge.
  if (run.judgeState === "done" || run.judgeState === "skipped") {
    const judgeLabel = getAutomationRunJudgeLabel(run);
    if (judgeLabel) {
      facts.push({
        label: "Judge",
        content: judgeLabel,
        testId: `automation-run-${run.id}-judge-fact`,
      });
    }
  }
  facts.push({ label: "Prompt", content: PROMPT_AUTHOR_LABELS[run.promptAuthor] });

  return (
    <div
      className="mt-3 flex flex-wrap items-center gap-x-5 gap-y-1.5 pt-3 text-sm"
      style={{
        borderTopColor: "var(--border-subtle, #2e2e36)",
        borderTopStyle: "solid",
        borderTopWidth: "1px",
      }}
      data-testid={`automation-run-${run.id}-facts`}
    >
      {facts.length === 0 ? (
        <span style={{ color: "var(--text-muted)" }}>No run details recorded yet.</span>
      ) : (
        facts.map((fact) => (
          <span key={fact.label} className="inline-flex min-w-0 items-center gap-1.5">
            <FieldLabel className="shrink-0">{fact.label}</FieldLabel>
            <span
              className="min-w-0 truncate tabular-nums"
              style={{ color: "var(--text-secondary)" }}
              {...(fact.testId ? { "data-testid": fact.testId } : {})}
            >
              {fact.content}
            </span>
          </span>
        ))
      )}
      <span className="ml-auto flex shrink-0 items-center gap-1">
        {onOpenPlan ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                aria-label="View run plan"
                className="h-6 w-6 shrink-0"
                onClick={onOpenPlan}
                data-testid={`automation-run-${run.id}-plan-icon`}
              >
                <FileText className="h-3.5 w-3.5" aria-hidden="true" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>View run plan</TooltipContent>
          </Tooltip>
        ) : null}
        <Button
          type="button"
          variant="link"
          className="h-auto p-0 text-sm"
          disabled={!canOpenConversation}
          onClick={onOpenConversation}
        >
          {run.conversationId ? "Open conversation" : "Not started"}
        </Button>
      </span>
    </div>
  );
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
      className="mt-3 rounded-md p-3"
      style={{
        backgroundColor: "var(--bg-hover)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
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
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="gap-2"
            onClick={() => setExpanded((value) => !value)}
          >
            {expanded ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
            {expanded ? "Hide next prompt" : "Show next prompt"}
          </Button>
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
  isLatest, onDeleteRun,
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
  onDeleteRun?: (run: AutomationRun) => void;
  onOpenRunConversation?: (projectId: string, conversationId: string) => void;
  onOpenAutomationRun?: (target: AutomationRunOpenTarget) => void;
  setupConversationId: string | null;
}) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  const [promptOpen, setPromptOpen] = useState(false);
  const [planOpen, setPlanOpen] = useState(false);
  const openPlan = useCallback(() => setPlanOpen(true), []);
  const ExpandIcon = expanded ? ChevronUp : ChevronDown;
  const canOpenConversation = Boolean(
    projectId && run.conversationId && (onOpenAutomationRun || onOpenRunConversation),
  );
  const failureReason = describeRunFailure(run);
  const highlight = runTimelineHighlight(automation, run);
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
    <div className="relative pl-6" data-testid={`automation-run-${run.id}`}>
      <div
        className="absolute left-0 top-[1.125rem] h-3 w-3 rounded-full"
        style={{
          backgroundColor: highlight.markerColor,
          borderColor: "var(--app-content-bg, #18181d)",
          borderStyle: "solid",
          borderWidth: "2px",
        }}
        data-testid={`automation-run-${run.id}-marker`}
      />
      <div
        className={cn("rounded-md", expanded ? "p-4" : "p-3")}
        style={{
          backgroundColor: highlight.backgroundColor,
          borderColor: highlight.borderColor,
          borderStyle: "solid",
          borderWidth: "1px",
        }}
        data-testid={`automation-run-${run.id}-card`}
      >
        <div className="flex items-start gap-1">
          <button
            type="button"
            onClick={() => setExpanded((value) => !value)}
            aria-expanded={expanded}
            aria-label={`${expanded ? "Collapse" : "Expand"} run ${run.runIndex}`}
            className="flex min-w-0 flex-1 flex-wrap items-center justify-between gap-3 pl-2 text-left outline-none focus-visible:outline-none"
          >
            <AutomationRunStatusHeader
              automation={automation} run={run} density="card"
              activeGoalItem={activeGoalItem} showPr={false}
              phaseTestId={`automation-run-${run.id}-phase`}
              testId={`automation-run-${run.id}-header`}
            />
            <span className="flex shrink-0 items-center gap-2">
              <span className={expanded ? "text-xs" : "text-[0.6875rem]"} style={{ color: "var(--text-muted)" }}>{formatDate(run.updatedAt)}</span>
              <ExpandIcon className="h-4 w-4" aria-hidden="true" style={{ color: "var(--text-muted)" }} />
            </span>
          </button>
          {/* Future Resume action slot: keep actions as siblings of the expand toggle. */}
          {isLatest && onDeleteRun && isAutomationRunDeletable(run) ? (
            <div className="flex items-center gap-1">
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button" variant="ghost" size="icon-sm"
                    aria-label={`${run.status === "running" ? "Stop and delete" : "Delete"} run ${run.runIndex}`}
                    className="shrink-0 text-[var(--text-muted)] hover:text-[var(--status-error)]"
                    onClick={() => onDeleteRun?.(run)}
                    data-testid={`automation-run-${run.id}-delete`}
                  >
                    <Trash2 className="h-4 w-4" aria-hidden="true" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>{run.status === "running" ? "Stop & delete run" : "Delete run"}</TooltipContent>
              </Tooltip>
            </div>
          ) : null}
        </div>
        {run.prUrl ? (
          <div className="mt-2">
            <AutomationRunPrLink
              run={run}
              testId={`automation-run-${run.id}-pr-link`}
            />
          </div>
        ) : null}

        {!expanded && (failureReason || summaryTeaser) ? (
          <p
            className="mt-2 min-w-0 truncate text-xs"
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
              <NoticeBanner
                tone="error"
                icon={<XCircle className="h-4 w-4" aria-hidden="true" />}
                title="Failed"
                className="mt-3"
                testId={`automation-run-${run.id}-failure`}
              >
                {failureReason}
              </NoticeBanner>
            ) : null}
            <JudgeVerdictCard run={run} />
            {run.agentSummary?.trim() && (
              <div className="mt-3" data-testid={`automation-run-${run.id}-summary`}>
                <FieldLabel className="mb-2 block">Agent summary</FieldLabel>
                <ExpandableText text={run.agentSummary} maxLines={6} />
              </div>
            )}
            <RunFactsRow
              run={run}
              canOpenConversation={canOpenConversation}
              onOpenConversation={openConversation}
              onOpenPlan={run.planArtifactId ? openPlan : null}
            />
            {run.conversationId && (
              <div className="mt-4">
                <AutomationRunTaskLedger
                  conversationId={run.conversationId}
                  projectId={projectId}
                  runStatus={run.status}
                />
              </div>
            )}
            {/* Debug prompt content stays unmounted until explicitly opened. */}
            {run.runPrompt?.trim() && (
              <div className="mt-3">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="gap-2"
                  aria-expanded={promptOpen}
                  onClick={() => setPromptOpen((value) => !value)}
                  data-testid={`automation-run-${run.id}-prompt-toggle`}
                >
                  {promptOpen ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
                  Run prompt
                </Button>
                {promptOpen && (
                  <div className="mt-2">
                    <ExpandableText text={run.runPrompt} />
                  </div>
                )}
              </div>
            )}
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
