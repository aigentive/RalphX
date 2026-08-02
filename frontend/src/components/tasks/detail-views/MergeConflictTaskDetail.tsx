/**
 * MergeConflictTaskDetail - View for tasks in merge_conflict state
 *
 * Shows conflict files, read-only chat history from merger agent,
 * and action buttons for manual resolution.
 */

import { useState, useCallback, useMemo } from "react";
import {
  AlertTriangle,
  FileWarning,
  CheckCircle2,
  GitMerge,
  Loader2,
  Ban,
  ChevronDown,
  ChevronRight,
  RefreshCw,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { extractErrorMessage } from "@/lib/errors";
import { Button } from "@/components/ui/button";
import {
  SectionTitle,
  DetailCard,
  StatusBanner,
  StatusPill,
  TwoColumnLayout,
} from "./shared";
import type { Task } from "@/types/task";
import { BranchBadge } from "@/components/shared/BranchBadge";
import { useQueryClient } from "@tanstack/react-query";
import { taskKeys } from "@/hooks/useTasks";
import { useConfirmation } from "@/hooks/useConfirmation";
import { api } from "@/lib/tauri";
import { useConflictDetection } from "@/hooks/useConflictDetection";
import { useConflictDiff } from "@/hooks/useConflictDiff";
import { ConflictDiffViewer } from "@/components/diff/ConflictDiffViewer";
import { statusTint } from "@/lib/theme-colors";
import { useAgentGate } from "@/hooks/useAgentGate";
import { AgentGateTooltip } from "@/components/remote/AgentGateTooltip";

interface MergeConflictTaskDetailProps {
  task: Task;
  isHistorical?: boolean;
}

/**
 * ConflictFilesList - Shows files with merge conflicts, expandable to show diff
 */
function ConflictFilesList({
  files,
  taskId,
}: {
  files: string[];
  taskId: string;
}) {
  const [expandedFile, setExpandedFile] = useState<string | null>(null);
  const { data: conflictDiff, isLoading: isLoadingDiff } = useConflictDiff({
    taskId,
    filePath: expandedFile,
  });

  if (files.length === 0) {
    return (
      <p className="text-[0.8125rem] text-text-primary/50 italic">
        No conflict files recorded
      </p>
    );
  }

  const toggleFile = (file: string) => {
    setExpandedFile(expandedFile === file ? null : file);
  };

  return (
    <div className="space-y-2">
      {files.map((file, index) => (
        <div key={index}>
          <button
            type="button"
            onClick={() => toggleFile(file)}
            className="w-full flex items-center gap-2 py-2 px-3 rounded-lg transition-colors cursor-pointer"
            style={{
              backgroundColor:
                expandedFile === file
                  ? "var(--status-warning-muted)"
                  : "var(--status-warning-muted)",
            }}
          >
            {expandedFile === file ? (
              <ChevronDown
                className="w-4 h-4 shrink-0"
                style={{ color: "var(--status-warning)" }}
              />
            ) : (
              <ChevronRight
                className="w-4 h-4 shrink-0"
                style={{ color: "var(--status-warning)" }}
              />
            )}
            <FileWarning className="w-4 h-4 shrink-0" style={{ color: "var(--status-warning)" }} />
            <span
              className="text-[0.8125rem] font-mono text-text-primary/70 truncate text-left"
              title={file}
            >
              {file}
            </span>
          </button>
          {expandedFile === file && (
            <div
              className="mt-2 rounded-lg overflow-hidden border"
              style={{
                borderColor: "var(--status-warning-border)",
                height: "400px",
              }}
            >
              {isLoadingDiff ? (
                <div
                  className="flex items-center justify-center h-full"
                  style={{ backgroundColor: "var(--bg-base)" }}
                >
                  <Loader2 className="w-5 h-5 animate-spin text-text-primary/50" />
                </div>
              ) : conflictDiff ? (
                <ConflictDiffViewer conflictDiff={conflictDiff} />
              ) : (
                <div
                  className="flex items-center justify-center h-full text-text-primary/50"
                  style={{ backgroundColor: "var(--bg-base)" }}
                >
                  Failed to load conflict diff
                </div>
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

/**
 * ResolutionInstructions - Guide for resolving conflicts manually
 */
function ResolutionInstructions({ branchName }: { branchName: string }) {
  return (
    <div className="space-y-3">
      <p className="text-[0.8125rem] text-text-primary/60">
        The AI agent could not automatically resolve the merge conflicts.
        Please resolve them manually:
      </p>
      <ol className="list-decimal list-inside space-y-2 text-[0.8125rem] text-text-primary/50">
        <li>Open the conflicting files in your editor</li>
        <li>Resolve the conflicts (remove conflict markers)</li>
        <li>Stage the resolved files: <code className="text-text-primary/70 bg-[var(--overlay-faint)] px-1 rounded">git add .</code></li>
        <li>Commit the merge: <code className="text-text-primary/70 bg-[var(--overlay-faint)] px-1 rounded">git commit</code></li>
        <li>Click "Conflicts Resolved" below to continue</li>
      </ol>
      <div className="pt-2">
        <BranchBadge branch={branchName} variant="muted" size="sm" />
      </div>
    </div>
  );
}

/**
 * ActionButtonsCard - Actions for conflict resolution
 */
function ActionButtonsCard({
  onResolve,
  onRetry,
  onCancel,
  isProcessing,
}: {
  onResolve: () => void;
  onRetry: () => void;
  onCancel: () => void;
  isProcessing: boolean;
}) {
  const retryGate = useAgentGate("mergeRetry");
  const resolveGate = useAgentGate("mergeResolveConflict");
  const moveGate = useAgentGate("taskMove");
  return (
    <div className="flex gap-2 justify-end flex-wrap">
      <AgentGateTooltip gated={retryGate.gated} reason={retryGate.reason}><Button
        data-testid="retry-merge-button"
        onClick={onRetry}
        disabled={isProcessing || retryGate.gated}
        variant="ghost"
        className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
        style={{
          color: "var(--text-secondary)",
          backgroundColor: "var(--bg-elevated)",
        }}
      >
        <GitMerge className="w-4 h-4" />
        Retry Merge
      </Button></AgentGateTooltip>
      <AgentGateTooltip gated={resolveGate.gated} reason={resolveGate.reason}><Button
        data-testid="resolve-conflict-button"
        onClick={onResolve}
        disabled={isProcessing || resolveGate.gated}
        className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
        style={{
          color: "white",
          backgroundColor: "var(--status-success)",
        }}
      >
        {isProcessing ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <CheckCircle2 className="w-4 h-4" />
        )}
        Conflicts Resolved
      </Button></AgentGateTooltip>
      <AgentGateTooltip gated={moveGate.gated} reason={moveGate.reason}><Button
        data-testid="cancel-task-button"
        onClick={onCancel}
        disabled={isProcessing || moveGate.gated}
        className="h-9 px-4 gap-2 rounded-lg font-medium text-[0.8125rem]"
        style={{
          color: "white",
          backgroundColor: "#ff4545",
        }}
      >
        {isProcessing ? (
          <Loader2 className="w-4 h-4 animate-spin" />
        ) : (
          <Ban className="w-4 h-4" />
        )}
        Cancel
      </Button></AgentGateTooltip>
    </div>
  );
}

export function MergeConflictTaskDetail({ task, isHistorical = false }: MergeConflictTaskDetailProps) {
  const queryClient = useQueryClient();
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const retryGate = useAgentGate("mergeRetry");
  const resolveGate = useAgentGate("mergeResolveConflict");
  const { confirm } = useConfirmation();

  // Parse conflict files from task metadata (for historical view or fallback)
  const metadataConflicts: string[] = useMemo(() => {
    if (!task.metadata) return [];
    try {
      const metadata = typeof task.metadata === "string"
        ? JSON.parse(task.metadata)
        : task.metadata;
      return Array.isArray(metadata?.conflict_files) ? metadata.conflict_files : [];
    } catch {
      return [];
    }
  }, [task.metadata]);

  // Parse conflict type from task metadata to distinguish freshness updates from real conflicts
  const conflictType = useMemo(() => {
    if (!task.metadata) return null;
    try {
      const parsed = typeof task.metadata === "string" ? JSON.parse(task.metadata) : task.metadata;
      if (parsed?.plan_update_conflict === true) return "plan_update" as const;
      if (parsed?.source_update_conflict === true) return "source_update" as const;
      return null;
    } catch {
      return null;
    }
  }, [task.metadata]);

  // Live conflict detection (only active for non-historical views)
  const {
    conflicts: liveConflicts,
    isLoading: isLoadingConflicts,
    isEnabled: isConflictDetectionEnabled,
  } = useConflictDetection({
    taskId: task.id,
    internalStatus: task.internalStatus,
    isHistorical,
    hasBranch: !!task.taskBranch,
  });

  // Hybrid data source: use live conflicts for active states, metadata for historical
  const conflictFiles: string[] = isHistorical
    ? metadataConflicts
    : (isConflictDetectionEnabled && liveConflicts.length > 0 ? liveConflicts : metadataConflicts);

  const branchName = task.taskBranch ?? "task branch";

  const handleResolveConflicts = useCallback(async () => {
    if (resolveGate.gated) return;
    setIsProcessing(true);
    setError(null);
    try {
      await invoke("resolve_merge_conflict", { taskId: task.id });
      await queryClient.invalidateQueries({
        queryKey: taskKeys.list(task.projectId),
      });
    } catch (err) {
      setError(extractErrorMessage(err, "Failed to mark conflicts as resolved"));
    } finally {
      setIsProcessing(false);
    }
  }, [task.id, task.projectId, queryClient, resolveGate.gated]);

  const handleRetryMerge = useCallback(async () => {
    if (retryGate.gated) return;
    setIsProcessing(true);
    setError(null);
    try {
      await invoke("retry_merge", { taskId: task.id });
      await queryClient.invalidateQueries({
        queryKey: taskKeys.list(task.projectId),
      });
    } catch (err) {
      setError(extractErrorMessage(err, "Failed to retry merge"));
    } finally {
      setIsProcessing(false);
    }
  }, [task.id, task.projectId, queryClient, retryGate.gated]);

  const handleCancel = useCallback(async () => {
    const confirmed = await confirm({
      title: "Cancel task?",
      description: "This will transition the task to Cancelled status. This action cannot be undone.",
      confirmText: "Cancel",
      variant: "destructive",
    });

    if (!confirmed) return;

    setIsProcessing(true);
    setError(null);
    try {
      await api.tasks.move(task.id, "cancelled");
      await queryClient.invalidateQueries({
        queryKey: taskKeys.list(task.projectId),
      });
    } catch (err) {
      setError(extractErrorMessage(err, "Failed to cancel task"));
    } finally {
      setIsProcessing(false);
    }
  }, [task.id, task.projectId, queryClient, confirm]);

  return (
    <TwoColumnLayout
      description={task.description}
      testId="merge-conflict-task-detail"
    >
      {/* Status Banner */}
      <StatusBanner
        icon={
          conflictType === "plan_update" || conflictType === "source_update"
            ? RefreshCw
            : AlertTriangle
        }
        title={
          conflictType === "plan_update"
            ? "Plan Update Conflict"
            : conflictType === "source_update"
            ? "Task Update Conflict"
            : "Merge Conflict"
        }
        subtitle={
          conflictType === "plan_update"
            ? isHistorical
              ? "Manual resolution was required to update plan from main"
              : "Manual resolution required to update plan from main"
            : conflictType === "source_update"
            ? isHistorical
              ? "Manual resolution was required to update task from plan"
              : "Manual resolution required to update task from plan"
            : isHistorical
            ? "Manual resolution was required"
            : "Manual resolution required"
        }
        variant={
          conflictType === "plan_update" || conflictType === "source_update"
            ? "info"
            : "warning"
        }
        badge={
          <StatusPill
            icon={
              conflictType === "plan_update" || conflictType === "source_update"
                ? RefreshCw
                : AlertTriangle
            }
            label="Conflict"
            variant={
              conflictType === "plan_update" || conflictType === "source_update"
                ? "info"
                : "warning"
            }
            size="md"
          />
        }
      />

      {/* Conflict Files */}
      <section data-testid="conflict-files-section">
        <SectionTitle>
          Conflict Files ({conflictFiles.length})
          {isConflictDetectionEnabled && isLoadingConflicts && (
            <Loader2 className="inline-block w-3.5 h-3.5 ml-2 animate-spin text-text-primary/40" />
          )}
        </SectionTitle>
        <DetailCard variant="warning">
          {isConflictDetectionEnabled && isLoadingConflicts && conflictFiles.length === 0 ? (
            <div className="flex items-center gap-2 py-2">
              <Loader2 className="w-4 h-4 animate-spin" style={{ color: "var(--status-warning)" }} />
              <span className="text-[0.8125rem] text-text-primary/50">Detecting conflicts...</span>
            </div>
          ) : (
            <ConflictFilesList files={conflictFiles} taskId={task.id} />
          )}
        </DetailCard>
      </section>

      {/* Resolution Instructions (not in historical mode) */}
      {!isHistorical && (
        <section data-testid="resolution-instructions-section">
          <SectionTitle>How to Resolve</SectionTitle>
          <DetailCard>
            <ResolutionInstructions branchName={branchName} />
          </DetailCard>
        </section>
      )}

      {/* Error Display */}
      {error && (
        <div
          className="p-3 rounded-lg text-[0.8125rem]"
          style={{
            backgroundColor: statusTint("error", 12),
            color: "var(--status-error)",
          }}
        >
          {error}
        </div>
      )}

      {/* Actions (hidden in historical mode) */}
      {!isHistorical && (
        <section data-testid="action-buttons">
          <ActionButtonsCard
            onResolve={handleResolveConflicts}
            onRetry={handleRetryMerge}
            onCancel={handleCancel}
            isProcessing={isProcessing}
          />
        </section>
      )}
    </TwoColumnLayout>
  );
}
