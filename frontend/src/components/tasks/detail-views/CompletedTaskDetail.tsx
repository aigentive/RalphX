/**
 * CompletedTaskDetail - macOS Tahoe-inspired completed task view
 *
 * Shows final summary, review history, and actions for completed tasks.
 */

import { useState, useCallback } from "react";
import { Button } from "@/components/ui/button";
import {
  SectionTitle,
  StatusBanner,
  StatusPill,
  TwoColumnLayout,
} from "./shared";
import { ReviewTimeline } from "./shared/ReviewTimeline";
import { useTaskStateHistory } from "@/hooks/useReviews";
import {
  CheckCircle2,
  Loader2,
  ExternalLink,
  RefreshCw,
  Code,
} from "lucide-react";
import type { Task } from "@/types/task";
import { ReviewDetailModal } from "@/components/reviews/ReviewDetailModal";
import { DurationDisplay } from "./shared/DurationDisplay";
import { api } from "@/lib/tauri";
import { useQueryClient } from "@tanstack/react-query";
import { taskKeys } from "@/hooks/useTasks";
import { executionKeys } from "@/hooks/useExecutionControl";
import { useTaskStateTransitions } from "@/hooks/useTaskStateTransitions";
import {
  TaskRerunDialog,
  type TaskRerunResult,
} from "@/components/tasks/TaskRerunDialog";
import { useGitDiff } from "@/hooks/useGitDiff";
import { resumeExecutionIfStopped } from "@/lib/task-actions/resume-execution-if-stopped";

interface CompletedTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
}

/**
 * ActionButtonsCard - View Diff and Reopen actions
 */
function ActionButtonsCard({
  onViewDiff,
  onReopenTask,
  onReviewCode,
}: {
  onViewDiff?: () => void;
  onReopenTask?: () => void;
  onReviewCode?: () => void;
}) {
  return (
    <div className="flex gap-2 justify-end">
      {onReviewCode && (
        <Button
          data-testid="review-code-button"
          onClick={onReviewCode}
          variant="ghost"
          className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
          style={{ color: "var(--status-info)" }}
        >
          <Code className="w-4 h-4" />
          Review Code
        </Button>
      )}
      <Button
        data-testid="view-diff-button"
        onClick={onViewDiff}
        variant="ghost"
        className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
        style={{
          color: "var(--text-secondary)",
          backgroundColor: "var(--bg-elevated)",
        }}
      >
        <ExternalLink className="w-4 h-4" />
        View Final Diff
      </Button>
      <Button
        data-testid="reopen-task-button"
        onClick={onReopenTask}
        variant="ghost"
        className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
        style={{
          color: "var(--text-secondary)",
          backgroundColor: "var(--bg-elevated)",
        }}
      >
        <RefreshCw className="w-4 h-4" />
        Reopen Task
      </Button>
    </div>
  );
}

export function CompletedTaskDetail({ task, isHistorical = false }: CompletedTaskDetailProps) {
  const queryClient = useQueryClient();
  const { data: history, isLoading } = useTaskStateHistory(task.id);
  const { data: stateTransitions = [] } = useTaskStateTransitions(task.id);

  const [showReviewModal, setShowReviewModal] = useState(false);
  const [isRerunDialogOpen, setIsRerunDialogOpen] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { commits } = useGitDiff({ taskId: task.id });
  const latestCommit = commits[0];
  const commitInfo = {
    sha: latestCommit?.shortSha ?? "unknown",
    message: latestCommit?.message ?? "No commit info available",
    hasDependentCommits: commits.length > 1,
  };

  const handleViewDiff = () => {
    // Diff viewer not yet implemented
  };

  const handleReopenTask = () => {
    setError(null);
    setIsRerunDialogOpen(true);
  };

  const handleDialogClose = useCallback(() => {
    if (!isProcessing) {
      setIsRerunDialogOpen(false);
      setError(null);
    }
  }, [isProcessing]);

  const handleRerunConfirm = useCallback(
    async (result: TaskRerunResult) => {
      setIsProcessing(true);
      setError(null);

      try {
        switch (result.option) {
          case "keep_changes":
          case "revert_commit":
          case "create_new":
            await api.tasks.move(task.id, "ready", result.note);
            await resumeExecutionIfStopped(task.projectId);
            break;
        }

        setIsRerunDialogOpen(false);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to reopen task");
      } finally {
        await queryClient.invalidateQueries({
          queryKey: taskKeys.list(task.projectId),
        });
        await queryClient.invalidateQueries({ queryKey: executionKeys.all });
        setIsProcessing(false);
      }
    },
    [task.id, task.projectId, queryClient]
  );

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
        testId="completed-task-detail"
      >
        {/* Status Banner */}
        <StatusBanner
          icon={CheckCircle2}
          title="Task Completed"
          subtitle="All work has been reviewed and approved"
          variant="success"
          badge={
            <StatusPill
              icon={CheckCircle2}
              label="Done"
              variant="success"
              size="md"
            />
          }
        />

        {/* Duration (static) */}
        {task.startedAt && task.completedAt && (
          <div data-testid="completed-task-duration">
            <DurationDisplay
              mode="static"
              startedAt={task.startedAt}
              completedAt={task.completedAt}
            />
          </div>
        )}

        {/* Review History */}
        <section data-testid="review-history-section">
          <SectionTitle>Review History</SectionTitle>
          <ReviewTimeline history={history} stateTransitions={stateTransitions} />
        </section>

        {/* Actions (hidden in historical mode) */}
        {!isHistorical && (
          <section data-testid="action-buttons">
            <ActionButtonsCard
              onViewDiff={handleViewDiff}
              onReopenTask={handleReopenTask}
              onReviewCode={() => setShowReviewModal(true)}
            />
          </section>
        )}
      </TwoColumnLayout>

      {/* Task Rerun Dialog */}
      <TaskRerunDialog
        isOpen={isRerunDialogOpen}
        onClose={handleDialogClose}
        onConfirm={handleRerunConfirm}
        task={task}
        commitInfo={commitInfo}
        isProcessing={isProcessing}
        error={error}
      />

      {/* Review Detail Modal */}
      {showReviewModal && (
        <ReviewDetailModal
          taskId={task.id}
          history={history}
          showActions={false}
          onClose={() => setShowReviewModal(false)}
        />
      )}
    </>
  );
}
