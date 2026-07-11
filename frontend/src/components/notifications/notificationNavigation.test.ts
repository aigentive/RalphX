import type { QueryClient } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { requestAutomationRunOpen } from "@/components/automations/automationRunNavigation";
import { navigateToIdeationSession } from "@/lib/navigation";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import type { NotificationCategory, NotificationTarget } from "@/types/notifications";

import { navigateNotification } from "./notificationNavigation";

vi.mock("@/components/automations/automationRunNavigation", () => ({
  requestAutomationRunOpen: vi.fn(),
}));
vi.mock("@/lib/navigation", () => ({ navigateToIdeationSession: vi.fn() }));

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

  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(useUiStore, "getState").mockReturnValue({ navigateToTask, setCurrentView } as ReturnType<typeof useUiStore.getState>);
    vi.spyOn(useProjectStore, "getState").mockReturnValue({ selectProject } as ReturnType<typeof useProjectStore.getState>);
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

  it("routes a task target when it has an id and still closes incomplete task targets", () => {
    const onClose = vi.fn();
    navigateNotification(
      { id: "task-1", category: "task_failed", target: { kind: "task", taskId: "task-1" } },
      {} as QueryClient,
      { onClose },
    );
    navigateNotification(
      { id: "task-missing", category: "task_failed", target: { kind: "task" } },
      {} as QueryClient,
      { onClose },
    );

    expect(navigateToTask).toHaveBeenCalledTimes(1);
    expect(navigateToTask).toHaveBeenCalledWith("task-1");
    expect(onClose).toHaveBeenCalledTimes(2);
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
