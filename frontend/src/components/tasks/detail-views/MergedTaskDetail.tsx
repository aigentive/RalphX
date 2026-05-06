/**
 * MergedTaskDetail - View for successfully merged tasks
 *
 * Shows completion info, merge commit SHA, and read-only historical chat.
 */

import {
  CheckCircle2,
  GitMerge,
  GitCommit,
  GitPullRequest,
  ExternalLink,
  Loader2,
} from "lucide-react";
import {
  SectionTitle,
  DetailCard,
  StatusBanner,
  StatusPill,
  TwoColumnLayout,
  TaskMetricsCard,
  ChangeReviewSection,
  PlanMergeContextSection,
} from "./shared";
import { useTaskDetailContextModel } from "./shared/TaskDetailContext";
import { ValidationProgress } from "./shared/ValidationProgress";
import { useTaskStateHistory } from "@/hooks/useReviews";
import { useTaskStateTransitions } from "@/hooks/useTaskStateTransitions";
import type { Task } from "@/types/task";
import { BranchBadge } from "@/components/shared/BranchBadge";
import { DurationDisplay } from "./shared/DurationDisplay";
import { usePlanBranchForTask } from "@/hooks/usePlanBranchForTask";

interface MergedTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
}

function formatRelativeTime(date: Date | string | null | undefined): string {
  if (!date) return "Unknown";

  const now = new Date();
  const then = typeof date === "string" ? new Date(date) : date;
  const diffMs = now.getTime() - then.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMins / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffMins < 1) return "Just now";
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  return `${diffDays}d ago`;
}

/**
 * MergeInfoCard - Shows merge commit and branch details
 */
function MergeInfoCard({
  mergeCommitSha,
  branchName,
  mergedAt,
}: {
  mergeCommitSha: string | null | undefined;
  branchName: string | null | undefined;
  mergedAt: Date | string | null | undefined;
}) {
  const shortSha = mergeCommitSha?.substring(0, 7);

  return (
    <DetailCard variant="success">
      <div className="space-y-4">
        {/* Merge commit */}
        {shortSha && (
          <div className="flex items-center gap-3">
            <div
              className="flex items-center justify-center w-8 h-8 rounded-xl shrink-0"
              style={{ backgroundColor: "var(--status-success-muted)" }}
            >
              <GitCommit className="w-4 h-4" style={{ color: "var(--status-success)" }} />
            </div>
            <div className="flex-1 min-w-0">
              <span className="text-[0.6875rem] uppercase tracking-wider text-text-primary/40 block">
                Merge Commit
              </span>
              <span className="text-[0.8125rem] text-text-primary/70 font-mono">
                {shortSha}
              </span>
            </div>
            <span className="text-[0.75rem] text-text-primary/40">
              {formatRelativeTime(mergedAt)}
            </span>
          </div>
        )}

        {/* Branch info */}
        {branchName && (
          <>
            {shortSha && (
              <div
                className="h-px"
                style={{ backgroundColor: "var(--overlay-weak)" }}
              />
            )}
            <div className="flex items-center gap-3">
              <div
                className="flex items-center justify-center w-8 h-8 rounded-xl shrink-0"
                style={{ backgroundColor: "var(--status-success-muted)" }}
              >
                <GitMerge className="w-4 h-4" style={{ color: "var(--status-success)" }} />
              </div>
              <div className="flex-1 min-w-0">
                <span className="text-[0.6875rem] uppercase tracking-wider text-text-primary/40 block">
                  Branch
                </span>
                <BranchBadge branch={branchName} variant="muted" />
              </div>
              <span className="text-[0.625rem] px-2 py-0.5 rounded bg-[var(--overlay-faint)] text-text-primary/40">
                Deleted
              </span>
            </div>
          </>
        )}
      </div>
    </DetailCard>
  );
}

export function MergedTaskDetail({
  task,
  isHistorical: _isHistorical = false,
}: MergedTaskDetailProps) {
  const { data: history, isLoading } = useTaskStateHistory(task.id);
  const { data: stateTransitions = [] } = useTaskStateTransitions(task.id);
  const { data: planBranch } = usePlanBranchForTask(task.id);
  const detailContext = useTaskDetailContextModel();

  const isPlanMerge = task.category === "plan_merge";
  const effectiveMergeCommitSha = task.mergeCommitSha ?? planBranch?.mergeCommitSha ?? null;
  const hasPrContext = isPlanMerge && planBranch?.prNumber != null;
  const hasMergeInfo = Boolean(effectiveMergeCommitSha || task.taskBranch);
  // Use completedAt as mergedAt (merge happens after approval which sets completedAt)
  const mergedAt = planBranch?.mergedAt ?? task.completedAt ?? task.updatedAt;

  const mergeTarget =
    planBranch?.baseBranchOverride ?? (isPlanMerge ? planBranch?.sourceBranch : null);
  const mergedIntoSubtitle = mergeTarget
    ? `Merged into ${mergeTarget}`
    : "Changes have been merged into the base branch";

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-16">
        <Loader2
          className="w-6 h-6 animate-spin text-text-primary/30"
        />
      </div>
    );
  }

  return (
    <>
      <TwoColumnLayout
        description={task.description}
        testId="merged-task-detail"
      >
      {/* Status Banner */}
      <StatusBanner
        icon={CheckCircle2}
        title="Task Merged"
        subtitle={mergedIntoSubtitle}
        variant="success"
        badge={
          <StatusPill
            icon={CheckCircle2}
            label="Merged"
            variant="success"
            size="md"
          />
        }
      />

      {/* Duration (static) */}
      {task.startedAt && task.completedAt && (
        <div data-testid="merged-task-duration">
          <DurationDisplay
            mode="static"
            startedAt={task.startedAt}
            completedAt={task.completedAt}
          />
        </div>
      )}

      {isPlanMerge && !detailContext && <PlanMergeContextSection taskId={task.id} />}

      {/* Merged via PR */}
      {hasPrContext && !detailContext && (
        <section data-testid="merged-via-pr-section">
          <SectionTitle>Pull Request</SectionTitle>
          <DetailCard variant="success">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <GitPullRequest className="w-4 h-4" style={{ color: "var(--status-success)" }} />
                <span className="text-[0.8125rem] text-text-primary/80">
                  Merged via PR #{planBranch.prNumber}
                </span>
              </div>
              {planBranch.prUrl && (
                <button
                  type="button"
                  onClick={async () => {
                    const { openUrl } = await import("@tauri-apps/plugin-opener");
                    await openUrl(planBranch.prUrl!);
                  }}
                  className="flex items-center gap-1 text-[0.75rem] cursor-pointer"
                  style={{ color: "var(--status-success)" }}
                >
                  <ExternalLink className="w-3.5 h-3.5" />
                  View PR
                </button>
              )}
            </div>
          </DetailCard>
        </section>
      )}

      {/* Task Metrics */}
      {!isPlanMerge && (
        <section data-testid="task-metrics-section">
          <SectionTitle>Metrics</SectionTitle>
          <TaskMetricsCard taskId={task.id} />
        </section>
      )}

      {/* Merge Info */}
      {hasMergeInfo && !detailContext && (
        <section data-testid="merge-info-section">
          <SectionTitle>Merge Details</SectionTitle>
          <MergeInfoCard
            mergeCommitSha={effectiveMergeCommitSha}
            branchName={task.taskBranch}
            mergedAt={mergedAt}
          />
        </section>
      )}

      {/* Merge Validation History */}
      <ValidationProgress
        taskId={task.id}
        metadata={task.metadata}
      />

      <ChangeReviewSection
        taskId={task.id}
        history={history}
        stateTransitions={stateTransitions}
        context={isPlanMerge ? "plan_merge" : "task"}
      />
      </TwoColumnLayout>
    </>
  );
}
