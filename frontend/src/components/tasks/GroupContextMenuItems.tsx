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
import { cn } from "@/lib/utils";
import { useCallback } from "react";
import {
  ContextMenuItem,
  ContextMenuSeparator,
} from "@/components/ui/context-menu";
import {
  EXPLAINED_DISABLED_MENU_ITEM_CLASS,
  MenuItemExplanation,
  explainedDisabledMenuItemProps,
} from "@/components/ui/menu-item-explanation";
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
  const cancelGate = useAgentGate("groupCancel");
  const pauseGate = useAgentGate("groupPause");
  const resumeGate = useAgentGate("groupResume");
  const cancelAction = GROUP_ACTIONS.cancelAll;
  const pauseAction = GROUP_ACTIONS.pauseAll;
  const resumeAction = GROUP_ACTIONS.resumeAll;
  const archiveAction = GROUP_ACTIONS.archiveAll;

  const cancelLabel = getCancelAllLabel(groupKind, groupLabel);
  const pauseLabel = getPauseAllLabel(groupKind, groupLabel);
  const resumeLabel = getResumeAllLabel(groupKind, groupLabel);
  const archiveLabel = getArchiveAllLabel(groupKind, groupLabel);

  const handleCancelAll = useCallback(async () => {
    if (cancelGate.gated) return;
    if (taskCount === 0 || !onCancelAll) return;
    const config = cancelAction.confirmConfig(groupLabel, taskCount);
    const confirmed = await confirm({
      title: config.title,
      description: config.description,
      confirmText: "Cancel",
      variant: config.variant,
    });
    if (confirmed) onCancelAll();
  }, [
    cancelGate.gated,
    taskCount,
    cancelAction,
    groupLabel,
    confirm,
    onCancelAll,
  ]);

  const handlePauseAll = useCallback(async () => {
    if (pauseGate.gated) return;
    if (taskCount === 0 || !onPauseAll) return;
    const config = pauseAction.confirmConfig(groupLabel, taskCount);
    const confirmed = await confirm({
      title: config.title,
      description: config.description,
      confirmText: "Pause",
      variant: config.variant,
    });
    if (confirmed) onPauseAll();
  }, [
    pauseGate.gated,
    taskCount,
    pauseAction,
    groupLabel,
    confirm,
    onPauseAll,
  ]);

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
        <MenuItemExplanation
          reason={pauseGate.gated ? pauseGate.reason : null}
          testId="pause-all-gate-explanation"
        >
          <ContextMenuItem
            onClick={pauseGate.gated ? undefined : handlePauseAll}
            data-testid="pause-all-action"
            className={
              pauseGate.gated ? EXPLAINED_DISABLED_MENU_ITEM_CLASS : undefined
            }
            {...(pauseGate.gated
              ? {
                  "data-agent-gated": "true",
                  "aria-label":
                    `${pauseLabel} — ${pauseGate.reason ?? ""}`.trim(),
                  ...explainedDisabledMenuItemProps(),
                }
              : {})}
          >
            <PauseIcon className="w-4 h-4 mr-2" />
            {pauseLabel}
          </ContextMenuItem>
        </MenuItemExplanation>
      )}
      {onResumeAll && (
        // Soft-disabled, not `disabled`: a Radix-disabled item has
        // `pointer-events: none` and leaves the roving-focus order, so its reason
        // would be unreachable by both mouse and keyboard.
        <MenuItemExplanation
          reason={resumeGate.gated ? resumeGate.reason : null}
          testId="resume-all-gate-explanation"
        >
          <ContextMenuItem
            onClick={resumeGate.gated ? undefined : handleResumeAll}
            className={
              resumeGate.gated ? EXPLAINED_DISABLED_MENU_ITEM_CLASS : undefined
            }
            data-testid="resume-all-action"
            {...(resumeGate.gated
              ? {
                  "data-agent-gated": "true",
                  "aria-label":
                    `${resumeLabel} — ${resumeGate.reason ?? ""}`.trim(),
                  ...explainedDisabledMenuItemProps(),
                }
              : {})}
          >
            <ResumeIcon className="w-4 h-4 mr-2" />
            {resumeLabel}
          </ContextMenuItem>
        </MenuItemExplanation>
      )}
      {onArchiveAll && (
        <ContextMenuItem
          onClick={handleArchiveAll}
          data-testid="archive-all-action"
        >
          <ArchiveIcon className="w-4 h-4 mr-2" />
          {archiveLabel}
        </ContextMenuItem>
      )}
      {onCancelAll && (onPauseAll ?? onResumeAll ?? onArchiveAll) && (
        <ContextMenuSeparator />
      )}
      {onCancelAll && (
        <MenuItemExplanation
          reason={cancelGate.gated ? cancelGate.reason : null}
          testId="cancel-all-gate-explanation"
        >
          <ContextMenuItem
            onClick={cancelGate.gated ? undefined : handleCancelAll}
            className={cn(
              "text-destructive",
              cancelGate.gated && EXPLAINED_DISABLED_MENU_ITEM_CLASS,
            )}
            data-testid="cancel-all-action"
            {...(cancelGate.gated
              ? {
                  "data-agent-gated": "true",
                  "aria-label":
                    `${cancelLabel} — ${cancelGate.reason ?? ""}`.trim(),
                  ...explainedDisabledMenuItemProps(),
                }
              : {})}
          >
            <CancelIcon className="w-4 h-4 mr-2" />
            {cancelLabel}
          </ContextMenuItem>
        </MenuItemExplanation>
      )}
    </>
  );
}
