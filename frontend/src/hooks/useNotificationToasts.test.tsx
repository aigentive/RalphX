import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactElement, ReactNode } from "react";

import { useAgentArtifactUiStore } from "@/components/agents/agentArtifactUiStore";
import { performNotificationPrimaryAction } from "@/components/notifications/notificationNavigation";
import { notificationsApi } from "@/api/notifications";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import { resetNotificationToastStateForTests, useNotificationToasts } from "./useNotificationToasts";

const { preferences, subscribers, toastWarning, toastDismiss } = vi.hoisted(() => ({
  preferences: { ready: true, focusedToastsEnabled: true, mutedProjectIds: [] as string[] },
  subscribers: new Map<string, (payload: unknown) => void>(),
  toastWarning: vi.fn(),
  toastDismiss: vi.fn(),
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({ subscribe: (event: string, callback: (payload: unknown) => void) => { subscribers.set(event, callback); return vi.fn(); } }),
}));
vi.mock("@/components/notifications/notificationNavigation", () => ({
  performNotificationPrimaryAction: vi.fn(),
}));
vi.mock("@/api/notifications", () => ({ notificationsApi: { markRead: vi.fn() } }));
vi.mock("sonner", () => ({ toast: { warning: toastWarning, dismiss: toastDismiss } }));
vi.mock("./useNotificationPreferences", () => ({ useNotificationPreferences: () => preferences }));

const notification = {
  id: "notification-1", createdAt: "2026-07-10T10:00:00Z", projectId: "project-1",
  category: "permission_request", severity: "action_required", title: "Permission requested",
  body: "git push", target: { kind: "none" },
};

const agentConversationNotification = {
  id: "agent-notification-1",
  createdAt: "2026-07-10T10:00:00Z",
  projectId: "project-1",
  category: "agent_question" as const,
  severity: "action_required" as const,
  title: "Agent has a question",
  body: "Need a decision",
  target: {
    kind: "agent_conversation" as const,
    projectId: "project-1",
    conversationId: "conversation-1",
  },
};

const planReviewNotification = {
  ...agentConversationNotification,
  id: "plan-notification-1",
  category: "plan_approval" as const,
  title: "Plan ready for review",
};

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>;
}

function renderToastContent(callIndex = 0) {
  const content = toastWarning.mock.calls[callIndex]?.[0] as ReactElement;
  return render(content);
}

describe("useNotificationToasts", () => {
  beforeEach(() => {
    Object.defineProperty(document, "hasFocus", { configurable: true, value: () => true });
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    preferences.focusedToastsEnabled = true;
    preferences.ready = true;
    preferences.mutedProjectIds = [];
    useUiStore.setState({ notificationsPanelOpen: false });
    useAgentSessionStore.setState({
      selectedConversationId: null,
      visibleAgentScope: null,
    });
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
    vi.mocked(notificationsApi.markRead).mockResolvedValue(null);
    vi.mocked(performNotificationPrimaryAction).mockResolvedValue(true);
    renderHook(() => useNotificationToasts(), { wrapper });
  });

  afterEach(() => {
    subscribers.clear();
    resetNotificationToastStateForTests();
    vi.clearAllMocks();
    vi.restoreAllMocks();
  });

  it.each([
    { focused: true, severity: "action_required", drawerOpen: false, shouldToast: true },
    { focused: false, severity: "action_required", drawerOpen: false, shouldToast: false },
    { focused: true, severity: "warning", drawerOpen: false, shouldToast: false },
    { focused: true, severity: "info", drawerOpen: false, shouldToast: false },
    { focused: true, severity: "action_required", drawerOpen: true, shouldToast: false },
  ] as const)("gates focused toast ($focused focus, $severity severity, drawer $drawerOpen)", ({ focused, severity, drawerOpen, shouldToast }) => {
    Object.defineProperty(document, "hasFocus", { configurable: true, value: () => focused });
    useUiStore.setState({ notificationsPanelOpen: drawerOpen });

    subscribers.get("notification:created")?.({ ...notification, severity });

    expect(toastWarning).toHaveBeenCalledTimes(shouldToast ? 1 : 0);
  });

  it("never toasts an info notification", () => {
    subscribers.get("notification:created")?.({ ...notification, severity: "info" });
    expect(toastWarning).not.toHaveBeenCalled();
  });

  it("does not toast when focused toasts are disabled in preferences", () => {
    preferences.focusedToastsEnabled = false;
    renderHook(() => useNotificationToasts(), { wrapper });
    subscribers.get("notification:created")?.(notification);
    expect(toastWarning).not.toHaveBeenCalled();
  });

  it("suppresses action-required toasts until notification preferences finish hydrating", () => {
    preferences.ready = false;
    renderHook(() => useNotificationToasts(), { wrapper });

    subscribers.get("notification:created")?.(notification);

    expect(toastWarning).not.toHaveBeenCalled();
  });

  it("suppresses a muted project toast but keeps global notifications eligible", () => {
    preferences.mutedProjectIds = ["project-1"];
    renderHook(() => useNotificationToasts(), { wrapper });

    subscribers.get("notification:created")?.(notification);
    subscribers.get("notification:created")?.({ ...notification, id: "global-1", projectId: undefined });

    expect(toastWarning).toHaveBeenCalledTimes(1);
    expect(toastWarning).toHaveBeenCalledWith(expect.any(Object), expect.any(Object));
  });

  it("renders every durable action-required notification as a persistent custom toast", () => {
    subscribers.get("notification:created")?.(notification);
    const options = toastWarning.mock.calls[0]?.[1] as {
      action?: unknown;
      duration: number;
    };
    const view = renderToastContent();

    expect(options.duration).toBe(Infinity);
    expect(options.action).toBeUndefined();
    expect(view.getByText("Permission requested")).toBeVisible();
    expect(view.getByText("git push")).toBeVisible();
    expect(view.getByRole("button", { name: "Respond" })).toBeEnabled();
    expect(view.getByRole("button", { name: "Dismiss" })).toBeEnabled();
  });

  it("dismisses and marks a non-Agent notification read only after its action succeeds", async () => {
    subscribers.get("notification:created")?.(notification);
    const view = renderToastContent();

    fireEvent.click(view.getByRole("button", { name: "Respond" }));

    await waitFor(() => {
      expect(performNotificationPrimaryAction).toHaveBeenCalledWith(
        notification,
        expect.any(QueryClient),
      );
      expect(toastDismiss).toHaveBeenCalledWith("notification-1");
      expect(notificationsApi.markRead).toHaveBeenCalledWith("notification-1");
    });
  });

  it("logs mark-read failures without making the dismissed toast interactive again", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.mocked(notificationsApi.markRead).mockRejectedValueOnce(new Error("offline"));
    subscribers.get("notification:created")?.(notification);
    const view = renderToastContent();
    fireEvent.click(view.getByRole("button", { name: "Respond" }));
    await waitFor(() => {
      expect(consoleError).toHaveBeenCalledWith(
        "Failed to mark notification read:",
        expect.any(Error),
      );
      expect(toastDismiss).toHaveBeenCalledWith(notification.id);
    });
  });

  it("resumes a paused automation directly from the toast action", async () => {
    const pausedNotification = {
      ...notification,
      id: "automation-paused-1",
      category: "automation_paused",
      title: "Automation paused",
      target: {
        kind: "automation_run",
        projectId: "project-1",
        automationId: "automation-1",
      },
    };
    subscribers.get("notification:created")?.(pausedNotification);
    const view = renderToastContent();
    fireEvent.click(view.getByRole("button", { name: "Resume" }));

    await waitFor(() => {
      expect(performNotificationPrimaryAction).toHaveBeenCalledWith(
        pausedNotification,
        expect.any(QueryClient),
      );
      expect(notificationsApi.markRead).toHaveBeenCalledWith("automation-paused-1");
    });
  });

  it("manual dismissal hides the toast without marking the notification read", () => {
    subscribers.get("notification:created")?.(agentConversationNotification);
    const view = renderToastContent();

    fireEvent.click(view.getByRole("button", { name: "Dismiss" }));

    expect(toastDismiss).toHaveBeenCalledWith(agentConversationNotification.id);
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("manual dismissal cancels a pending Agent acknowledgement", async () => {
    let settleAction: ((result: boolean) => void) | undefined;
    vi.mocked(performNotificationPrimaryAction).mockReturnValue(
      new Promise<boolean>((resolve) => {
        settleAction = resolve;
      }),
    );
    subscribers.get("notification:created")?.(agentConversationNotification);
    const view = renderToastContent();

    fireEvent.click(view.getByRole("button", { name: "Answer" }));
    await waitFor(() => expect(performNotificationPrimaryAction).toHaveBeenCalled());
    fireEvent.click(view.getByRole("button", { name: "Dismiss" }));

    act(() => {
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-1",
        visibleAgentScope: null,
      });
      settleAction?.(true);
    });

    await waitFor(() => expect(view.getByRole("button", { name: "Answer" })).toBeEnabled());
    expect(toastDismiss).toHaveBeenCalledWith(agentConversationNotification.id);
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("manual dismissal cancels a pending non-Agent acknowledgement", async () => {
    let settleAction: ((result: boolean) => void) | undefined;
    vi.mocked(performNotificationPrimaryAction).mockReturnValue(
      new Promise<boolean>((resolve) => {
        settleAction = resolve;
      }),
    );
    subscribers.get("notification:created")?.(notification);
    const view = renderToastContent();

    fireEvent.click(view.getByRole("button", { name: "Respond" }));
    await waitFor(() => expect(performNotificationPrimaryAction).toHaveBeenCalled());
    fireEvent.click(view.getByRole("button", { name: "Dismiss" }));

    act(() => settleAction?.(true));

    await waitFor(() => expect(view.getByRole("button", { name: "Respond" })).toBeEnabled());
    expect(toastDismiss).toHaveBeenCalledWith(notification.id);
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("keeps an Agent conversation toast unread until the target conversation is observed", async () => {
    subscribers.get("notification:created")?.(agentConversationNotification);
    const view = renderToastContent();

    fireEvent.click(view.getByRole("button", { name: "Answer" }));
    await waitFor(() => expect(performNotificationPrimaryAction).toHaveBeenCalled());

    expect(toastDismiss).not.toHaveBeenCalled();
    expect(notificationsApi.markRead).not.toHaveBeenCalled();

    act(() => {
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-1",
        visibleAgentScope: {
          workspaceConversationId: "conversation-1",
          visibleConversationId: "conversation-1",
        },
      });
    });

    await waitFor(() => {
      expect(toastDismiss).toHaveBeenCalledWith(agentConversationNotification.id);
      expect(notificationsApi.markRead).toHaveBeenCalledWith(agentConversationNotification.id);
    });
  });

  it("acknowledges an Agent-targeted permission when its dialog opens", async () => {
    const permissionNotification = {
      ...agentConversationNotification,
      id: "permission-notification-1",
      category: "permission_request" as const,
      dedupeKey: "perm:permission-1",
      title: "Permission needed",
    };
    subscribers.get("notification:created")?.(permissionNotification);
    const view = renderToastContent();

    fireEvent.click(view.getByRole("button", { name: "Respond" }));

    await waitFor(() => {
      expect(performNotificationPrimaryAction).toHaveBeenCalledWith(
        permissionNotification,
        expect.any(QueryClient),
      );
      expect(toastDismiss).toHaveBeenCalledWith(permissionNotification.id);
      expect(notificationsApi.markRead).toHaveBeenCalledWith(permissionNotification.id);
    });
    expect(useAgentSessionStore.getState().visibleAgentScope).toBeNull();
  });

  it("acknowledges and dismisses only the toast for the conversation the user visits", async () => {
    subscribers.get("notification:created")?.(agentConversationNotification);
    subscribers.get("notification:created")?.({
      ...agentConversationNotification,
      id: "agent-notification-2",
      target: {
        ...agentConversationNotification.target,
        conversationId: "conversation-2",
      },
    });

    act(() => {
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-2",
        visibleAgentScope: {
          workspaceConversationId: "conversation-2",
          visibleConversationId: "conversation-2",
        },
      });
    });
    await Promise.resolve();

    expect(toastDismiss).toHaveBeenCalledWith("agent-notification-2");
    expect(toastDismiss).not.toHaveBeenCalledWith(agentConversationNotification.id);
    expect(notificationsApi.markRead).toHaveBeenCalledWith("agent-notification-2");
    expect(notificationsApi.markRead).not.toHaveBeenCalledWith(agentConversationNotification.id);
  });

  it("acknowledges an Agent notification that arrives while its conversation is already selected", () => {
    act(() => {
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-1",
        visibleAgentScope: {
          workspaceConversationId: "conversation-1",
          visibleConversationId: "conversation-1",
        },
      });
    });

    subscribers.get("notification:created")?.(agentConversationNotification);

    expect(toastWarning).not.toHaveBeenCalled();
    expect(notificationsApi.markRead).toHaveBeenCalledWith(agentConversationNotification.id);
  });

  it("does not suppress a workspace toast while a different child conversation is rendered", () => {
    act(() => {
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-1",
        visibleAgentScope: {
          workspaceConversationId: "conversation-1",
          visibleConversationId: "verification-conversation",
        },
      });
    });

    subscribers.get("notification:created")?.(agentConversationNotification);

    expect(toastWarning).toHaveBeenCalledTimes(1);
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("keeps an Agent conversation toast unread when its CTA cannot navigate", async () => {
    vi.mocked(performNotificationPrimaryAction).mockResolvedValue(false);
    subscribers.get("notification:created")?.(agentConversationNotification);
    const view = renderToastContent();
    const action = view.getByRole("button", { name: "Answer" });

    fireEvent.click(action);
    await waitFor(() => expect(performNotificationPrimaryAction).toHaveBeenCalled());
    await waitFor(() => expect(action).toBeEnabled());

    expect(toastDismiss).not.toHaveBeenCalled();
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("disables repeated CTA clicks while an action is pending", async () => {
    let settleAction: ((result: boolean) => void) | undefined;
    vi.mocked(performNotificationPrimaryAction).mockReturnValue(
      new Promise<boolean>((resolve) => {
        settleAction = resolve;
      }),
    );
    subscribers.get("notification:created")?.(notification);
    const view = renderToastContent();
    const action = view.getByRole("button", { name: "Respond" });

    fireEvent.click(action);
    fireEvent.click(action);

    expect(action).toBeDisabled();
    expect(performNotificationPrimaryAction).toHaveBeenCalledTimes(1);

    act(() => settleAction?.(false));
    await waitFor(() => expect(action).toBeEnabled());
    expect(toastDismiss).not.toHaveBeenCalled();
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("suppresses a plan-review toast whenever the exact Agent workspace is visible", () => {
    useAgentArtifactUiStore.getState().setArtifactState("conversation-1", {
      isOpen: true,
      activeTab: "tasks",
      taskMode: "kanban",
      hiddenTabs: ["plan"],
    });
    act(() => {
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-1",
        visibleAgentScope: {
          workspaceConversationId: "conversation-1",
          visibleConversationId: "conversation-1",
        },
      });
    });
    subscribers.get("notification:created")?.(planReviewNotification);

    expect(toastWarning).not.toHaveBeenCalled();
    expect(notificationsApi.markRead).toHaveBeenCalledWith(planReviewNotification.id);
  });

  it("keeps a plan notification for the already-selected conversation visible until its CTA is clicked", async () => {
    act(() => {
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-1",
        visibleAgentScope: null,
      });
      useAgentArtifactUiStore.getState().setArtifactState("conversation-1", {
        isOpen: true,
        activeTab: "tasks",
        taskMode: "graph",
        hiddenTabs: ["plan"],
      });
    });
    vi.mocked(performNotificationPrimaryAction).mockImplementation(async () => {
      useAgentArtifactUiStore.getState().setArtifactState("conversation-1", {
        isOpen: true,
        activeTab: "plan",
        taskMode: "graph",
        hiddenTabs: [],
      });
      useAgentSessionStore.getState().setVisibleAgentScope({
        workspaceConversationId: "conversation-1",
        visibleConversationId: "conversation-1",
      });
      return true;
    });

    subscribers.get("notification:created")?.(planReviewNotification);
    const view = renderToastContent();

    expect(performNotificationPrimaryAction).not.toHaveBeenCalled();
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
    expect(view.getByRole("button", { name: "Review plan" })).toBeEnabled();

    fireEvent.click(view.getByRole("button", { name: "Review plan" }));

    await waitFor(() => {
      expect(performNotificationPrimaryAction).toHaveBeenCalledWith(
        planReviewNotification,
        expect.any(QueryClient),
      );
      expect(notificationsApi.markRead).toHaveBeenCalledWith(planReviewNotification.id);
    });
    expect(toastDismiss).toHaveBeenCalledWith(planReviewNotification.id);
  });

  it("suppresses only the exact focused automation run", () => {
    act(() => {
      useAgentSessionStore.setState({
        visibleAgentScope: {
          workspaceConversationId: "setup-conversation-1",
          visibleConversationId: "run-conversation-1",
          automationRunId: "run-1",
          automationConversationId: "run-conversation-1",
        },
      });
    });
    const automationNotification = {
      ...agentConversationNotification,
      id: "automation-plan-1",
      category: "automation_plan_approval" as const,
      target: {
        kind: "automation_run" as const,
        projectId: "project-1",
        setupConversationId: "setup-conversation-1",
        automationId: "automation-1",
        runId: "run-1",
        conversationId: "run-conversation-1",
      },
    };

    subscribers.get("notification:created")?.(automationNotification);
    subscribers.get("notification:created")?.({
      ...automationNotification,
      id: "automation-plan-2",
      target: { ...automationNotification.target, runId: "run-2" },
    });

    expect(notificationsApi.markRead).toHaveBeenCalledWith("automation-plan-1");
    expect(notificationsApi.markRead).not.toHaveBeenCalledWith("automation-plan-2");
    expect(toastWarning).toHaveBeenCalledTimes(1);
  });

  it("dismisses only the live toast named by a durable notification update", () => {
    subscribers.get("notification:created")?.(agentConversationNotification);
    subscribers.get("notification:created")?.({
      ...agentConversationNotification,
      id: "agent-notification-2",
    });

    subscribers.get("notification:updated")?.({
      ...agentConversationNotification,
      readAt: "2026-07-10T10:01:00Z",
    });

    expect(toastDismiss).toHaveBeenCalledWith(agentConversationNotification.id);
    expect(toastDismiss).not.toHaveBeenCalledWith("agent-notification-2");
  });

  it("dismisses active notification toasts when the drawer opens", () => {
    subscribers.get("notification:created")?.(notification);
    expect(toastWarning).toHaveBeenCalledWith(expect.any(Object), expect.objectContaining({ id: "notification-1" }));

    act(() => { useUiStore.setState({ notificationsPanelOpen: true }); });

    expect(toastDismiss).toHaveBeenCalledWith("notification-1");
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("acknowledges an in-flight Agent CTA after the drawer hides its toast", async () => {
    let settleAction: ((result: boolean) => void) | undefined;
    vi.mocked(performNotificationPrimaryAction).mockReturnValue(
      new Promise<boolean>((resolve) => {
        settleAction = resolve;
      }),
    );
    subscribers.get("notification:created")?.(agentConversationNotification);
    const view = renderToastContent();

    fireEvent.click(view.getByRole("button", { name: "Answer" }));
    await waitFor(() => expect(performNotificationPrimaryAction).toHaveBeenCalled());

    act(() => {
      useUiStore.setState({ notificationsPanelOpen: true });
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-1",
        visibleAgentScope: {
          workspaceConversationId: "conversation-1",
          visibleConversationId: "conversation-1",
        },
      });
      settleAction?.(true);
    });

    await waitFor(() => {
      expect(toastDismiss).toHaveBeenCalledWith(agentConversationNotification.id);
      expect(notificationsApi.markRead).toHaveBeenCalledWith(agentConversationNotification.id);
    });
  });

  it("acknowledges an in-flight non-Agent CTA after the drawer hides its toast", async () => {
    let settleAction: ((result: boolean) => void) | undefined;
    vi.mocked(performNotificationPrimaryAction).mockReturnValue(
      new Promise<boolean>((resolve) => {
        settleAction = resolve;
      }),
    );
    subscribers.get("notification:created")?.(notification);
    const view = renderToastContent();

    fireEvent.click(view.getByRole("button", { name: "Respond" }));
    await waitFor(() => expect(performNotificationPrimaryAction).toHaveBeenCalled());

    act(() => {
      useUiStore.setState({ notificationsPanelOpen: true });
      settleAction?.(true);
    });

    await waitFor(() => {
      expect(notificationsApi.markRead).toHaveBeenCalledWith(notification.id);
    });
  });

  it("does not acknowledge an Agent toast hidden by the drawer without a CTA action", async () => {
    subscribers.get("notification:created")?.(agentConversationNotification);

    act(() => {
      useUiStore.setState({ notificationsPanelOpen: true });
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-1",
        visibleAgentScope: {
          workspaceConversationId: "conversation-1",
          visibleConversationId: "conversation-1",
        },
      });
    });

    await Promise.resolve();
    expect(toastDismiss).toHaveBeenCalledWith(agentConversationNotification.id);
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("does not dismiss a toast that already settled before the drawer opens", () => {
    subscribers.get("notification:created")?.(notification);
    const dismissedOptions = toastWarning.mock.calls[0]?.[1] as {
      onDismiss: () => void;
    };
    dismissedOptions.onDismiss();

    act(() => { useUiStore.setState({ notificationsPanelOpen: true }); });
    expect(toastDismiss).not.toHaveBeenCalled();

    act(() => { useUiStore.setState({ notificationsPanelOpen: false }); });
  });
});
