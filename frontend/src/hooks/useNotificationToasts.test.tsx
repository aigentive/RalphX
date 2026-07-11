import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

import { navigateNotification } from "@/components/notifications/notificationNavigation";
import { notificationsApi } from "@/api/notifications";
import { useUiStore } from "@/stores/uiStore";
import { useNotificationToasts } from "./useNotificationToasts";

const { preferences, subscribers, toastWarning } = vi.hoisted(() => ({
  preferences: { ready: true, focusedToastsEnabled: true, mutedProjectIds: [] as string[] },
  subscribers: new Map<string, (payload: unknown) => void>(),
  toastWarning: vi.fn(),
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({ subscribe: (event: string, callback: (payload: unknown) => void) => { subscribers.set(event, callback); return vi.fn(); } }),
}));
vi.mock("@/components/notifications/notificationNavigation", () => ({ navigateNotification: vi.fn() }));
vi.mock("@/api/notifications", () => ({ notificationsApi: { markRead: vi.fn() } }));
vi.mock("sonner", () => ({ toast: { warning: toastWarning } }));
vi.mock("./useNotificationPreferences", () => ({ useNotificationPreferences: () => preferences }));

const notification = {
  id: "notification-1", createdAt: "2026-07-10T10:00:00Z", projectId: "project-1",
  category: "permission_request", severity: "action_required", title: "Permission requested",
  body: "git push", target: { kind: "none" },
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
    vi.mocked(notificationsApi.markRead).mockResolvedValue(null);
    renderHook(() => useNotificationToasts(), { wrapper });
  });

  afterEach(() => {
    subscribers.clear();
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
});
