import { afterAll, afterEach, describe, expect, it, vi } from "vitest";

import { getStore, resetStore, seedMockNotifications } from "@/api-mock/store";

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
    resetStore();
  });

  afterAll(() => {
    warn.mockRestore();
  });

  it("derives attention items from actionable seeded tasks and respects project filtering", async () => {
    const store = getStore();
    const seededStates = [
      ["task-mock-1", "review_passed", null, "review_needed"],
      ["task-mock-2", "escalated", null, "review_escalated"],
      ["task-mock-3", "qa_failed", null, "qa_failed"],
      ["task-mock-4", "merge_conflict", null, "merge_conflict"],
      ["task-mock-5", "merge_incomplete", null, "merge_incomplete"],
      ["task-mock-6", "failed", null, "task_failed"],
      ["task-mock-7", "blocked", "human:Need approval", "task_blocked"],
    ] as const;
    seededStates.forEach(([id, internalStatus, blockedReason]) => {
      const task = store.tasks.get(id);
      if (!task) throw new Error(`Expected ${id} to exist`);
      store.tasks.set(id, { ...task, internalStatus, blockedReason });
    });

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

    expect(allItems).toEqual(seededStates.map(([taskId, , , category]) => expect.objectContaining({
      id: `task:${taskId}:${category}`,
      category,
      target: { kind: "task", projectId: "project-mock-1", taskId },
    })));
    expect(projectItems).toHaveLength(seededStates.length);
    expect(otherProjectItems).toEqual([]);
    expect(warn).not.toHaveBeenCalled();
  });

  it("maps only review_passed, not approved, to review-needed attention", async () => {
    const store = getStore();
    const approvedTask = store.tasks.get("task-mock-6");
    const reviewPassedTask = store.tasks.get("task-mock-7");
    if (!approvedTask || !reviewPassedTask) throw new Error("Expected review attention fixtures");

    store.tasks.set(approvedTask.id, { ...approvedTask, internalStatus: "approved" });
    store.tasks.set(reviewPassedTask.id, { ...reviewPassedTask, internalStatus: "review_passed" });

    const items = await invoke<Array<{ category: string; target: { taskId?: string } }>>(
      "list_attention_items",
      {},
    );

    expect(items).not.toContainEqual(expect.objectContaining({ target: expect.objectContaining({ taskId: approvedTask.id }) }));
    expect(items).toContainEqual(expect.objectContaining({
      category: "review_needed",
      target: expect.objectContaining({ taskId: reviewPassedTask.id }),
    }));
  });

  it("defaults notification history and unread counts to empty", async () => {
    await expect(invoke("list_attention_items", {})).resolves.toEqual([]);
    await expect(invoke("list_notifications", {})).resolves.toEqual({
      notifications: [], cursor: null, hasMore: false,
    });
    await expect(invoke("get_unread_notification_count", {})).resolves.toBe(0);
  });

  it("reads seeded notification history, tracks unread rows, and respects project filtering", async () => {
    seedMockNotifications([
      {
        id: "notification-mock-git-auth",
        createdAt: "2026-07-11T10:00:00.000Z",
        projectId: null,
        category: "git_auth_preflight",
        severity: "warning",
        title: "Git authentication needs attention",
        body: "1 project blocked by Git or GitHub authentication",
        target: { kind: "none" },
        dedupeKey: "git-auth-preflight",
        readAt: null,
      },
      {
        id: "notification-mock-read",
        createdAt: "2026-07-11T09:00:00.000Z",
        projectId: "project-mock-1",
        category: "info",
        severity: "info",
        title: "Mock notification",
        body: null,
        target: { kind: "none" },
        dedupeKey: null,
        readAt: "2026-07-11T09:05:00.000Z",
      },
    ]);

    const page = await invoke<{ notifications: Array<{ id: string; readAt: string | null }>; cursor: string | null; hasMore: boolean }>(
      "list_notifications",
      {},
    );
    const unreadCount = await invoke<number>("get_unread_notification_count", {});
    const settings = await invoke<typeof notificationSettings>("get_notification_settings", {});

    await expect(invoke("mark_notification_read", { id: "notification-mock-git-auth" })).resolves.toBeNull();
    await expect(invoke("mark_all_notifications_read", { projectId: "project-mock-1" })).resolves.toBeNull();
    await expect(invoke("set_dock_badge_count", { count: unreadCount })).resolves.toBeNull();
    await expect(invoke("update_notification_settings", { input: { focusedToastsEnabled: false } })).resolves.toEqual(notificationSettings);

    expect(page).toMatchObject({ cursor: null, hasMore: false });
    expect(page.notifications).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "notification-mock-git-auth", readAt: null }),
      expect.objectContaining({ id: "notification-mock-read", readAt: expect.any(String) }),
    ]));
    expect(unreadCount).toBe(1);
    await expect(invoke("get_unread_notification_count", {})).resolves.toBe(0);
    expect(settings).toEqual(notificationSettings);
    expect(warn).not.toHaveBeenCalled();
  });
});

describe("review settings command mocks", () => {
  afterEach(async () => {
    await invoke("update_review_settings", {
      input: { autoCreateFollowupAgentConversation: false },
    });
  });

  it("defaults automatic follow-up conversations to disabled and allows an explicit opt-in", async () => {
    const initial = await invoke<{
      auto_create_followup_agent_conversation: boolean;
    }>("get_review_settings", {});
    expect(initial.auto_create_followup_agent_conversation).toBe(false);

    const updated = await invoke<{
      auto_create_followup_agent_conversation: boolean;
    }>("update_review_settings", {
      input: { autoCreateFollowupAgentConversation: true },
    });
    expect(updated.auto_create_followup_agent_conversation).toBe(true);
  });
});

describe("manual role default command mocks", () => {
  it("mirrors the complete backend catalog and project inheritance sources", async () => {
    const globalCatalog = await invoke<{
      roles: Array<{
        role: string;
        description: string;
        configured: Record<string, unknown> | null;
        effective: Record<string, unknown> | null;
        source: string;
      }>;
    }>("get_manual_role_defaults", { projectId: null });
    expect(globalCatalog.roles).toHaveLength(29);
    expect(globalCatalog.roles.every((role) => role.description.trim().length > 0)).toBe(true);

    const inheritedCatalog = await invoke<typeof globalCatalog>(
      "get_manual_role_defaults",
      { projectId: "mock-agents-project" },
    );
    const inheritedEdit = inheritedCatalog.roles.find(
      (role) => role.role === "workspace_edit",
    );
    expect(inheritedEdit).toMatchObject({
      configured: null,
      effective: globalCatalog.roles.find((role) => role.role === "workspace_edit")?.configured,
      source: "global_ui",
    });

    await invoke("update_manual_role_default", {
      input: {
        projectId: "mock-agents-project",
        role: "workspace_edit",
        value: { marker: "project override" },
      },
    });
    const projectCatalog = await invoke<typeof globalCatalog>(
      "get_manual_role_defaults",
      { projectId: "mock-agents-project" },
    );
    expect(projectCatalog.roles.find((role) => role.role === "workspace_edit"))
      .toMatchObject({
        configured: { marker: "project override" },
        effective: { marker: "project override" },
        source: "project_ui",
      });

    await invoke("clear_manual_role_default", {
      input: { projectId: "mock-agents-project", role: "workspace_edit" },
    });
  });
});

describe("ticketing command mocks", () => {
  it("returns ticket filter options in the same object shape as the Tauri command", async () => {
    const options = await invoke<{
      assignees: string[];
      sprints: string[];
      complete: boolean;
      truncated: boolean;
    }>("list_ticket_filter_options", {
      query: { provider: "jira", projectId: "project-mock-1", containerId: "RX" },
    });

    expect(options).toEqual({
      assignees: ["A. Dev"],
      sprints: [],
      complete: true,
      truncated: false,
    });
  });
});
