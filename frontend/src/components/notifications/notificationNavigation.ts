import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { requestAutomationRunOpen } from "@/components/automations/automationRunNavigation";
import { tasksApi } from "@/api/tasks";
import { navigateToIdeationSession } from "@/lib/navigation";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import type { NotificationCategory, NotificationTarget } from "@/types/notifications";

const AUTOMATION_NOTIFICATION_TAB_HINTS = {
  automation_plan_approval: "plan",
  automation_run_failed: "automation",
  automation_run_completed: "pr",
} as const;

function permissionRequestId(id: string): string {
  return id.replace(/^(?:permission|perm):/, "");
}

export interface NotificationNavigationItem {
  id: string;
  category: NotificationCategory;
  target: NotificationTarget;
}

export interface NotificationNavigationOptions {
  onClose?: () => void;
  onOpenAutomationDetail?: (automationId: string) => void;
}

/** The one target dispatcher used by attention rows, history rows, and toast actions. */
export async function navigateNotification(
  item: NotificationNavigationItem,
  queryClient: QueryClient,
  options: NotificationNavigationOptions = {},
) {
  if (item.category === "permission_request") {
    window.dispatchEvent(new CustomEvent("ralphx:open-permission-dialog", {
      detail: { requestId: permissionRequestId(item.id) },
    }));
    options.onClose?.();
    return;
  }

  const { target } = item;
  if (target.kind === "task" && target.taskId) {
    try {
      await tasksApi.get(target.taskId);
    } catch {
      toast.error("This task no longer exists.");
      return;
    }

    const projectState = useProjectStore.getState();
    if (target.projectId && projectState.activeProjectId !== target.projectId) {
      const uiState = useUiStore.getState();
      useUiStore.setState({
        viewByProject: { ...uiState.viewByProject, [target.projectId]: "kanban" },
        selectedTaskByProject: { ...uiState.selectedTaskByProject, [target.projectId]: target.taskId },
      });
      projectState.selectProject(target.projectId);
    } else {
      useUiStore.getState().navigateToTask(target.taskId);
    }
    options.onClose?.();
    return;
  }
  if (target.kind === "agent_conversation") {
    const conversationId = target.conversationId ?? target.setupConversationId;
    if (conversationId) navigateToIdeationSession(conversationId);
  }
  if (
    target.kind === "automation_run" &&
    target.projectId &&
    target.automationId &&
    target.runId &&
    target.conversationId
  ) {
    void requestAutomationRunOpen(queryClient, {
      projectId: target.projectId,
      automationId: target.automationId,
      runId: target.runId,
      conversationId: target.conversationId,
      ...(target.setupConversationId && { setupConversationId: target.setupConversationId }),
    }, {
      ...(options.onOpenAutomationDetail && { onOpenAutomationDetail: options.onOpenAutomationDetail }),
      ...(item.category in AUTOMATION_NOTIFICATION_TAB_HINTS && {
        tabHint: AUTOMATION_NOTIFICATION_TAB_HINTS[
          item.category as keyof typeof AUTOMATION_NOTIFICATION_TAB_HINTS
        ],
      }),
    });
  } else if (target.kind === "automation_run" && target.automationId) {
    options.onOpenAutomationDetail?.(target.automationId);
  }
  if (target.kind === "project" && target.projectId) {
    useProjectStore.getState().selectProject(target.projectId);
    useUiStore.getState().setCurrentView("kanban");
  }
  if (target.kind !== "none") options.onClose?.();
}
