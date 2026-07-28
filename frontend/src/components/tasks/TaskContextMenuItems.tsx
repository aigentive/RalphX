/* eslint-disable react-refresh/only-export-components */
/**
 * TaskContextMenuItems - Shared context menu items for task cards and graph nodes.
 *
 * Renders ContextMenuItem elements (no ContextMenu/ContextMenuTrigger wrapper).
 * Uses the shared action registry to produce: View Details, Edit, status-specific
 * actions, archive/restore, and delete — with confirmation dialogs and BlockReasonDialog.
 *
 * Both TaskCardContextMenu and TaskNodeContextMenu render this component inside
 * their respective ContextMenu wrappers.
 *
 * Usage (Items inside ContextMenuContent, Dialogs outside):
 * ```tsx
 * const menuState = useTaskContextMenu();
 * <TaskContextMenuProvider state={menuState}>
 *   <ContextMenu>
 *     <ContextMenuTrigger>{children}</ContextMenuTrigger>
 *     <ContextMenuContent>
 *       <TaskContextMenuItems task={task} handlers={handlers} context="kanban" />
 *     </ContextMenuContent>
 *     <TaskContextMenuDialogs task={task} handlers={handlers} />
 *   </ContextMenu>
 * </TaskContextMenuProvider>
 * ```
 */

import { useState, createContext, useContext, useCallback } from "react";

import { useAgentGate, useActiveEffectiveScopes } from "@/hooks/useAgentGate";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import {
  resolveAffordanceGate,
  type AgentGatedAffordance,
} from "@/lib/remote/agent-gate";
import {
  ContextMenuItem,
  ContextMenuSeparator,
} from "@/components/ui/context-menu";
import { Eye, Pencil, Archive, RotateCcw, Lightbulb } from "lucide-react";
import type { Task } from "@/types/task";
import type { TaskAction, ActionSurface } from "@/lib/task-actions";
import { getTaskActions, canEdit } from "@/lib/task-actions";
import { useConfirmation } from "@/hooks/useConfirmation";
import { BlockReasonDialog } from "./BlockReasonDialog";

// ============================================================================
// Handler Interface
// ============================================================================

/**
 * Union of all possible handler callbacks for task context menu actions.
 * Consumers provide only the handlers relevant to their surface.
 */
export interface TaskContextMenuHandlers {
  onViewDetails: () => void;
  onEdit?: () => void;
  onArchive?: () => void;
  onRestore?: () => void;
  /**
   * @deprecated Delete has been removed from the UI. Kept for backwards
   * compatibility with callers. Has no effect.
   */
  onPermanentDelete?: () => void;
  onStatusChange?: (newStatus: string) => void;
  onBlockWithReason?: (reason?: string) => void;
  onUnblock?: () => void;
  onStartExecution?: () => void;
  onPause?: () => void;
  onResume?: () => void;
  onApprove?: () => void;
  onReject?: () => void;
  onRequestChanges?: () => void;
  onMarkResolved?: () => void;
  onStartIdeation?: () => void;
  onViewAgentChat?: () => void;
  /**
   * @deprecated Remove action has been removed from the UI. Kept for backwards
   * compatibility with callers. Has no effect.
   */
  onRemove?: () => void;
}

// ============================================================================
// Props
// ============================================================================

export interface TaskContextMenuItemsProps {
  task: Task;
  handlers: TaskContextMenuHandlers;
  /** Which surface is rendering — determines which action set to show */
  context?: ActionSurface;
}

// ============================================================================
// Shared dialog state (connects Items ↔ Dialogs via context)
// ============================================================================

interface DialogState {
  showBlockDialog: boolean;
  setShowBlockDialog: (show: boolean) => void;
  confirm: (opts: {
    title: string;
    description: string;
    confirmText?: string;
    variant?: "default" | "destructive";
  }) => Promise<boolean>;
  confirmationDialogProps: ReturnType<typeof useConfirmation>["confirmationDialogProps"];
  ConfirmationDialog: ReturnType<typeof useConfirmation>["ConfirmationDialog"];
}

const DialogStateContext = createContext<DialogState | null>(null);

/** Hook to create shared state for TaskContextMenuItems + TaskContextMenuDialogs */
export function useTaskContextMenu() {
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const [showBlockDialog, setShowBlockDialog] = useState(false);

  return { showBlockDialog, setShowBlockDialog, confirm, confirmationDialogProps, ConfirmationDialog };
}

/** Provider that wraps ContextMenu to share dialog state between Items and Dialogs */
export function TaskContextMenuProvider({
  children,
  state,
}: {
  children: React.ReactNode;
  state: ReturnType<typeof useTaskContextMenu>;
}) {
  return (
    <DialogStateContext.Provider value={state}>
      {children}
    </DialogStateContext.Provider>
  );
}

// ============================================================================
// Handler key → actual handler resolution
// ============================================================================

function resolveHandler(
  key: string,
  handlers: TaskContextMenuHandlers,
  action: TaskAction,
): (() => void) | undefined {
  switch (key) {
    case "onStatusChange":
      return handlers.onStatusChange
        ? () => {
            const statusMap: Record<string, string> = {
              cancel: "cancelled",
              reopen: "backlog",
              retry: "ready",
              unblock: "ready",
            };
            handlers.onStatusChange!(statusMap[action.id] ?? action.id);
          }
        : undefined;
    case "onBlockWithReason":
      return undefined; // Handled via BlockReasonDialog
    case "onUnblock":
      return handlers.onUnblock;
    case "onStartExecution":
      return handlers.onStartExecution;
    case "onPause":
      return handlers.onPause;
    case "onResume":
      return handlers.onResume;
    case "onApprove":
      return handlers.onApprove;
    case "onReject":
      return handlers.onReject;
    case "onRequestChanges":
      return handlers.onRequestChanges;
    case "onMarkResolved":
      return handlers.onMarkResolved;
    case "onViewAgentChat":
      return handlers.onViewAgentChat ?? handlers.onViewDetails;
    default:
      return undefined;
  }
}

/**
 * Handler keys that STEER work forward and therefore require `ui:agent` (2.6-b).
 *
 * The complement is deliberate, not an oversight: `onPause`, `onBlockWithReason`,
 * `onStatusChange` for `cancel`, and the view-only actions are authority-reducing or
 * inert, so a paired device keeps them under the viewer-with-brakes boundary. This
 * one map covers both the Kanban and graph menus, so neither surface can drift from
 * the other.
 */
const AGENT_STEERING_HANDLER_KEYS: ReadonlySet<string> = new Set([
  "onStatusChange",
  "onUnblock",
  "onStartExecution",
  "onResume",
  "onApprove",
  "onReject",
  "onRequestChanges",
  "onMarkResolved",
]);

/**
 * The facade op each steering handler fronts, so a menu item can say WHY it is
 * disabled. `onResume` maps to `resume_task`, which the host does not expose
 * remotely at all — that item reads "runs only on the host", not "enable agent
 * control". Keys with no entry fall back to the scope-only answer.
 */
const HANDLER_AFFORDANCES: Readonly<
  Partial<Record<string, AgentGatedAffordance>>
> = {
  onStatusChange: "taskMove",
  onUnblock: "taskUnblock",
  onStartExecution: "taskMove",
  onResume: "taskResume",
  onApprove: "taskApprove",
};

// ============================================================================
// Items Component (renders inside ContextMenuContent)
// ============================================================================

export function TaskContextMenuItems({
  task,
  handlers,
  context = "kanban",
}: TaskContextMenuItemsProps) {
  const dialogState = useContext(DialogStateContext);
  if (!dialogState) {
    throw new Error("TaskContextMenuItems must be wrapped in TaskContextMenuProvider");
  }

  const { confirm, setShowBlockDialog } = dialogState;
  const agentGate = useAgentGate();
  const isRemote = useIsRemoteEnvironment();
  const scopes = useActiveEffectiveScopes();
  const gateForAction = useCallback(
    (action: TaskAction) => {
      if (!AGENT_STEERING_HANDLER_KEYS.has(action.handlerKey)) return null;
      const affordance = HANDLER_AFFORDANCES[action.handlerKey];
      const gate =
        affordance === undefined
          ? agentGate
          : resolveAffordanceGate(affordance, isRemote, scopes);
      return gate.gated ? gate : null;
    },
    [agentGate, isRemote, scopes]
  );

  const isArchived = task.archivedAt !== null;
  const canEditTask = canEdit(task);
  const isBacklog = task.internalStatus === "backlog";
  const statusActions = getTaskActions(task.internalStatus, context);

  const handleRegistryAction = useCallback(async (action: TaskAction) => {
    // Belt-and-braces: the item is already disabled, but a keyboard activation path
    // must not be able to dispatch a steering mutation either.
    if (gateForAction(action) !== null) {
      return;
    }
    if (action.opensDialog && action.handlerKey === "onBlockWithReason") {
      setShowBlockDialog(true);
      return;
    }

    const handler = resolveHandler(action.handlerKey, handlers, action);
    if (!handler) {
      handlers.onViewDetails();
      return;
    }

    if (action.isViewAction) {
      handler();
      return;
    }

    if (action.confirmConfig) {
      const confirmed = await confirm({
        title: action.confirmConfig.title,
        description: action.confirmConfig.description,
        confirmText: action.label,
        variant: action.confirmConfig.variant,
      });
      if (confirmed) handler();
      return;
    }

    handler();
  }, [gateForAction, handlers, confirm, setShowBlockDialog]);

  const handleArchive = useCallback(async () => {
    const confirmed = await confirm({
      title: "Archive this task?",
      description: "The task will be moved to the archive.",
      confirmText: "Archive",
      variant: "default",
    });
    if (confirmed) handlers.onArchive?.();
  }, [confirm, handlers]);

  const handleRestore = useCallback(async () => {
    const confirmed = await confirm({
      title: "Restore this task?",
      description: "The task will be restored to the backlog.",
      confirmText: "Restore",
      variant: "default",
    });
    if (confirmed) handlers.onRestore?.();
  }, [confirm, handlers]);

  return (
    <>
      <ContextMenuItem
        onClick={handlers.onViewDetails}
        data-testid="view-details-action"
      >
        <Eye className="w-4 h-4 mr-2" />
        View Details
      </ContextMenuItem>

      {canEditTask && handlers.onEdit && (
        <ContextMenuItem onClick={handlers.onEdit}>
          <Pencil className="w-4 h-4 mr-2" />
          Edit
        </ContextMenuItem>
      )}

      {isBacklog && handlers.onStartIdeation && (
        <ContextMenuItem onClick={handlers.onStartIdeation}>
          <Lightbulb className="w-4 h-4 mr-2" />
          Start Ideation
        </ContextMenuItem>
      )}

      {statusActions.length > 0 && (
        <>
          <ContextMenuSeparator />
          {statusActions.map((action) => {
            const gate = gateForAction(action);
            const gated = gate !== null;
            return (
            <ContextMenuItem
              key={action.id}
              disabled={gated}
              onClick={() => handleRegistryAction(action)}
              className={action.variant === "destructive" ? "text-destructive" : ""}
              data-testid={`${action.id}-action`}
              {...(gate !== null
                ? { "data-agent-gated": "true", title: gate.reason ?? undefined }
                : {})}
            >
              <action.icon className="w-4 h-4 mr-2" />
              {action.label}
            </ContextMenuItem>
            );
          })}
        </>
      )}

      {!isArchived && handlers.onArchive && (
        <>
          <ContextMenuSeparator />
          <ContextMenuItem onClick={handleArchive}>
            <Archive className="w-4 h-4 mr-2" />
            Archive
          </ContextMenuItem>
        </>
      )}

      {isArchived && handlers.onRestore && (
        <>
          <ContextMenuSeparator />
          <ContextMenuItem onClick={handleRestore}>
            <RotateCcw className="w-4 h-4 mr-2" />
            Restore
          </ContextMenuItem>
        </>
      )}
    </>
  );
}

// ============================================================================
// Dialogs Component (render as sibling of ContextMenuContent, NOT inside it)
// ============================================================================

export function TaskContextMenuDialogs({
  task,
  handlers,
}: {
  task: Task;
  handlers: TaskContextMenuHandlers;
}) {
  const dialogState = useContext(DialogStateContext);
  if (!dialogState) {
    throw new Error("TaskContextMenuDialogs must be wrapped in TaskContextMenuProvider");
  }

  const {
    showBlockDialog,
    setShowBlockDialog,
    confirmationDialogProps,
    ConfirmationDialog,
  } = dialogState;

  return (
    <>
      <ConfirmationDialog {...confirmationDialogProps} />
      <BlockReasonDialog
        isOpen={showBlockDialog}
        onClose={() => setShowBlockDialog(false)}
        onConfirm={(reason) => {
          handlers.onBlockWithReason?.(reason);
          setShowBlockDialog(false);
        }}
        taskTitle={task.title}
      />
    </>
  );
}
