import { afterAll, afterEach, describe, expect, it, vi } from "vitest";

import { invoke } from "./tauri-api-core";

const notificationSettings = {
  desktop_enabled: true,
  desktop_only_when_unfocused: true,
  focused_toasts_enabled: true,
  desktop_agent_requests_enabled: true,
  desktop_agent_waiting_enabled: true,
  desktop_reviews_enabled: true,
  desktop_task_failures_enabled: true,
  desktop_automation_approvals_enabled: true,
  desktop_automation_run_completions_enabled: false,
  desktop_git_github_enabled: true,
  muted_project_ids: [],
};

describe("notification command mocks", () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);

  afterEach(() => {
    warn.mockClear();
  });

  afterAll(() => {
    warn.mockRestore();
  });

  it("returns the seeded review attention item and respects project filtering", async () => {
    const allItems = await invoke<Array<{ id: string; projectId: string | null; target: { taskId?: string } }>>(
      "list_attention_items",
      {},
    );
    const projectItems = await invoke<Array<{ id: string }>>(
      "list_attention_items",
      { projectId: "project-mock-1" },
    );
    const otherProjectItems = await invoke<Array<{ id: string }>>(
      "list_attention_items",
      { projectId: "project-other" },
    );

    expect(allItems).toEqual(expect.arrayContaining([
      expect.objectContaining({
        category: "review_needed",
        target: { kind: "task", projectId: "project-mock-1", taskId: "task-mock-6" },
      }),
      expect.objectContaining({ category: "permission_request" }),
      expect.objectContaining({ category: "automation_plan_approval" }),
    ]));
    expect(projectItems).toHaveLength(3);
    expect(otherProjectItems).toEqual([]);
    expect(warn).not.toHaveBeenCalled();
  });

  it("handles notification history, read, badge, and settings commands", async () => {
    const page = await invoke<{ notifications: Array<{ id: string; readAt: string | null }>; cursor: string | null; hasMore: boolean }>(
      "list_notifications",
      {},
    );
    const unreadCount = await invoke<number>("get_unread_notification_count", {});
    const settings = await invoke<typeof notificationSettings>("get_notification_settings", {});

    await expect(invoke("mark_notification_read", { id: page.notifications[0]?.id })).resolves.toBeNull();
    await expect(invoke("mark_all_notifications_read", { projectId: "project-mock-1" })).resolves.toBeNull();
    await expect(invoke("set_dock_badge_count", { count: unreadCount })).resolves.toBeNull();
    await expect(invoke("update_notification_settings", { input: { focusedToastsEnabled: false } })).resolves.toEqual(notificationSettings);

    expect(page).toMatchObject({ cursor: null, hasMore: false });
    expect(page.notifications).toEqual(expect.arrayContaining([
      expect.objectContaining({ readAt: null }),
      expect.objectContaining({ readAt: expect.any(String) }),
    ]));
    expect(unreadCount).toBe(2);
    expect(settings).toEqual(notificationSettings);
    expect(warn).not.toHaveBeenCalled();
  });
});
