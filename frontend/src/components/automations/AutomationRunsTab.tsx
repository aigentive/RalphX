import { Info, XCircle } from "lucide-react";

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
import { NoticeBanner } from "@/components/ui/notice-banner";
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
  onDeleteRun?: (run: AutomationRun) => void;
  onResumeRun?: (run: AutomationRun) => void;
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
  onDeleteRun,
  onResumeRun,
  onOpenRunConversation,
  onOpenAutomationRun,
}: AutomationRunsTabProps) {
  const newestRuns = sortedNewestRuns(runs);
  return (
    <div>
      {judgeRecovery ? (
        <NoticeBanner
          tone="error"
          icon={<XCircle className="h-4 w-4" aria-hidden="true" />}
          title={`${judgeRecovery.statusLabel}.`}
          action={(
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
          )}
          className="mb-4"
          testId={`automation-${judgeRecovery.kind}-judge-recovery`}
        >
          {judgeRecovery.description}
        </NoticeBanner>
      ) : null}
      {idleAfterCancelledRun ? (
        <NoticeBanner
          tone="neutral"
          icon={<Info className="h-4 w-4" aria-hidden="true" />}
          action={(
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={actionPending}
              onClick={onRunNow}
            >
              Run now
            </Button>
          )}
          className="mb-4"
          testId="automation-idle-after-cancelled"
        >
          {CANCELLED_RUN_RESTART_DESCRIPTION}
        </NoticeBanner>
      ) : null}
      <Section title="Runs timeline" testId="automation-runs-timeline">
        {newestRuns.length === 0 ? (
          <p className="text-sm" style={{ color: "var(--text-muted)" }}>
            No runs have been created yet.
          </p>
        ) : (
          <div className="relative space-y-4 before:absolute before:bottom-0 before:left-[5px] before:top-2 before:w-px before:bg-[var(--border-subtle)]">
            {newestRuns.map((run) => {
              const isLatest = run.runIndex === latest?.runIndex;
              return (
                <RunTimelineItem
                  key={run.id}
                  run={run}
                  automation={automation}
                  projectId={projectId}
                  defaultExpanded={isLatest || isOpenAutomationRun(run)}
                  activeGoalItem={activeGoalItem}
                  isLatest={isLatest}
                  {...(onDeleteRun ? { onDeleteRun } : {})}
                  {...(onResumeRun ? { onResumeRun } : {})}
                  {...(onOpenRunConversation ? { onOpenRunConversation } : {})}
                  {...(onOpenAutomationRun ? { onOpenAutomationRun } : {})}
                  setupConversationId={automation.setupConversationId}
                />
              );
            })}
          </div>
        )}
      </Section>
    </div>
  );
}
