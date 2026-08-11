import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { notificationsApi } from "./notifications";

describe("notificationsApi", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("binds durable notification commands with camelCase args and page response shape", async () => {
    vi.mocked(invoke).mockResolvedValue({
      notifications: [{
        id: "notification-1",
        createdAt: "2026-07-10T10:00:00Z",
        projectId: null,
        category: "task_failed",
        severity: "warning",
        title: "Task failed",
        body: null,
        target: { kind: "none" },
        dedupeKey: null,
        readAt: null,
      }],
      cursor: null,
      hasMore: true,
    });

    await expect(notificationsApi.list({ projectId: "project-1", cursor: "cursor-0", limit: 25 })).resolves.toEqual({
      notifications: [{
        id: "notification-1",
        createdAt: "2026-07-10T10:00:00Z",
        projectId: undefined,
        category: "task_failed",
        severity: "warning",
        title: "Task failed",
        body: undefined,
        target: { kind: "none" },
        dedupeKey: undefined,
        readAt: undefined,
      }],
      cursor: undefined,
      hasMore: true,
    });
    expect(invoke).toHaveBeenCalledWith("list_notifications", { projectId: "project-1", cursor: "cursor-0", limit: 25 });
  });

  it("normalizes explicit serde-null attention fields", async () => {
    vi.mocked(invoke).mockResolvedValue([{
      id: "attention:1",
      category: "task_failed",
      title: "Task failed",
      detail: null,
      projectId: null,
      createdAt: null,
      target: { kind: "none" },
    }]);

    await expect(notificationsApi.listAttentionItems()).resolves.toEqual([{
      id: "attention:1",
      category: "task_failed",
      title: "Task failed",
      detail: undefined,
      projectId: undefined,
      createdAt: undefined,
      target: { kind: "none" },
    }]);

    expect(invoke).toHaveBeenCalledWith("list_attention_items", {});
  });

  it("binds read operations and unread count without snake_case invoke arguments", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(3);

    await notificationsApi.markRead("notification-1");
    await notificationsApi.markAllRead("project-1");
    await expect(notificationsApi.getUnreadCount("project-1")).resolves.toBe(3);

    expect(invoke).toHaveBeenNthCalledWith(1, "mark_notification_read", { id: "notification-1" });
    expect(invoke).toHaveBeenNthCalledWith(2, "mark_all_notifications_read", { projectId: "project-1" });
    expect(invoke).toHaveBeenNthCalledWith(3, "get_unread_notification_count", { projectId: "project-1" });
  });

  it("passes the requested count to the dock badge command", async () => {
    vi.mocked(invoke).mockResolvedValue(null);

    await expect(notificationsApi.setDockBadgeCount(10)).resolves.toBeNull();

    expect(invoke).toHaveBeenCalledWith("set_dock_badge_count", { count: 10 });
  });
});
