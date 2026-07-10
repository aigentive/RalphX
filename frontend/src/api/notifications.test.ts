import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { notificationsApi } from "./notifications";

describe("notificationsApi", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("binds durable notification commands with camelCase args and page response shape", async () => {
    vi.mocked(invoke).mockResolvedValue({ notifications: [], cursor: "cursor-1", hasMore: true });

    await expect(notificationsApi.list({ projectId: "project-1", cursor: "cursor-0", limit: 25 })).resolves.toEqual({
      notifications: [], cursor: "cursor-1", hasMore: true,
    });
    expect(invoke).toHaveBeenCalledWith("list_notifications", { projectId: "project-1", cursor: "cursor-0", limit: 25 });
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
});
