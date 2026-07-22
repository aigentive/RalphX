import type { Automation, AutomationRun } from "@/api/automations";
import {
  CANCELLED_RUN_RESTART_DESCRIPTION,
  isOpenAutomationRun,
} from "@/components/automations/automationStage";
import type { AutomationGoalItem } from "@/components/automations/automationGoalItems";
import type { AutomationJudgeRecovery } from "@/components/automations/automationRunView";
import type { AutomationRunOpenTarget } from "@/components/automations/automationRunNavigation";
import { RunTimelineItem } from "@/components/automations/AutomationRunTimelineItem";
import { Button } from "@/components/ui/button";
import { Section } from "./automationDetailShared";

function sortedNewestRuns(runs: AutomationRun[]): AutomationRun[] {
  return [...runs].sort((a, b) => b.runIndex - a.runIndex);
}

export interface AutomationRunsTabProps {
  automation: Automation;
  runs: AutomationRun[];
  latest: AutomationRun | null;
  activeGoalItem: AutomationGoalItem | null;
  projectId: string | null;
  judgeRecovery: AutomationJudgeRecovery | null;
  idleAfterCancelledRun: boolean;
  actionPending: boolean;
  onRetryPlanJudge: () => void;
  onRetryJudge: () => void;
  onRunNow: () => void;
  onOpenRunConversation?: (projectId: string, conversationId: string) => void;
  onOpenAutomationRun?: (target: AutomationRunOpenTarget) => void;
}

export function AutomationRunsTab({
  automation,
  runs,
  latest,
  activeGoalItem,
  projectId,
  judgeRecovery,
  idleAfterCancelledRun,
  actionPending,
  onRetryPlanJudge,
  onRetryJudge,
  onRunNow,
  onOpenRunConversation,
  onOpenAutomationRun,
}: AutomationRunsTabProps) {
  const newestRuns = sortedNewestRuns(runs);
  return (
    <div>
      {judgeRecovery ? (
        <div
          className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-md px-3 py-2 text-sm"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
          data-testid={`automation-${judgeRecovery.kind}-judge-recovery`}
        >
          <span className="min-w-0">
            <strong style={{ color: "var(--text-primary)" }}>
              {judgeRecovery.statusLabel}.
            </strong>{" "}
            {judgeRecovery.description}
          </span>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={actionPending}
            onClick={() =>
              judgeRecovery.kind === "plan" ? onRetryPlanJudge() : onRetryJudge()
            }
          >
            {judgeRecovery.actionLabel}
          </Button>
        </div>
      ) : null}
      {idleAfterCancelledRun ? (
        <div
          className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-md px-3 py-2 text-sm"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
          data-testid="automation-idle-after-cancelled"
        >
          <span className="min-w-0">
            {CANCELLED_RUN_RESTART_DESCRIPTION}
          </span>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={actionPending}
            onClick={onRunNow}
          >
            Run now
          </Button>
        </div>
      ) : null}
      <Section title="Runs timeline" testId="automation-runs-timeline">
        {newestRuns.length === 0 ? (
          <p className="text-sm" style={{ color: "var(--text-muted)" }}>
            No runs have been created yet.
          </p>
        ) : (
          <div className="relative space-y-4 before:absolute before:bottom-0 before:left-[5px] before:top-2 before:w-px before:bg-[var(--border-default)]">
            {newestRuns.map((run) => (
              <RunTimelineItem
                key={run.id}
                run={run}
                automation={automation}
                projectId={projectId}
                defaultExpanded={
                  run.runIndex === latest?.runIndex || isOpenAutomationRun(run)
                }
                activeGoalItem={activeGoalItem}
                {...(onOpenRunConversation ? { onOpenRunConversation } : {})}
                {...(onOpenAutomationRun ? { onOpenAutomationRun } : {})}
                setupConversationId={automation.setupConversationId}
              />
            ))}
          </div>
        )}
      </Section>
    </div>
  );
}
