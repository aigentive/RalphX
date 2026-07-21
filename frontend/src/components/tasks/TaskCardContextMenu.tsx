/**
 * TaskCardContextMenu - Right-click context menu for task cards (Kanban)
 *
 * Keeps the ContextMenu/ContextMenuTrigger wrapper (Kanban-specific)
 * and delegates all menu item rendering to the shared TaskContextMenuItems.
 * Dialogs are rendered outside ContextMenuContent to survive menu close.
 */

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import type { Task } from "@/types/task";
import type { GroupInfo } from "@/lib/task-actions";
import {
  TaskContextMenuItems,
  TaskContextMenuDialogs,
  TaskContextMenuProvider,
  useTaskContextMenu,
  type TaskContextMenuHandlers,
} from "./TaskContextMenuItems";
import { GroupContextMenuItems } from "./GroupContextMenuItems";

interface TaskCardContextMenuProps extends TaskContextMenuHandlers {
  task: Task;
  children: React.ReactNode;
  groupInfo?: GroupInfo;
  readOnly?: boolean;
}

export function TaskCardContextMenu({
  task,
  children,
  groupInfo,
  readOnly = false,
  ...handlers
}: TaskCardContextMenuProps) {
  const menuState = useTaskContextMenu();

  if (readOnly) return children;

  return (
    <TaskContextMenuProvider state={menuState}>
      <ContextMenu>
        <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
        <ContextMenuContent>
          <TaskContextMenuItems task={task} handlers={handlers} context="kanban" />
          {groupInfo && (
            <>
              <ContextMenuSeparator />
              <GroupContextMenuItems
                groupLabel={groupInfo.groupLabel}
                groupKind={groupInfo.groupKind}
                taskCount={groupInfo.taskCount}
                projectId={groupInfo.projectId}
                groupId={groupInfo.groupId}
                {...(groupInfo.onCancelAll !== undefined && { onCancelAll: groupInfo.onCancelAll })}
                {...(groupInfo.onPauseAll !== undefined && { onPauseAll: groupInfo.onPauseAll })}
                {...(groupInfo.onResumeAll !== undefined && { onResumeAll: groupInfo.onResumeAll })}
                {...(groupInfo.onArchiveAll !== undefined && { onArchiveAll: groupInfo.onArchiveAll })}
                confirm={menuState.confirm}
              />
            </>
          )}
        </ContextMenuContent>
        <TaskContextMenuDialogs task={task} handlers={handlers} />
      </ContextMenu>
    </TaskContextMenuProvider>
  );
}
