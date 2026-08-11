/**
 * ActiveMergeCard - Compact row for actively merging task
 *
 * Single-line row: spinner | title | branch (short) | elapsed | stop button
 */

import { Square, Loader2 } from "lucide-react";
import type { MergePipelineTask } from "@/api/merge-pipeline";
import { useCallback, useState } from "react";
import { getStatusIconConfig } from "@/types/status-icons";
import { BranchBadge } from "@/components/shared/BranchBadge";
interface ActiveMergeCardProps {
  task: MergePipelineTask;
  onStop: (taskId: string) => void;
  onViewDetails: (taskId: string) => void;
}

function formatElapsedTime(startTime: Date): string {
  const now = new Date();
  const elapsed = Math.floor((now.getTime() - startTime.getTime()) / 1000);

  if (elapsed < 60) return `${elapsed}s`;
  if (elapsed < 3600) return `${Math.floor(elapsed / 60)}m ${elapsed % 60}s`;

  const hours = Math.floor(elapsed / 3600);
  const minutes = Math.floor((elapsed % 3600) / 60);
  return `${hours}h ${minutes}m`;
}

export function ActiveMergeCard({ task, onStop, onViewDetails }: ActiveMergeCardProps) {
  const mergingStyle = getStatusIconConfig("merging");
  const conflictStyle = getStatusIconConfig("merge_conflict");
  const stoppedStyle = getStatusIconConfig("stopped");
  const [startTime] = useState(() => new Date(Date.now() - 30000));
  const elapsedTime = formatElapsedTime(startTime);

  const handleStop = useCallback(() => {
    onStop(task.taskId);
  }, [onStop, task.taskId]);

  const conflictInfo = task.conflictFiles && task.conflictFiles.length > 0
    ? ` | ${task.conflictFiles.length} conflicts`
    : "";

  return (
    <div
      className="flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-white/[0.04] transition-colors"
      title={`merging → ${task.targetBranch}${conflictInfo}\nAgent resolving | ${elapsedTime}`}
    >
      <Loader2
        className="w-3.5 h-3.5 animate-spin shrink-0"
        style={{ color: mergingStyle.color }}
      />
      <button
        className="flex-1 text-xs font-medium truncate min-w-0 text-left cursor-pointer hover:opacity-75 transition-opacity"
        style={{ color: "var(--text-primary)" }}
        onClick={() => onViewDetails(task.taskId)}
      >
        {task.displayTitle ?? task.title}
      </button>
      <span className="shrink-0 max-w-[100px] truncate">
        <BranchBadge branch={task.targetBranch} variant="muted" size="sm" />
      </span>
      {task.conflictFiles && task.conflictFiles.length > 0 && (
        <span
          className="text-[0.6875rem] shrink-0"
          style={{ color: conflictStyle.color }}
        >
          {task.conflictFiles.length}cf
        </span>
      )}
      <span
        className="text-[0.6875rem] shrink-0 tabular-nums"
        style={{ color: "var(--text-muted)" }}
      >
        {elapsedTime}
      </span>
      <button
        onClick={handleStop}
        className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors shrink-0"
        style={{ color: stoppedStyle.color }}
        title="Stop merge"
      >
        <Square className="w-2.5 h-2.5 fill-current" />
      </button>
    </div>
  );
}
