/**
 * GroupContextMenuItems — Renders group-level bulk actions as ContextMenuItems.
 *
 * Reusable across Kanban columns, Graph plan groups, and Graph uncategorized
 * containers. Renders ContextMenuItem elements only (no wrapper).
 *
 * Usage:
 * ```tsx
 * const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
 *
 * <ContextMenu>
 *   <ContextMenuTrigger>{children}</ContextMenuTrigger>
 *   <ContextMenuContent>
 *     <GroupContextMenuItems
 *       groupLabel="Ready"
 *       groupKind="column"
 *       taskCount={5}
 *       projectId="proj-123"
 *       groupId="ready"
 *       onCancelAll={handleCancelAll}
 *       onPauseAll={handlePauseAll}
 *       onResumeAll={handleResumeAll}
 *       onArchiveAll={handleArchiveAll}
 *       confirm={confirm}
 *     />
 *   </ContextMenuContent>
 *   <ConfirmationDialog {...confirmationDialogProps} />
 * </ContextMenu>
 * ```
 */

import { useAgentGate } from "@/hooks/useAgentGate";
import { useCallback } from "react";
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu";
import {
  type GroupKind,
  GROUP_ACTIONS,
  getCancelAllLabel,
  getPauseAllLabel,
  getResumeAllLabel,
  getArchiveAllLabel,
} from "@/lib/task-actions";

export interface GroupContextMenuItemsProps {
  /** Display label for the group (e.g. "Ready", plan name, "Uncategorized") */
  groupLabel: string;
  /** What kind of group: column (status), plan, or uncategorized */
  groupKind: GroupKind;
  /** Number of tasks in this group */
  taskCount: number;
  /** Project ID for cleanup API */
  projectId: string;
  /** Group identifier: status name or session ID */
  groupId: string;
  /** Optional handler called after user confirms cancellation */
  onCancelAll?: () => void;
  /** Optional handler called after user confirms pause all */
  onPauseAll?: () => void;
  /** Optional handler called after user confirms resume all */
  onResumeAll?: () => void;
  /** Optional handler called after user confirms archive all */
  onArchiveAll?: () => void;
  /** Confirm function from useConfirmation hook */
  confirm: (opts: {
    title: string;
    description: string;
    confirmText?: string;
    variant?: "default" | "destructive";
  }) => Promise<boolean>;
}

export function GroupContextMenuItems({
  groupLabel,
  groupKind,
  taskCount,
  onCancelAll,
  onPauseAll,
  onResumeAll,
  onArchiveAll,
  confirm,
}: GroupContextMenuItemsProps) {
  const agentGate = useAgentGate();
  const cancelAction = GROUP_ACTIONS.cancelAll;
  const pauseAction = GROUP_ACTIONS.pauseAll;
  const resumeAction = GROUP_ACTIONS.resumeAll;
  const archiveAction = GROUP_ACTIONS.archiveAll;

  const cancelLabel = getCancelAllLabel(groupKind, groupLabel);
  const pauseLabel = getPauseAllLabel(groupKind, groupLabel);
  const resumeLabel = getResumeAllLabel(groupKind, groupLabel);
  const archiveLabel = getArchiveAllLabel(groupKind, groupLabel);

  const handleCancelAll = useCallback(async () => {
    if (taskCount === 0 || !onCancelAll) return;
    const config = cancelAction.confirmConfig(groupLabel, taskCount);
    const confirmed = await confirm({
      title: config.title,
      description: config.description,
      confirmText: "Cancel",
      variant: config.variant,
    });
    if (confirmed) onCancelAll();
  }, [taskCount, cancelAction, groupLabel, confirm, onCancelAll]);

  const handlePauseAll = useCallback(async () => {
    if (taskCount === 0 || !onPauseAll) return;
    const config = pauseAction.confirmConfig(groupLabel, taskCount);
    const confirmed = await confirm({
      title: config.title,
      description: config.description,
      confirmText: "Pause",
      variant: config.variant,
    });
    if (confirmed) onPauseAll();
  }, [taskCount, pauseAction, groupLabel, confirm, onPauseAll]);

  const handleResumeAll = useCallback(async () => {
    if (taskCount === 0 || !onResumeAll) return;
    const config = resumeAction.confirmConfig(groupLabel, taskCount);
    const confirmed = await confirm({
      title: config.title,
      description: config.description,
      confirmText: "Resume",
      variant: config.variant,
    });
    if (confirmed) onResumeAll();
  }, [taskCount, resumeAction, groupLabel, confirm, onResumeAll]);

  const handleArchiveAll = useCallback(async () => {
    if (taskCount === 0 || !onArchiveAll) return;
    const config = archiveAction.confirmConfig(groupLabel, taskCount);
    const confirmed = await confirm({
      title: config.title,
      description: config.description,
      confirmText: "Archive",
      variant: config.variant,
    });
    if (confirmed) onArchiveAll();
  }, [taskCount, archiveAction, groupLabel, confirm, onArchiveAll]);

  if (taskCount === 0) return null;

  const hasAnyAction = onPauseAll ?? onResumeAll ?? onArchiveAll ?? onCancelAll;
  if (!hasAnyAction) return null;

  const PauseIcon = pauseAction.icon;
  const ResumeIcon = resumeAction.icon;
  const ArchiveIcon = archiveAction.icon;
  const CancelIcon = cancelAction.icon;

  return (
    <>
      {onPauseAll && (
        <ContextMenuItem onClick={handlePauseAll} data-testid="pause-all-action">
          <PauseIcon className="w-4 h-4 mr-2" />
          {pauseLabel}
        </ContextMenuItem>
      )}
      {onResumeAll && (
        <ContextMenuItem
          onClick={handleResumeAll}
          disabled={agentGate.gated}
          data-testid="resume-all-action"
          {...(agentGate.gated
            ? { "data-agent-gated": "true", title: agentGate.reason ?? undefined }
            : {})}
        >
          <ResumeIcon className="w-4 h-4 mr-2" />
          {resumeLabel}
        </ContextMenuItem>
      )}
      {onArchiveAll && (
        <ContextMenuItem onClick={handleArchiveAll} data-testid="archive-all-action">
          <ArchiveIcon className="w-4 h-4 mr-2" />
          {archiveLabel}
        </ContextMenuItem>
      )}
      {onCancelAll && (onPauseAll ?? onResumeAll ?? onArchiveAll) && (
        <ContextMenuSeparator />
      )}
      {onCancelAll && (
        <ContextMenuItem
          onClick={handleCancelAll}
          className="text-destructive"
          data-testid="cancel-all-action"
        >
          <CancelIcon className="w-4 h-4 mr-2" />
          {cancelLabel}
        </ContextMenuItem>
      )}
    </>
  );
}
