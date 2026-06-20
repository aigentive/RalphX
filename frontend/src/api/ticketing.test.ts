import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ticketingApi } from "./ticketing";

const capabilities = {
  supportsBoards: true,
  supportsKanban: true,
  kanbanWrite: false,
  statusWrite: false,
  assignmentWrite: false,
  commentWrite: false,
  freshness: "manual",
};

describe("ticketingApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("lists providers through the normalized read command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      {
        provider: "jira",
        label: "Jira",
        enabled: true,
        connectionStatus: "connected",
        capabilities,
        fetchedAt: "2026-06-19T22:00:00.000Z",
      },
    ]);

    const providers = await ticketingApi.listProviders({ projectId: "project-1" });

    expect(invoke).toHaveBeenCalledWith("list_ticketing_providers", {
      projectId: "project-1",
    });
    expect(providers).toHaveLength(1);
    expect(providers[0]?.capabilities.supportsKanban).toBe(true);
  });

  it("passes ticket filters and pagination with camelCase invoke args", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      items: [],
      nextCursor: "cursor-2",
      total: 24,
      fetchedAt: "2026-06-19T22:00:00.000Z",
    });

    const page = await ticketingApi.listTickets({
      provider: "linear",
      projectId: "project-1",
      containerId: "team-1",
      cursor: "cursor-1",
      limit: 30,
      filters: {
        text: "merge",
        stateIds: ["started"],
        labels: ["backend"],
      },
      sort: "updated_desc",
    });

    expect(invoke).toHaveBeenCalledWith("list_tickets", {
      query: {
        provider: "linear",
        projectId: "project-1",
        containerId: "team-1",
        cursor: "cursor-1",
        limit: 30,
        filters: {
          text: "merge",
          stateIds: ["started"],
          labels: ["backend"],
        },
        sort: "updated_desc",
      },
    });
    expect(page.nextCursor).toBe("cursor-2");
  });
});
