import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { requestAutomationRunOpen } from "@/components/automations/automationRunNavigation";
import { automationsApi } from "@/api/automations";
import { permissionApi } from "@/api/permission";
import {
  REMOTE_UNAVAILABLE_HINT,
  remoteErrorBannerProps,
} from "@/lib/remote/agent-gate";
import { isRemoteTransportError } from "@/lib/remote/transport-errors";
import {
  navigateToAgentConversation,
  navigateToAgentPlan,
  navigateToIdeationSession,
  openTaskInAgents,
} from "@/lib/navigation";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import type { NotificationCategory, NotificationTarget } from "@/types/notifications";

const AUTOMATION_NOTIFICATION_TAB_HINTS = {
  automation_plan_approval: "plan",
  automation_run_failed: "automation",
  automation_run_completed: "pr",
} as const;
const ATTENTION_QUERY_KEY = ["attention"] as const;
const AUTOMATIONS_QUERY_KEY = ["automations"] as const;
const AUTOMATION_SIDEBAR_QUERY_KEY = [
  "agents",
  "sidebar-conversations",
  "automation",
] as const;

function permissionRequestId(item: NotificationNavigationItem): string | null {
  if (item.dedupeKey?.startsWith("perm:")) {
    return item.dedupeKey.slice("perm:".length) || null;
  }
  if (item.id.startsWith("permission:")) {
    return item.id.slice("permission:".length) || null;
  }
  return null;
}

export interface NotificationNavigationItem {
  id: string;
  dedupeKey?: string | undefined;
  category: NotificationCategory;
  target: NotificationTarget;
}

export interface NotificationNavigationOptions {
  onClose?: () => void;
  onOpenAutomationDetail?: (automationId: string) => void;
}

export async function performNotificationPrimaryAction(
  item: NotificationNavigationItem,
  queryClient: QueryClient,
  options: NotificationNavigationOptions = {},
): Promise<boolean> {
  if (item.category !== "automation_paused" || !item.target.automationId) {
    return navigateNotification(item, queryClient, options);
  }
  const automationId = item.target.automationId;
  try {
    await automationsApi.resume(automationId);
    void queryClient.invalidateQueries({ queryKey: AUTOMATIONS_QUERY_KEY });
    void queryClient.invalidateQueries({ queryKey: AUTOMATION_SIDEBAR_QUERY_KEY });
    void queryClient.invalidateQueries({ queryKey: ATTENTION_QUERY_KEY });
    toast.success("Automation resumed");
    options.onClose?.();
    return true;
  } catch (error) {
    void queryClient.invalidateQueries({ queryKey: ATTENTION_QUERY_KEY });
    const remoteBanner = remoteErrorBannerProps(error);
    toast.error(
      isRemoteTransportError(error)
        ? (remoteBanner?.body ?? REMOTE_UNAVAILABLE_HINT)
        : "Automation is no longer resumable",
    );
    return false;
  }
}

/** The one target dispatcher used by attention rows, history rows, and toast actions. */
export async function navigateNotification(
  item: NotificationNavigationItem,
  queryClient: QueryClient,
  options: NotificationNavigationOptions = {},
): Promise<boolean> {
  if (item.category === "permission_request") {
    const requestId = permissionRequestId(item);
    if (!requestId) return false;
    let pending;
    try {
      pending = await permissionApi.listPendingPermissionGates();
    } catch {
      toast.error("Unable to load pending permission requests");
      return false;
    }
    if (!pending.some((request) => request.request_id === requestId)) {
      return true;
    }
    window.dispatchEvent(new CustomEvent("ralphx:open-permission-dialog", {
      detail: { requestId },
    }));
    options.onClose?.();
    return true;
  }

  const { target } = item;
  if (target.kind === "task" && target.taskId) {
    const applied = await openTaskInAgents(target.taskId, "graph", {
      ...(target.projectId ? { projectId: target.projectId } : {}),
    });
    if (applied) options.onClose?.();
    return applied;
  }
  if (target.kind === "agent_conversation") {
    if (target.projectId && target.conversationId) {
      if (item.category === "plan_approval") {
        navigateToAgentPlan(target.projectId, target.conversationId);
      } else {
        navigateToAgentConversation(target.projectId, target.conversationId);
      }
      options.onClose?.();
      return true;
    } else if (target.setupConversationId) {
      navigateToIdeationSession(target.setupConversationId);
      options.onClose?.();
      return true;
    }
    options.onClose?.();
    return false;
  }
  if (
    target.kind === "automation_run" &&
    target.projectId &&
    target.automationId &&
    target.runId &&
    target.conversationId
  ) {
    const result = await requestAutomationRunOpen(queryClient, {
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
    if (result.applied) {
      options.onClose?.();
    }
    return result.applied;
  } else if (target.kind === "automation_run" && target.automationId) {
    options.onOpenAutomationDetail?.(target.automationId);
    options.onClose?.();
    return true;
  }
  if (target.kind === "project" && target.projectId) {
    useProjectStore.getState().selectProject(target.projectId);
    useUiStore.getState().setCurrentView("agents");
    options.onClose?.();
    return true;
  }
  if (target.kind !== "none") options.onClose?.();
  return false;
}
