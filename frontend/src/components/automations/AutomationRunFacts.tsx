import type { ReactNode } from "react";

import type { AutomationRun } from "@/api/automations";
import { describeAutomationRunPrState } from "@/components/automations/automationStage";
import { CopyableRef } from "@/components/ui/copyable-ref";

import { formatDate, numberField, parseRecord } from "./automationDetailFormat";
import { getAutomationRunJudgeLabel } from "./automationRunView";
import { FieldLabel } from "./automationDetailShared";

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
interface RunFact { label: string; content: ReactNode; testId?: string }

/**
 * Run metadata grid for the expanded Runs-timeline card: each fact stacks a
 * micro-label above its value in a recessed two-column panel.
 */
export function RunFactsRow({
  run,
  ownerAgent,
}: {
  run: AutomationRun;
  ownerAgent: string | null;
}) {
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
      className="mt-3 grid grid-cols-1 gap-x-6 gap-y-2.5 rounded-md px-3.5 py-3 text-sm sm:grid-cols-2"
      style={{
        backgroundColor: "var(--bg-sunken, #15151a)",
        borderColor: "var(--border-subtle, #2e2e36)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid={`automation-run-${run.id}-facts`}
    >
      {facts.map((fact) => (
        <div key={fact.label} className="flex min-w-0 flex-col gap-0.5">
          <FieldLabel>{fact.label}</FieldLabel>
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
