import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

import { navigateNotification } from "@/components/notifications/notificationNavigation";
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
vi.mock("@/components/notifications/notificationNavigation", () => ({ navigateNotification: vi.fn() }));
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

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>;
}

describe("useNotificationToasts", () => {
  beforeEach(() => {
    Object.defineProperty(document, "hasFocus", { configurable: true, value: () => true });
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    preferences.focusedToastsEnabled = true;
    preferences.ready = true;
    preferences.mutedProjectIds = [];
    useUiStore.setState({ notificationsPanelOpen: false });
    useAgentSessionStore.setState({ selectedConversationId: null });
    vi.mocked(notificationsApi.markRead).mockResolvedValue(null);
    vi.mocked(navigateNotification).mockResolvedValue(true);
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
    expect(toastWarning).toHaveBeenCalledWith("Permission requested", expect.any(Object));
  });

  it("navigates and marks the notification read from the toast action", async () => {
    subscribers.get("notification:created")?.(notification);
    const options = toastWarning.mock.calls[0]?.[1] as { action: { onClick: () => void }; duration: number };
    expect(options.duration).toBeLessThanOrEqual(5 * 60_000);

    options.action.onClick();
    await Promise.resolve();
    expect(navigateNotification).toHaveBeenCalledWith(notification, expect.any(QueryClient));
    expect(notificationsApi.markRead).toHaveBeenCalledWith("notification-1");
  });

  it("keeps Agent conversation toasts open until manually dismissed", () => {
    subscribers.get("notification:created")?.(agentConversationNotification);

    const options = toastWarning.mock.calls[0]?.[1] as {
      closeButton: boolean;
      closeButtonAriaLabel: string;
      duration: number;
      onDismiss: () => void;
    };
    expect(options.duration).toBe(Infinity);
    expect(options.closeButton).toBe(true);
    expect(options.closeButtonAriaLabel).toBe("Dismiss notification");

    options.onDismiss();

    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("acknowledges and dismisses an Agent conversation toast after its CTA navigates", async () => {
    subscribers.get("notification:created")?.(agentConversationNotification);
    const options = toastWarning.mock.calls[0]?.[1] as {
      action: { onClick: () => void };
    };

    options.action.onClick();
    await Promise.resolve();
    await Promise.resolve();

    expect(navigateNotification).toHaveBeenCalledWith(
      agentConversationNotification,
      expect.any(QueryClient),
    );
    expect(toastDismiss).toHaveBeenCalledWith(agentConversationNotification.id);
    expect(notificationsApi.markRead).toHaveBeenCalledWith(agentConversationNotification.id);
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
      useAgentSessionStore.setState({ selectedConversationId: "conversation-2" });
    });
    await Promise.resolve();

    expect(toastDismiss).toHaveBeenCalledWith("agent-notification-2");
    expect(toastDismiss).not.toHaveBeenCalledWith(agentConversationNotification.id);
    expect(notificationsApi.markRead).toHaveBeenCalledWith("agent-notification-2");
    expect(notificationsApi.markRead).not.toHaveBeenCalledWith(agentConversationNotification.id);
  });

  it("acknowledges an Agent notification that arrives while its conversation is already selected", () => {
    act(() => {
      useAgentSessionStore.setState({ selectedConversationId: "conversation-1" });
    });

    subscribers.get("notification:created")?.(agentConversationNotification);

    expect(toastWarning).not.toHaveBeenCalled();
    expect(notificationsApi.markRead).toHaveBeenCalledWith(agentConversationNotification.id);
  });

  it("keeps an Agent conversation toast unread when its CTA cannot navigate", async () => {
    vi.mocked(navigateNotification).mockResolvedValue(false);
    subscribers.get("notification:created")?.(agentConversationNotification);
    const options = toastWarning.mock.calls[0]?.[1] as {
      action: { onClick: () => void };
    };

    options.action.onClick();
    await Promise.resolve();
    await Promise.resolve();

    expect(toastDismiss).not.toHaveBeenCalled();
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("dismisses active notification toasts when the drawer opens", () => {
    subscribers.get("notification:created")?.(notification);
    expect(toastWarning).toHaveBeenCalledWith("Permission requested", expect.objectContaining({ id: "notification-1" }));

    act(() => { useUiStore.setState({ notificationsPanelOpen: true }); });

    expect(toastDismiss).toHaveBeenCalledWith("notification-1");
  });

  it("does not dismiss a toast that already settled before the drawer opens", () => {
    subscribers.get("notification:created")?.(notification);
    const dismissedOptions = toastWarning.mock.calls[0]?.[1] as {
      onDismiss: () => void;
      onAutoClose: () => void;
    };
    dismissedOptions.onDismiss();

    act(() => { useUiStore.setState({ notificationsPanelOpen: true }); });
    expect(toastDismiss).not.toHaveBeenCalled();

    act(() => { useUiStore.setState({ notificationsPanelOpen: false }); });
    subscribers.get("notification:created")?.({ ...notification, id: "notification-auto-closed" });
    const autoClosedOptions = toastWarning.mock.calls[1]?.[1] as {
      onAutoClose: () => void;
    };
    autoClosedOptions.onAutoClose();

    act(() => { useUiStore.setState({ notificationsPanelOpen: true }); });
    expect(toastDismiss).not.toHaveBeenCalled();
  });
});
