import { memo, type ReactNode, useCallback, useState } from "react";
import { ChevronDown, ChevronRight, ChevronUp, FileText, Play, Trash2, XCircle } from "lucide-react";

import type { Automation, AutomationRun } from "@/api/automations";
import {
  describeAutomationRunPrState,
  describeRunFailure,
  isOpenAutomationRun,
} from "@/components/automations/automationStage";
import type { AutomationGoalItem } from "@/components/automations/automationGoalItems";
import { AutomationPlanDialog } from "@/components/automations/AutomationPlanDialog";
import { AutomationRunStatusHeader } from "@/components/automations/AutomationRunStatusHeader";
import { AutomationRunTaskLedger } from "@/components/automations/AutomationRunTaskLedger";
import type { AutomationRunOpenTarget } from "@/components/automations/automationRunNavigation";
import { Button } from "@/components/ui/button";
import { CopyableRef } from "@/components/ui/copyable-ref";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import { formatDate, numberField, parseRecord, stringField } from "./automationDetailFormat";
import {
  getAutomationRunJudgeLabel,
  isAutomationRunDeletable,
  isAutomationRunResumable,
  runTimelineHighlight,
} from "./automationRunView";
import { ExpandableText, FieldLabel, Pill } from "./automationDetailShared";
const PROMPT_AUTHOR_LABELS: Record<AutomationRun["promptAuthor"], string> = {
  setup_agent: "Setup agent", judge: "Judge", skip_judge_template: "Skip-judge template",
};
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
function RunFactsRow({ run, ownerAgent }: { run: AutomationRun; ownerAgent: string | null }) {
  const facts: RunFact[] = [];
  if (run.finishedAt) {
    facts.push({ label: "Finished", content: formatDate(run.finishedAt) });
  }
  if (ownerAgent) {
    facts.push({
      label: "Agent",
      content: (
        <span className="block truncate font-mono text-[0.8125rem]" title={ownerAgent}
          data-testid={`automation-run-${run.id}-agent`}>
          {ownerAgent}
        </span>
      ),
    });
  }
  if (run.branchName) {
    facts.push({
      label: "Branch",
      content: (
        <CopyableRef value={run.branchName} ariaLabel="Copy branch"
          testId={`automation-run-${run.id}-branch`} />
      ),
    });
  }
  const base = run.baseRefUsed || run.baseRefKind;
  if (base) {
    facts.push({
      label: "Base",
      content: (
        <CopyableRef value={base} ariaLabel="Copy base ref"
          testId={`automation-run-${run.id}-base`} />
      ),
    });
  }
  if (run.judgeState === "done" || run.judgeState === "skipped") {
    const judgeLabel = getAutomationRunJudgeLabel(run);
    if (judgeLabel) {
      facts.push({ label: "Judge", content: judgeLabel,
        testId: `automation-run-${run.id}-judge-fact` });
    }
  }
  const diff = formatDiffStats(run.diffStatsJson);
  if (diff) {
    facts.push({ label: "Diff", content: diff });
  }
  facts.push({ label: "Prompt", content: PROMPT_AUTHOR_LABELS[run.promptAuthor] });
  if (run.prNumber || run.prUrl) {
    facts.push({ label: "PR", content: describeAutomationRunPrState(run),
      testId: `automation-run-${run.id}-pr-state` });
  }
  return (
    <div
      className="mt-4 grid grid-cols-[5rem_minmax(0,1fr)] gap-x-3 gap-y-1.5 pt-3 text-sm sm:grid-cols-[5rem_minmax(0,1fr)_5rem_minmax(0,1fr)] sm:gap-x-4"
      style={{
        borderTopColor: "var(--border-subtle, #2e2e36)",
        borderTopStyle: "solid",
        borderTopWidth: "1px",
      }}
      data-testid={`automation-run-${run.id}-facts`}
    >
      {facts.map((fact) => (
        <div key={fact.label} className="contents">
          <FieldLabel className="self-center">{fact.label}</FieldLabel>
          <span className="min-w-0 truncate tabular-nums"
            style={{ color: "var(--text-secondary, #c7c7cc)" }}
            {...(fact.testId ? { "data-testid": fact.testId } : {})}>
            {fact.content}
          </span>
        </div>
      ))}
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
  isLatest, onDeleteRun, onResumeRun,
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
  onResumeRun?: (run: AutomationRun) => void;
  onOpenRunConversation?: (projectId: string, conversationId: string) => void;
  onOpenAutomationRun?: (target: AutomationRunOpenTarget) => void;
  setupConversationId: string | null;
}) {
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
        className="absolute left-[0.5px] top-[1.125rem] h-2.5 w-2.5 rounded-full"
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
          <span className="pointer-events-none relative z-10 ml-auto flex shrink-0 items-center gap-1">
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
