import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

import { notificationsApi } from "@/api/notifications";
import { navigateNotification } from "@/components/notifications/notificationNavigation";

import { useNotificationEvents } from "./useNotificationEvents";
import { attentionKeys } from "./useAttentionItems";
import { notificationKeys } from "./useNotificationHistory";

const subscribers = new Map<string, (payload: unknown) => void>();
vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({ subscribe: (event: string, callback: (payload: unknown) => void) => { subscribers.set(event, callback); return vi.fn(); } }),
}));
vi.mock("@/components/notifications/notificationNavigation", () => ({ navigateNotification: vi.fn() }));
vi.mock("@/api/notifications", () => ({ notificationsApi: { markRead: vi.fn() } }));

describe("useNotificationEvents", () => {
  afterEach(() => { subscribers.clear(); vi.clearAllMocks(); vi.restoreAllMocks(); });

  it("invalidates attention for every emitted Tier 1 source event", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
    renderHook(() => useNotificationEvents(), { wrapper });

    expect([...subscribers.keys()]).toEqual([
      "review:update", "task:status_changed", "permission:request", "permission:expired",
      "permission:resolved",
      "agent:ask_user_question", "agent:question_resolved", "automation:updated",
      "automation:run:updated", "plan_artifact:created", "plan_artifact:approved",
      "pr_review_artifact:created", "pr_review_artifact:updated",
      "notification:created", "notification:updated", "notification:desktop_activated",
    ]);
    subscribers.get("permission:request")?.();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: attentionKeys.all });

    subscribers.get("permission:resolved")?.();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: attentionKeys.all });

    subscribers.get("notification:created")?.();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: attentionKeys.all });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: notificationKeys.all });

    invalidate.mockClear();
    subscribers.get("notification:updated")?.();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: attentionKeys.all });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: notificationKeys.all });
  });

  it("opens the exact project conversation carried by a desktop activation", async () => {
    const queryClient = new QueryClient();
    vi.mocked(navigateNotification).mockResolvedValue(true);
    vi.mocked(notificationsApi.markRead).mockResolvedValue(null);
    const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
    renderHook(() => useNotificationEvents(), { wrapper });
    const notification = {
      id: "notification-1",
      createdAt: "2026-07-16T15:00:00Z",
      projectId: "project-2",
      category: "agent_question",
      severity: "action_required",
      title: "Agent has a question",
      body: "Choose an implementation direction",
      target: {
        kind: "agent_conversation",
        projectId: "project-2",
        conversationId: "conversation-2",
      },
    };

    subscribers.get("notification:desktop_activated")?.(notification);
    await Promise.resolve();
    await Promise.resolve();

    expect(navigateNotification).toHaveBeenCalledWith(notification, queryClient);
    expect(notificationsApi.markRead).toHaveBeenCalledWith("notification-1");
  });

  it("keeps a desktop notification unread when its target cannot be opened", async () => {
    vi.mocked(navigateNotification).mockResolvedValue(false);
    const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>;
    renderHook(() => useNotificationEvents(), { wrapper });

    subscribers.get("notification:desktop_activated")?.({
      id: "notification-stale",
      createdAt: "2026-07-16T15:00:00Z",
      category: "agent_question",
      severity: "action_required",
      title: "Agent has a question",
      target: { kind: "agent_conversation" },
    });
    await Promise.resolve();
    await Promise.resolve();

    expect(navigateNotification).toHaveBeenCalledOnce();
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });

  it("still refreshes notification history when marking an opened desktop notification read fails", async () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    vi.mocked(navigateNotification).mockResolvedValue(true);
    vi.mocked(notificationsApi.markRead).mockRejectedValue(new Error("offline"));
    const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
    renderHook(() => useNotificationEvents(), { wrapper });

    subscribers.get("notification:desktop_activated")?.({
      id: "notification-retry",
      createdAt: "2026-07-16T15:00:00Z",
      category: "agent_question",
      severity: "action_required",
      title: "Agent has a question",
      target: { kind: "agent_conversation" },
    });
    await Promise.resolve();
    await Promise.resolve();

    expect(notificationsApi.markRead).toHaveBeenCalledWith("notification-retry");
    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({ queryKey: notificationKeys.all });
    });
  });

  it("ignores malformed desktop activation payloads", () => {
    const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>;
    renderHook(() => useNotificationEvents(), { wrapper });

    subscribers.get("notification:desktop_activated")?.({
      id: "notification-malformed",
      target: { kind: "agent_conversation" },
    });

    expect(navigateNotification).not.toHaveBeenCalled();
    expect(notificationsApi.markRead).not.toHaveBeenCalled();
  });
});
