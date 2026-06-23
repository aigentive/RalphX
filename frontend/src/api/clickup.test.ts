import { beforeEach, describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";

import {
  ClickUpIntegrationSettingsSchema,
  ClickUpTaskSummarySchema,
  ClickUpWorkspaceSchema,
  clickupApi,
} from "./clickup";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

describe("clickupApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads, saves, and validates ClickUp integration settings", async () => {
    const settings = {
      enabled: true,
      hasApiToken: true,
      workspaceId: "team-1",
      validationStatus: "valid",
      taskSearchAvailable: true,
      lastValidatedAt: "2026-06-17T12:00:00Z",
      lastError: null,
      updatedAt: "2026-06-17T12:00:00Z",
    };
    vi.mocked(typedInvoke).mockResolvedValue(settings);

    await expect(clickupApi.getSettings()).resolves.toEqual(settings);
    await expect(
      clickupApi.saveSettings({ apiToken: "pk_token", workspaceId: "team-1" }),
    ).resolves.toEqual(settings);
    await expect(clickupApi.validate()).resolves.toEqual(settings);

    expect(typedInvoke).toHaveBeenNthCalledWith(
      1,
      "get_clickup_integration_settings",
      {},
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      2,
      "save_clickup_integration_settings",
      { input: { apiToken: "pk_token", workspaceId: "team-1" } },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      3,
      "validate_clickup_integration",
      {},
      expect.any(Object),
    );
  });

  it("disconnects the ClickUp integration", async () => {
    const settings = {
      enabled: false,
      hasApiToken: false,
      workspaceId: null,
      validationStatus: "not_configured",
      taskSearchAvailable: false,
      lastValidatedAt: null,
      lastError: null,
      updatedAt: "2026-06-17T12:00:00Z",
    };
    vi.mocked(typedInvoke).mockResolvedValue(settings);

    await expect(clickupApi.disconnect()).resolves.toEqual(settings);

    expect(typedInvoke).toHaveBeenCalledWith(
      "disconnect_clickup_integration",
      {},
      expect.any(Object),
    );
  });

  it("unwraps ClickUp workspace list responses", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      workspaces: [
        { id: "team-1", name: "Acme", color: "#ff6b35" },
        { id: "team-2", name: "Globex", color: null },
      ],
    });

    await expect(clickupApi.listWorkspaces()).resolves.toEqual([
      { id: "team-1", name: "Acme", color: "#ff6b35" },
      { id: "team-2", name: "Globex", color: null },
    ]);

    expect(typedInvoke).toHaveBeenCalledWith(
      "list_clickup_workspaces",
      {},
      expect.any(Object),
    );
  });

  it("unwraps ClickUp task search responses and wraps spaceIds under input", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      tasks: [
        {
          id: "task-1",
          name: "Fix bug",
          assignees: [],
          tags: [],
        },
      ],
    });

    await expect(
      clickupApi.searchTasks({ spaceIds: ["space-1", "space-2"] }),
    ).resolves.toEqual([
      {
        id: "task-1",
        name: "Fix bug",
        assignees: [],
        tags: [],
      },
    ]);

    expect(typedInvoke).toHaveBeenCalledWith(
      "search_clickup_tasks",
      { input: { spaceIds: ["space-1", "space-2"] } },
      expect.any(Object),
    );
  });

  it("defaults task search to an empty space scope", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({ tasks: [] });

    await expect(clickupApi.searchTasks()).resolves.toEqual([]);

    expect(typedInvoke).toHaveBeenCalledWith(
      "search_clickup_tasks",
      { input: { spaceIds: [] } },
      expect.any(Object),
    );
  });
});

describe("ClickUp Zod schemas match the backend response shapes", () => {
  it("parses the camelCase settings response", () => {
    const parsed = ClickUpIntegrationSettingsSchema.parse({
      enabled: true,
      hasApiToken: true,
      workspaceId: "team-1",
      validationStatus: "valid",
      taskSearchAvailable: true,
      lastValidatedAt: "2026-06-17T12:00:00Z",
      lastError: null,
      updatedAt: "2026-06-17T12:00:00Z",
    });

    expect(parsed.workspaceId).toBe("team-1");
    expect(parsed.taskSearchAvailable).toBe(true);
    expect(parsed.validationStatus).toBe("valid");
  });

  it("parses a workspace and a task summary", () => {
    expect(
      ClickUpWorkspaceSchema.parse({ id: "team-1", name: "Acme" }),
    ).toEqual({ id: "team-1", name: "Acme" });

    const task = ClickUpTaskSummarySchema.parse({
      id: "task-1",
      name: "Fix bug",
      statusName: "In Progress",
      statusCategory: "in_progress",
      assignees: ["ada"],
      tags: ["bug"],
      spaceId: "space-1",
    });
    expect(task.statusCategory).toBe("in_progress");
    expect(task.assignees).toEqual(["ada"]);
  });
});
