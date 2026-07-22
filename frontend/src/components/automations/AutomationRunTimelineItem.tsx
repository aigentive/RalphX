import { memo, type ReactNode, useCallback, useState } from "react";
import { ChevronDown, ChevronUp, FileText, Trash2 } from "lucide-react";

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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import { formatDate, numberField, parseRecord, stringField } from "./automationDetailFormat";
import { getAutomationRunJudgeLabel, isAutomationRunDeletable } from "./automationRunView";
import { ExpandableText, Pill } from "./automationDetailShared";

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

function runTimelineHighlight(run: AutomationRun): RunTimelineHighlight {
  if (describeRunFailure(run) || run.status === "cancelled") {
    return {
      backgroundColor: "var(--bg-hover)",
      borderColor: "var(--border-default)",
      markerColor: "var(--text-muted)",
    };
  }
  if (isOpenAutomationRun(run)) {
    return {
      backgroundColor: "var(--accent-muted)",
      borderColor: "var(--accent-border)",
      markerColor: "var(--accent-primary)",
    };
  }
  return {
    backgroundColor: "var(--status-success-muted)",
    borderColor: "var(--status-success-border)",
    markerColor: "var(--status-success)",
  };
}

/** Human-readable diff stats, or null when nothing was recorded (fact omitted). */
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

/** First non-empty line of the agent summary, for the collapsed-card teaser. */
function summaryTeaserLine(summary: string | null): string | null {
  const firstLine = summary?.trim().split(/\r?\n/)[0]?.trim();
  return firstLine ? firstLine : null;
}

interface RunFact {
  label: string;
  content: ReactNode;
  testId?: string;
}

/**
 * Tier-2 compact facts row: only populated facts render — empty fields are
 * omitted entirely instead of showing "Not recorded"-style placeholders. The
 * conversation slot is a persistent action, not a fact, so it always renders.
 */
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
        <code
          className="font-mono text-[0.8125rem]"
          data-testid={`automation-run-${run.id}-branch`}
        >
          {run.branchName}
        </code>
      ),
    });
  }
  const base = run.baseRefUsed || run.baseRefKind;
  if (base) {
    facts.push({ label: "Base", content: base });
  }
  // A settled judge outcome lives here instead of a header badge — the green
  // settled status badge already tells the headline story.
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
      className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-sm"
      data-testid={`automation-run-${run.id}-facts`}
    >
      {facts.length === 0 ? (
        <span style={{ color: "var(--text-muted)" }}>No run details recorded yet.</span>
      ) : (
        facts.map((fact) => (
          <span key={fact.label} className="inline-flex min-w-0 items-center gap-1.5">
            <span
              className="shrink-0 text-xs font-medium uppercase tracking-normal"
              style={{ color: "var(--text-muted)" }}
            >
              {fact.label}
            </span>
            <span
              className="min-w-0 truncate"
              style={{ color: "var(--text-secondary)" }}
              {...(fact.testId ? { "data-testid": fact.testId } : {})}
            >
              {fact.content}
            </span>
          </span>
        ))
      )}
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
    projectId &&
      run.conversationId &&
      (onOpenAutomationRun || onOpenRunConversation),
  );
  const failureReason = describeRunFailure(run);
  const highlight = runTimelineHighlight(run);
  // Boolean annotation defeats the `run is AutomationRun` type-guard aliasing,
  // which would otherwise narrow `run` to `never` in the negated branch.
  const runIsOpen: boolean = isOpenAutomationRun(run);
  const summaryTeaser =
    !expanded && !runIsOpen ? summaryTeaserLine(run.agentSummary) : null;
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
        className="absolute left-0 top-1 h-3 w-3 rounded-full"
        style={{
          backgroundColor: highlight.markerColor,
          borderColor: "var(--app-content-bg)",
          borderStyle: "solid",
          borderWidth: "2px",
        }}
        data-testid={`automation-run-${run.id}-marker`}
      />
      <div
        className="rounded-md p-4"
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
            className="flex min-w-0 flex-1 flex-wrap items-center justify-between gap-3 text-left outline-none focus-visible:outline-none"
          >
            <AutomationRunStatusHeader
              automation={automation} run={run} density="card"
              activeGoalItem={activeGoalItem} showPr={false}
              phaseTestId={`automation-run-${run.id}-phase`}
              testId={`automation-run-${run.id}-header`}
            />
            <span className="flex shrink-0 items-center gap-2">
              <span className="text-xs" style={{ color: "var(--text-muted)" }}>{formatDate(run.updatedAt)}</span>
              <ExpandIcon className="h-4 w-4" aria-hidden="true" style={{ color: "var(--text-muted)" }} />
            </span>
          </button>
          {isLatest && onDeleteRun && isAutomationRunDeletable(run) ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button" variant="ghost" size="icon-sm"
                  aria-label={`${run.status === "running" ? "Stop and delete" : "Delete"} run ${run.runIndex}`}
                  className="shrink-0"
                  style={{ color: "var(--status-error)" }}
                  onClick={() => onDeleteRun?.(run)}
                  data-testid={`automation-run-${run.id}-delete`}
                >
                  <Trash2 className="h-4 w-4" aria-hidden="true" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{run.status === "running" ? "Stop & delete run" : "Delete run"}</TooltipContent>
            </Tooltip>
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

        {/* Tier 1: the failure reason stays visible even while collapsed. */}
        {failureReason && (
          <div
            className={cn(
              "mt-3 rounded-md px-3 py-2 text-sm font-medium",
              !expanded && "truncate",
            )}
            style={{
              backgroundColor: "var(--bg-hover)",
              color: "var(--status-error)",
            }}
            data-testid={`automation-run-${run.id}-failure`}
          >
            {failureReason}
          </div>
        )}

        {/* Tier 1: one-line outcome teaser so a collapsed run answers "what
            happened?"; suppressed while the run is open (the status header
            already narrates live progress). */}
        {summaryTeaser && (
          <p
            className="mt-2 truncate text-sm"
            style={{ color: "var(--text-muted)" }}
            data-testid={`automation-run-${run.id}-summary-teaser`}
          >
            {summaryTeaser}
          </p>
        )}

        {expanded && (
          <div data-testid={`automation-run-${run.id}-body`}>
            {/* Tier 2: decision first, then the human-readable outcome, then
                populated-only facts. */}
            <JudgeVerdictCard run={run} />
            {run.agentSummary?.trim() && (
              <div className="mt-3" data-testid={`automation-run-${run.id}-summary`}>
                <div className="mb-2 text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
                  Agent summary
                </div>
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
            {/* Tier 3: reproducibility/debug data stays fully closed until
                asked for — zero prompt lines render while the toggle is shut. */}
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
