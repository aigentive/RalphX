import type { QueryClient } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { requestAutomationRunOpen } from "@/components/automations/automationRunNavigation";
import { tasksApi } from "@/api/tasks";
import { navigateToIdeationSession } from "@/lib/navigation";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import type { NotificationCategory, NotificationTarget } from "@/types/notifications";

import { navigateNotification } from "./notificationNavigation";

const { toastError } = vi.hoisted(() => ({ toastError: vi.fn() }));

vi.mock("@/components/automations/automationRunNavigation", () => ({
  requestAutomationRunOpen: vi.fn(),
}));
vi.mock("@/api/tasks", () => ({ tasksApi: { get: vi.fn() } }));
vi.mock("@/lib/navigation", () => ({ navigateToIdeationSession: vi.fn() }));
vi.mock("sonner", () => ({ toast: { error: toastError } }));

const target = {
  kind: "automation_run" as const,
  projectId: "project-1",
  automationId: "automation-1",
  runId: "run-1",
  conversationId: "run-conversation-1",
  setupConversationId: "setup-conversation-1",
};

describe("navigateNotification", () => {
  const navigateToTask = vi.fn();
  const setCurrentView = vi.fn();
  const selectProject = vi.fn();
  const setState = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(tasksApi.get).mockResolvedValue({ id: "task-1" } as never);
    vi.spyOn(useUiStore, "getState").mockReturnValue({ navigateToTask, setCurrentView, viewByProject: {}, selectedTaskByProject: {} } as ReturnType<typeof useUiStore.getState>);
    vi.spyOn(useUiStore, "setState").mockImplementation(setState);
    vi.spyOn(useProjectStore, "getState").mockReturnValue({ activeProjectId: "project-1", selectProject } as ReturnType<typeof useProjectStore.getState>);
  });

  afterEach(() => vi.restoreAllMocks());

  it.each([
    ["automation_plan_approval", "plan"],
    ["automation_run_failed", "automation"],
    ["automation_run_completed", "pr"],
  ] as const)("maps %s to the %s automation tab intent", (category, tabHint) => {
    navigateNotification(
      { id: "notification-1", category, target },
      {} as QueryClient,
    );

    expect(requestAutomationRunOpen).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ setupConversationId: "setup-conversation-1" }),
      expect.objectContaining({ tabHint }),
    );
  });

  it("keeps a supplied automation detail callback when opening a complete run", () => {
    const onClose = vi.fn();
    const onOpenAutomationDetail = vi.fn();

    navigateNotification(
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

  it("opens the selected permission request and closes the notification surface", () => {
    const onClose = vi.fn();
    const listener = vi.fn();
    window.addEventListener("ralphx:open-permission-dialog", listener);

    navigateNotification(
      { id: "perm:request-1", category: "permission_request", target: { kind: "none" } },
      {} as QueryClient,
      { onClose },
    );

    expect(listener).toHaveBeenCalledWith(expect.objectContaining({ detail: { requestId: "request-1" } }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(navigateToTask).not.toHaveBeenCalled();
    window.removeEventListener("ralphx:open-permission-dialog", listener);
  });

  it("keeps same-project task routing on the fast path", async () => {
    const onClose = vi.fn();
    await navigateNotification(
      { id: "task-1", category: "task_failed", target: { kind: "task", taskId: "task-1", projectId: "project-1" } },
      {} as QueryClient,
      { onClose },
    );

    expect(tasksApi.get).toHaveBeenCalledWith("task-1");
    expect(navigateToTask).toHaveBeenCalledWith("task-1");
    expect(selectProject).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("switches projects before restoring a cross-project task target", async () => {
    const onClose = vi.fn();
    await navigateNotification(
      { id: "task-2", category: "task_failed", target: { kind: "task", taskId: "task-2", projectId: "project-2" } },
      {} as QueryClient,
      { onClose },
    );

    expect(tasksApi.get).toHaveBeenCalledWith("task-2");
    expect(setState).toHaveBeenCalledWith(expect.objectContaining({
      viewByProject: { "project-2": "kanban" },
      selectedTaskByProject: { "project-2": "task-2" },
    }));
    expect(selectProject).toHaveBeenCalledWith("project-2");
    expect(navigateToTask).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps the drawer open when a task target no longer exists", async () => {
    const onClose = vi.fn();
    vi.mocked(tasksApi.get).mockRejectedValueOnce(new Error("not found"));

    await navigateNotification(
      { id: "task-missing", category: "task_failed", target: { kind: "task", taskId: "task-missing", projectId: "project-2" } },
      {} as QueryClient,
      { onClose },
    );

    expect(toastError).toHaveBeenCalledWith("This task no longer exists.");
    expect(selectProject).not.toHaveBeenCalled();
    expect(navigateToTask).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes a malformed task target without fetching or selecting a task", async () => {
    const onClose = vi.fn();

    await navigateNotification(
      { id: "task-missing-id", category: "task_failed", target: { kind: "task" } },
      {} as QueryClient,
      { onClose },
    );

    expect(tasksApi.get).not.toHaveBeenCalled();
    expect(navigateToTask).not.toHaveBeenCalled();
    expect(selectProject).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("opens either agent conversation target and ignores a target with neither conversation id", () => {
    const onClose = vi.fn();
    navigateNotification(
      { id: "conversation-1", category: "agent_waiting", target: { kind: "agent_conversation", setupConversationId: "setup-1" } },
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

  it("selects a project and returns to kanban only when a project id exists", () => {
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
    expect(setCurrentView).toHaveBeenCalledWith("kanban");
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("leaves a none target in place without routing or closing", () => {
    const onClose = vi.fn();
    const item = { id: "info-1", category: "info" as NotificationCategory, target: { kind: "none" } satisfies NotificationTarget };

    navigateNotification(item, {} as QueryClient, { onClose });

    expect(navigateToTask).not.toHaveBeenCalled();
    expect(selectProject).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });
});
