import { useCallback, useState } from "react";
import { AlertTriangle, Loader2, RefreshCw } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";

import { tasksApi } from "@/api/tasks";
import { Button } from "@/components/ui/button";
import { taskKeys } from "@/hooks/useTasks";
import { extractErrorMessage } from "@/lib/errors";
import type { Task, InternalStatus } from "@/types/task";

import { DetailCard, SectionTitle, StatusBanner } from "./shared";

interface BranchUpdateTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
  viewStatus?: InternalStatus | undefined;
}

function metadata(task: Task): Record<string, unknown> {
  try {
    return task.metadata ? (JSON.parse(task.metadata) as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

export function BranchUpdateTaskDetail({
  task,
  isHistorical = false,
  viewStatus,
}: BranchUpdateTaskDetailProps) {
  const status = viewStatus ?? task.internalStatus;
  const blocked = status === "branch_update_blocked";
  const details = metadata(task);
  const branchUpdate =
    typeof details.branch_update === "object" && details.branch_update !== null
      ? (details.branch_update as Record<string, unknown>)
      : {};
  const direction =
    status === "updating_task_branch" || branchUpdate.direction === "task_branch"
      ? "task branch"
      : "plan branch";
  const source =
    typeof branchUpdate.source_branch === "string" ? branchUpdate.source_branch : null;
  const target =
    typeof branchUpdate.target_branch === "string" ? branchUpdate.target_branch : null;
  const diagnostic =
    typeof branchUpdate.diagnostics === "string"
      ? branchUpdate.diagnostics
      : typeof details.error === "string"
        ? details.error
        : null;
  const failureKind =
    typeof branchUpdate.failure_kind === "string" ? branchUpdate.failure_kind : null;
  const conflictFiles = Array.isArray(branchUpdate.conflict_files)
    ? branchUpdate.conflict_files.filter(
        (path): path is string => typeof path === "string",
      )
    : [];
  const queryClient = useQueryClient();
  const [isRetrying, setIsRetrying] = useState(false);
  const [retryError, setRetryError] = useState<string | null>(null);
  const handleRetry = useCallback(async () => {
    setIsRetrying(true);
    setRetryError(null);
    try {
      await tasksApi.retryBranchUpdate(task.id);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: taskKeys.list(task.projectId) }),
        queryClient.invalidateQueries({ queryKey: taskKeys.detail(task.id) }),
      ]);
    } catch (error) {
      setRetryError(extractErrorMessage(error, "Failed to retry branch update"));
    } finally {
      setIsRetrying(false);
    }
  }, [queryClient, task.id, task.projectId]);

  return (
    <div className="space-y-4" data-testid="branch-update-task-detail">
      <StatusBanner
        icon={blocked ? AlertTriangle : Loader2}
        title={blocked ? "Branch update needs attention" : `Updating ${direction}`}
        subtitle={
          blocked
            ? "The branch freshness checkpoint stopped safely before the task could continue."
            : "RalphX is synchronizing branches. The task will return to its previous pipeline stage when this checkpoint completes."
        }
        variant={blocked ? "warning" : "info"}
        animated={!blocked}
      />

      <section>
        <SectionTitle>Branch checkpoint</SectionTitle>
        <DetailCard>
          <dl className="grid gap-3 text-[0.8125rem]">
            <div>
              <dt className="text-text-primary/40">Direction</dt>
              <dd className="font-medium text-text-primary/80">{direction}</dd>
            </div>
            {source && target ? (
              <div>
                <dt className="text-text-primary/40">Update</dt>
                <dd className="font-mono text-[0.75rem] text-text-primary/70">
                  {source} → {target}
                </dd>
              </div>
            ) : null}
            {failureKind ? (
              <div>
                <dt className="text-text-primary/40">Failure</dt>
                <dd className="font-medium text-text-primary/70">
                  {failureKind.replace(/_/g, " ")}
                </dd>
              </div>
            ) : null}
            {diagnostic ? (
              <div>
                <dt className="text-text-primary/40">Diagnostic</dt>
                <dd className="text-text-primary/70">{diagnostic}</dd>
              </div>
            ) : null}
            {conflictFiles.length > 0 ? (
              <div>
                <dt className="text-text-primary/40">Conflicts</dt>
                <dd>
                  <ul className="space-y-1 font-mono text-[0.75rem] text-text-primary/70">
                    {conflictFiles.map((path) => (
                      <li key={path}>{path}</li>
                    ))}
                  </ul>
                </dd>
              </div>
            ) : null}
          </dl>
        </DetailCard>
      </section>

      {isHistorical ? (
        <p className="text-[0.75rem] text-text-primary/40">
          Historical checkpoint — controls are read-only.
        </p>
      ) : blocked ? (
        <div className="space-y-2">
          {retryError ? (
            <p role="alert" className="text-[0.75rem] text-status-error">
              {retryError}
            </p>
          ) : null}
          <Button onClick={handleRetry} disabled={isRetrying} size="sm">
            {isRetrying ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="mr-2 h-4 w-4" />
            )}
            Retry branch update
          </Button>
        </div>
      ) : null}
    </div>
  );
}
