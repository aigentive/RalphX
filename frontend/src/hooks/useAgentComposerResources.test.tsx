import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clickupApi } from "@/api/clickup";
import { linearApi } from "@/api/linear";
import { atlassianApi } from "@/api/atlassian";

import { agentComposerKeys, useAgentComposerIntegrationResources } from "./useAgentComposerResources";

vi.mock("@/api/clickup", () => ({
  clickupApi: {
    searchTasks: vi.fn(),
  },
}));

vi.mock("@/api/linear", () => ({
  linearApi: {
    searchIssues: vi.fn(),
  },
}));

vi.mock("@/api/atlassian", () => ({
  atlassianApi: {
    searchResources: vi.fn(),
  },
}));

function renderIntegrationHook({
  kind,
  query,
  enabled = true,
}: {
  kind: "jira" | "linear" | "clickup" | "confluence" | null;
  query: string;
  enabled?: boolean;
}) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return {
    queryClient,
    ...renderHook(
      () =>
        useAgentComposerIntegrationResources({
          kind,
          query,
          enabled,
        }),
      { wrapper },
    ),
  };
}

describe("agentComposerKeys", () => {
  it("normalizes null integration kind in the query key", () => {
    expect(agentComposerKeys.integrations(null, "mbe")).toEqual([
      "agent-composer",
      "integrations",
      { kind: null, query: "mbe" },
    ]);
  });
});

describe("useAgentComposerIntegrationResources", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useRealTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("debounces ClickUp task searches and sends title text to the backend", async () => {
    vi.mocked(clickupApi.searchTasks).mockResolvedValue([
      {
        id: "task-1",
        customId: "MBE-2857",
        name: "Inbox classifier",
        url: "https://app.clickup.com/t/MBE-2857",
        statusName: "To Do",
        statusType: "open",
        statusCategory: "todo",
        statusColor: "#999",
        assignees: ["Adrian"],
        tags: ["frontend"],
        spaceId: "space-1",
        listName: "Current Sprint",
        updatedAt: "2026-06-23T00:00:00Z",
      },
    ]);

    const { result } = renderIntegrationHook({
      kind: "clickup",
      query: "inbox classifier",
    });

    expect(result.current.fetchStatus).toBe("idle");
    expect(clickupApi.searchTasks).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(clickupApi.searchTasks).toHaveBeenCalledWith({
        query: "inbox classifier",
        limit: 10,
      }),
      { timeout: 2_000 },
    );
    await waitFor(() =>
      expect(result.current.data).toEqual([
        expect.objectContaining({ id: "task-1", customId: "MBE-2857" }),
      ]),
    );
  });

  it("limits ClickUp task suggestions to ten items", async () => {
    vi.mocked(clickupApi.searchTasks).mockResolvedValue(
      Array.from({ length: 10 }, (_, index) => ({
        id: `task-${index}`,
        name: `Task ${index}`,
        assignees: [],
        tags: ["current"],
      })),
    );

    const { result } = renderIntegrationHook({
      kind: "clickup",
      query: "current",
    });

    await waitFor(() => expect(result.current.data).toHaveLength(10), {
      timeout: 2_000,
    });
    expect(clickupApi.searchTasks).toHaveBeenCalledWith({
      query: "current",
      limit: 10,
    });
    expect(result.current.data?.[0]).toMatchObject({ id: "task-0" });
  });

  it("runs empty ClickUp searches immediately for menu-open suggestions", async () => {
    vi.mocked(clickupApi.searchTasks).mockResolvedValue([]);

    const { result } = renderIntegrationHook({
      kind: "clickup",
      query: "   ",
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(clickupApi.searchTasks).toHaveBeenCalledWith({
      query: "",
      limit: 10,
    });
  });

  it("does not call provider APIs while disabled or kindless", () => {
    const disabled = renderIntegrationHook({
      kind: "clickup",
      query: "mbe",
      enabled: false,
    });
    const kindless = renderIntegrationHook({
      kind: null,
      query: "mbe",
    });

    expect(disabled.result.current.fetchStatus).toBe("idle");
    expect(kindless.result.current.fetchStatus).toBe("idle");
    expect(clickupApi.searchTasks).not.toHaveBeenCalled();
    expect(linearApi.searchIssues).not.toHaveBeenCalled();
    expect(atlassianApi.searchResources).not.toHaveBeenCalled();
  });

  it("passes Linear and Atlassian searches through with trimmed queries", async () => {
    vi.mocked(linearApi.searchIssues).mockResolvedValue([]);
    vi.mocked(atlassianApi.searchResources).mockResolvedValue([]);

    const linear = renderIntegrationHook({
      kind: "linear",
      query: "  LIN-123  ",
    });
    await waitFor(() => expect(linear.result.current.isSuccess).toBe(true));

    const jira = renderIntegrationHook({
      kind: "jira",
      query: "  RX-42  ",
    });
    await waitFor(() => expect(jira.result.current.isSuccess).toBe(true));

    expect(linearApi.searchIssues).toHaveBeenCalledWith({
      query: "LIN-123",
      limit: 12,
    });
    expect(atlassianApi.searchResources).toHaveBeenCalledWith({
      kind: "jira",
      query: "RX-42",
      limit: 12,
    });
  });
});
