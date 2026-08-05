import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Code, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ReviewDetailModal } from "@/components/reviews/ReviewDetailModal";
import { useGitDiff } from "@/hooks/useGitDiff";
import { usePlanBranchForTask } from "@/hooks/usePlanBranchForTask";
import { api } from "@/lib/tauri";
import type { StateTransition } from "@/api/tasks";
import type { ReviewNoteResponse } from "@/lib/tauri";
import { ReviewTimeline } from "./ReviewTimeline";
import { SectionTitle } from "./SectionTitle";

interface CommitSummaryCardProps {
  taskId: string;
}

interface ChangeReviewSectionProps {
  taskId: string;
  history: ReviewNoteResponse[] | undefined;
  stateTransitions: StateTransition[];
  context?: "task" | "plan_merge";
}

interface PlanReviewTimeline {
  history: ReviewNoteResponse[];
  taskTitlesById: Record<string, string>;
}

export function CommitSummaryCard({ taskId }: CommitSummaryCardProps) {
  const { commits, historyError, isLoadingHistory } = useGitDiff({ taskId });

  if (isLoadingHistory) {
    return (
      <div className="flex items-center justify-center py-4">
        <Loader2 className="w-5 h-5 animate-spin text-text-primary/30" />
      </div>
    );
  }

  if (historyError) {
    return (
      <p className="text-[0.8125rem] text-text-primary/50 italic">
        Commit history could not be loaded. It may be available only on the host.
      </p>
    );
  }

  if (commits.length === 0) {
    return (
      <p className="text-[0.8125rem] text-text-primary/50 italic">
        No commit history available
      </p>
    );
  }

  return (
    <div>
      {commits.map((commit, index) => (
        <div
          key={commit.sha}
          className="flex items-start gap-3 py-3 first:pt-0 last:pb-0"
          style={
            index < commits.length - 1
              ? { borderBottom: "1px solid var(--border-subtle)" }
              : undefined
          }
        >
          <span className="text-[0.6875rem] font-mono text-text-primary/50 shrink-0 pt-0.5">
            {commit.shortSha}
          </span>
          <span className="text-[0.8125rem] text-text-primary line-clamp-2">
            {commit.message}
          </span>
        </div>
      ))}
    </div>
  );
}

export function ChangeReviewSection({
  taskId,
  history,
  stateTransitions,
  context = "task",
}: ChangeReviewSectionProps) {
  const [showReviewModal, setShowReviewModal] = useState(false);
  const isPlanMerge = context === "plan_merge";
  const { data: planBranch, isLoading: isLoadingPlanBranch } = usePlanBranchForTask(taskId, {
    enabled: isPlanMerge,
  });
  const {
    data: planTimeline,
    isLoading: isLoadingPlanTimeline,
    isError: isPlanTimelineError,
  } = useQuery<PlanReviewTimeline>({
    queryKey: [
      "plan-review-timeline",
      planBranch?.projectId,
      planBranch?.sessionId,
      taskId,
    ] as const,
    enabled: isPlanMerge && Boolean(planBranch?.projectId && planBranch?.sessionId),
    staleTime: 30_000,
    queryFn: async () => {
      const taskPage = await api.tasks.list({
        projectId: planBranch!.projectId,
        ideationSessionId: planBranch!.sessionId,
        includeArchived: true,
        limit: 500,
      });

      const taskTitlesById = Object.fromEntries(
        taskPage.tasks.map((task) => [task.id, task.title])
      );
      const histories = await Promise.all(
        taskPage.tasks.map((task) => api.reviews.getTaskStateHistory(task.id))
      );
      const planHistory = histories
        .flat()
        .sort(
          (a, b) =>
            new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
        );

      return { history: planHistory, taskTitlesById };
    },
  });
  const isLoadingPlanReviews = isPlanMerge && (isLoadingPlanBranch || isLoadingPlanTimeline);
  const effectiveHistory = isPlanMerge
    ? planTimeline?.history ?? history ?? []
    : history ?? [];
  const effectiveStateTransitions = isPlanMerge ? [] : stateTransitions;
  const hasReviewHistory = effectiveHistory.length > 0;
  const getEntryContext = useMemo(
    () =>
      isPlanMerge
        ? (entry: ReviewNoteResponse) => planTimeline?.taskTitlesById[entry.task_id] ?? null
        : undefined,
    [isPlanMerge, planTimeline?.taskTitlesById]
  );

  return (
    <>
      <section data-testid="commits-section">
        <SectionTitle muted>Commits</SectionTitle>
        <CommitSummaryCard taskId={taskId} />
      </section>

      <section data-testid="review-history-section">
        <div className="flex items-center justify-between mb-3">
          <SectionTitle muted>{isPlanMerge ? "Code Review" : "Review History"}</SectionTitle>
          <Button
            data-testid="review-code-button"
            onClick={() => setShowReviewModal(true)}
            variant="ghost"
            className="h-8 px-3 gap-2 rounded-lg font-medium text-[0.75rem]"
            style={{ color: "var(--status-info)" }}
          >
            <Code className="w-4 h-4" />
            {isPlanMerge ? "Review Diff" : "Review Code"}
          </Button>
        </div>
        {isLoadingPlanReviews ? (
          <div className="flex items-center justify-center py-4">
            <Loader2 className="w-5 h-5 animate-spin text-text-primary/30" />
          </div>
        ) : isPlanMerge && isPlanTimelineError ? (
          <p className="text-[0.8125rem] text-text-primary/50 italic">
            Review history could not be loaded.
          </p>
        ) : isPlanMerge && !hasReviewHistory ? (
          <p className="text-[0.8125rem] text-text-primary/50 italic">
            No internal plan review records available
          </p>
        ) : (
          <ReviewTimeline
            history={effectiveHistory}
            stateTransitions={effectiveStateTransitions}
            emptyMessage={
              isPlanMerge
                ? "No internal plan review records available"
                : "No review history available"
            }
            {...(getEntryContext !== undefined && { getEntryContext })}
          />
        )}
      </section>

      {showReviewModal && (
        <ReviewDetailModal
          taskId={taskId}
          reviewMode={context}
          showActions={false}
          onClose={() => setShowReviewModal(false)}
          {...(history !== undefined && { history })}
        />
      )}
    </>
  );
}
