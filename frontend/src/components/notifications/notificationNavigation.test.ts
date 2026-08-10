import { QueryClient } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { requestAutomationRunOpen } from "@/components/automations/automationRunNavigation";
import { automationsApi } from "@/api/automations";
import { permissionApi } from "@/api/permission";
import {
  navigateToAgentConversation,
  navigateToAgentPlan,
  navigateToIdeationSession,
  openTaskInAgents,
} from "@/lib/navigation";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import { AGENT_CONTROL_DISABLED_HINT } from "@/lib/remote/agent-gate";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import type { NotificationCategory, NotificationTarget } from "@/types/notifications";

import {
  navigateNotification,
  performNotificationPrimaryAction,
} from "./notificationNavigation";

const { toastError, toastSuccess } = vi.hoisted(() => ({
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
}));

vi.mock("@/components/automations/automationRunNavigation", () => ({
  requestAutomationRunOpen: vi.fn(),
}));
vi.mock("@/api/automations", () => ({ automationsApi: { resume: vi.fn() } }));
vi.mock("@/api/permission", () => ({ permissionApi: { listPendingPermissionGates: vi.fn() } }));
vi.mock("@/lib/navigation", () => ({
  navigateToAgentConversation: vi.fn(),
  navigateToAgentPlan: vi.fn(),
  navigateToIdeationSession: vi.fn(),
  openTaskInAgents: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: { error: toastError, success: toastSuccess } }));

const target = {
  kind: "automation_run" as const,
  projectId: "project-1",
  automationId: "automation-1",
  runId: "run-1",
  conversationId: "run-conversation-1",
  setupConversationId: "setup-conversation-1",
};

describe("navigateNotification", () => {
  const setCurrentView = vi.fn();
  const selectProject = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(openTaskInAgents).mockResolvedValue(true);
    vi.mocked(permissionApi.listPendingPermissionGates).mockResolvedValue([]);
    vi.mocked(requestAutomationRunOpen).mockResolvedValue({ applied: true });
    vi.spyOn(useUiStore, "getState").mockReturnValue({ setCurrentView } as ReturnType<typeof useUiStore.getState>);
    vi.spyOn(useProjectStore, "getState").mockReturnValue({ activeProjectId: "project-1", selectProject } as ReturnType<typeof useProjectStore.getState>);
  });

  afterEach(() => vi.restoreAllMocks());

  it.each([
    ["automation_plan_approval", "plan"],
    ["automation_run_failed", "automation"],
    ["automation_run_completed", "pr"],
  ] as const)("maps %s to the %s automation tab intent", async (category, tabHint) => {
    await navigateNotification(
      { id: "notification-1", category, target },
      {} as QueryClient,
    );

    expect(requestAutomationRunOpen).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ setupConversationId: "setup-conversation-1" }),
      expect.objectContaining({ tabHint }),
    );
  });

  it("resumes a paused automation as the notification primary action", async () => {
    vi.mocked(automationsApi.resume).mockResolvedValue({} as never);
    const queryClient = new QueryClient();
    const onClose = vi.fn();

    const acted = await performNotificationPrimaryAction(
      {
        id: "automation-paused-1",
        category: "automation_paused",
        target: { ...target, runId: undefined, conversationId: undefined },
      },
      queryClient,
      { onClose },
    );

    expect(acted).toBe(true);
    expect(automationsApi.resume).toHaveBeenCalledWith("automation-1");
    expect(toastSuccess).toHaveBeenCalledWith("Automation resumed");
    expect(onClose).toHaveBeenCalledOnce();
    expect(requestAutomationRunOpen).not.toHaveBeenCalled();
  });

  it("keeps a stale paused notification open when resume is rejected", async () => {
    vi.mocked(automationsApi.resume).mockRejectedValue(new Error("not paused"));
    const onClose = vi.fn();

    const acted = await performNotificationPrimaryAction(
      {
        id: "automation-paused-stale",
        category: "automation_paused",
        target: { ...target, runId: undefined, conversationId: undefined },
      },
      new QueryClient(),
      { onClose },
    );

    expect(acted).toBe(false);
    expect(toastError).toHaveBeenCalledWith("Automation is no longer resumable");
    expect(onClose).not.toHaveBeenCalled();
    expect(requestAutomationRunOpen).not.toHaveBeenCalled();
  });

  it("reports the remote gate hint instead of a stale lifecycle rejection", async () => {
    vi.mocked(automationsApi.resume).mockRejectedValue(new RemoteTransportError({
      code: "REMOTE_FORBIDDEN",
      message: "scope denied",
      environmentId: "remote-1",
      cmd: "resume_automation",
    }));

    await performNotificationPrimaryAction(
      {
        id: "automation-paused-remote",
        category: "automation_paused",
        target: { ...target, runId: undefined, conversationId: undefined },
      },
      new QueryClient(),
    );

    expect(toastError).toHaveBeenCalledWith(AGENT_CONTROL_DISABLED_HINT);
    expect(toastError).not.toHaveBeenCalledWith("Automation is no longer resumable");
  });

  it("keeps a supplied automation detail callback when opening a complete run", async () => {
    const onClose = vi.fn();
    const onOpenAutomationDetail = vi.fn();

    await navigateNotification(
      { id: "automation-complete", category: "automation_run_failed", target },
      {} as QueryClient,
      { onClose, onOpenAutomationDetail },
    );

    expect(requestAutomationRunOpen).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ runId: "run-1" }),
      expect.objectContaining({ onOpenAutomationDetail, tabHint: "automation" }),
    );
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("opens a pending durable permission request from its correlation key", async () => {
    const onClose = vi.fn();
    const listener = vi.fn();
    window.addEventListener("ralphx:open-permission-dialog", listener);
    vi.mocked(permissionApi.listPendingPermissionGates).mockResolvedValue([
      { request_id: "request-1", tool_name: "Bash", tool_input: {} },
    ]);

    const navigated = await navigateNotification(
      {
        id: "durable-notification-uuid",
        dedupeKey: "perm:request-1",
        category: "permission_request",
        target: { kind: "none" },
      },
      {} as QueryClient,
      { onClose },
    );

    expect(navigated).toBe(true);
    expect(listener).toHaveBeenCalledWith(expect.objectContaining({ detail: { requestId: "request-1" } }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(permissionApi.listPendingPermissionGates).toHaveBeenCalledOnce();
    window.removeEventListener("ralphx:open-permission-dialog", listener);
  });

  it("settles a stale permission notification but fails closed on request-state read errors", async () => {
    const item = {
      id: "durable-notification-uuid",
      dedupeKey: "perm:request-1",
      category: "permission_request" as const,
      target: { kind: "none" as const },
    };

    await expect(navigateNotification(item, {} as QueryClient)).resolves.toBe(true);
    vi.mocked(permissionApi.listPendingPermissionGates).mockRejectedValueOnce(new Error("offline"));
    await expect(navigateNotification(item, {} as QueryClient)).resolves.toBe(false);
    expect(toastError).toHaveBeenCalledWith("Unable to load pending permission requests");
  });

  it("routes task notifications through the shared Agents owner", async () => {
    const onClose = vi.fn();
    await navigateNotification(
      { id: "task-1", category: "task_failed", target: { kind: "task", taskId: "task-1", projectId: "project-1" } },
      {} as QueryClient,
      { onClose },
    );

    expect(openTaskInAgents).toHaveBeenCalledWith("task-1", "graph", {
      projectId: "project-1",
    });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("passes cross-project task ownership hints to the shared Agents owner", async () => {
    const onClose = vi.fn();
    await navigateNotification(
      { id: "task-2", category: "task_failed", target: { kind: "task", taskId: "task-2", projectId: "project-2" } },
      {} as QueryClient,
      { onClose },
    );

    expect(openTaskInAgents).toHaveBeenCalledWith("task-2", "graph", {
      projectId: "project-2",
    });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps the drawer open when Agents ownership cannot be resolved", async () => {
    const onClose = vi.fn();
    vi.mocked(openTaskInAgents).mockResolvedValueOnce(false);

    await navigateNotification(
      { id: "task-missing", category: "task_failed", target: { kind: "task", taskId: "task-missing", projectId: "project-2" } },
      {} as QueryClient,
      { onClose },
    );

    expect(openTaskInAgents).toHaveBeenCalledWith("task-missing", "graph", {
      projectId: "project-2",
    });
    expect(selectProject).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes a malformed task target without fetching or selecting a task", async () => {
    const onClose = vi.fn();

    await navigateNotification(
      { id: "task-missing-id", category: "task_failed", target: { kind: "task" } },
      {} as QueryClient,
      { onClose },
    );

    expect(openTaskInAgents).not.toHaveBeenCalled();
    expect(selectProject).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("routes an Agent conversation target to its exact project conversation", () => {
    const onClose = vi.fn();
    navigateNotification(
      {
        id: "conversation-1",
        category: "agent_question",
        target: {
          kind: "agent_conversation",
          projectId: "project-2",
          conversationId: "conversation-1",
        },
      },
      {} as QueryClient,
      { onClose },
    );

    expect(navigateToAgentConversation).toHaveBeenCalledWith(
      "project-2",
      "conversation-1",
    );
    expect(navigateToIdeationSession).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("opens artifact plan approval in the conversation Plan artifact", () => {
    const onClose = vi.fn();

    navigateNotification(
      {
        id: "plan-approval-1",
        category: "plan_approval",
        target: {
          kind: "agent_conversation",
          projectId: "project-2",
          conversationId: "conversation-1",
        },
      },
      {} as QueryClient,
      { onClose },
    );

    expect(navigateToAgentPlan).toHaveBeenCalledWith("project-2", "conversation-1");
    expect(navigateToAgentConversation).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("reports automation navigation success only after exact run focus applies", async () => {
    vi.mocked(requestAutomationRunOpen).mockResolvedValueOnce({
      applied: false,
      reason: "stale",
    });

    const result = await navigateNotification(
      { id: "automation-1", category: "automation_plan_approval", target },
      {} as QueryClient,
    );

    expect(result).toBe(false);
  });

  it("keeps the legacy setup-conversation fallback and closes malformed targets", () => {
    const onClose = vi.fn();
    navigateNotification(
      { id: "conversation-legacy", category: "agent_waiting", target: { kind: "agent_conversation", setupConversationId: "setup-1" } },
      {} as QueryClient,
      { onClose },
    );
    navigateNotification(
      { id: "conversation-missing", category: "agent_waiting", target: { kind: "agent_conversation" } },
      {} as QueryClient,
      { onClose },
    );

    expect(navigateToIdeationSession).toHaveBeenCalledWith("setup-1");
    expect(navigateToIdeationSession).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("opens a partial automation target in its detail surface and closes it", () => {
    const onClose = vi.fn();
    const onOpenAutomationDetail = vi.fn();

    navigateNotification(
      { id: "automation-1", category: "automation_paused", target: { kind: "automation_run", automationId: "automation-1" } },
      {} as QueryClient,
      { onClose, onOpenAutomationDetail },
    );

    expect(requestAutomationRunOpen).not.toHaveBeenCalled();
    expect(onOpenAutomationDetail).toHaveBeenCalledWith("automation-1");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("selects a project and returns to Agents only when a project id exists", () => {
    const onClose = vi.fn();
    navigateNotification(
      { id: "project-1", category: "info", target: { kind: "project", projectId: "project-1" } },
      {} as QueryClient,
      { onClose },
    );
    navigateNotification(
      { id: "project-missing", category: "info", target: { kind: "project" } },
      {} as QueryClient,
      { onClose },
    );

    expect(selectProject).toHaveBeenCalledTimes(1);
    expect(selectProject).toHaveBeenCalledWith("project-1");
    expect(setCurrentView).toHaveBeenCalledWith("agents");
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("leaves a none target in place without routing or closing", () => {
    const onClose = vi.fn();
    const item = { id: "info-1", category: "info" as NotificationCategory, target: { kind: "none" } satisfies NotificationTarget };

    navigateNotification(item, {} as QueryClient, { onClose });

    expect(selectProject).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});
