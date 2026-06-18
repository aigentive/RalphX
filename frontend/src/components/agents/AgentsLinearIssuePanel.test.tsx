import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { linearApi, type AgentConversationLinearIssue } from "@/api/linear";
import { TooltipProvider } from "@/components/ui/tooltip";

import { AgentsLinearIssuePanel } from "./AgentsLinearIssuePanel";

vi.mock("@/api/linear", async () => {
  const actual = await vi.importActual<typeof import("@/api/linear")>("@/api/linear");
  return {
    ...actual,
    linearApi: {
      getAgentConversationLinearIssue: vi.fn(),
      assignAgentConversationLinearIssue: vi.fn(),
      refreshAgentConversationLinearIssue: vi.fn(),
      clearAgentConversationLinearIssue: vi.fn(),
      searchIssues: vi.fn(),
    },
  };
});

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}));

const getIssueMock = vi.mocked(linearApi.getAgentConversationLinearIssue);
const searchIssuesMock = vi.mocked(linearApi.searchIssues);
const assignIssueMock = vi.mocked(linearApi.assignAgentConversationLinearIssue);
const refreshIssueMock = vi.mocked(linearApi.refreshAgentConversationLinearIssue);
const clearIssueMock = vi.mocked(linearApi.clearAgentConversationLinearIssue);

function issue(
  overrides: Partial<AgentConversationLinearIssue> = {},
): AgentConversationLinearIssue {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    provider: "linear",
    issueId: "issue-1",
    issueKey: "LIN-123",
    issueUrl: "https://linear.app/acme/issue/LIN-123/fix-linear-tab",
    title: "Fix Linear tab",
    status: "In Progress",
    assignee: "A. User",
    reporter: "C. User",
    updatedAtRemote: "2026-06-18T08:00:00Z",
    descriptionMarkdown: "## Details\n\nBody text",
    descriptionText: "Details\nBody text",
    comments: [],
    attachments: [],
    lastRefreshedAt: "2026-06-18T08:05:00Z",
    refreshStatus: "loaded",
    refreshError: null,
    assignedAt: "2026-06-18T08:00:00Z",
    assignedFromMessageId: null,
    manuallyAssigned: true,
    createdAt: "2026-06-18T08:00:00Z",
    updatedAt: "2026-06-18T08:05:00Z",
    ...overrides,
  };
}

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{children}</TooltipProvider>
    </QueryClientProvider>
  );
  render(
    <AgentsLinearIssuePanel conversationId="conversation-1" projectId="project-1" />,
    { wrapper },
  );
  return queryClient;
}

describe("AgentsLinearIssuePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    searchIssuesMock.mockResolvedValue([]);
  });

  it("refreshes newly assigned not-loaded Linear issues without a manual click", async () => {
    getIssueMock.mockResolvedValue(
      issue({
        descriptionMarkdown: null,
        descriptionText: null,
        refreshStatus: "not_loaded",
        lastRefreshedAt: null,
      }),
    );
    refreshIssueMock.mockResolvedValue(
      issue({
        descriptionMarkdown: "## Loaded Details\n\nFetched from Linear",
        descriptionText: "Loaded Details\nFetched from Linear",
      }),
    );

    renderPanel();

    await waitFor(() =>
      expect(refreshIssueMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
      }),
    );
    expect(await screen.findByRole("heading", { name: "Loaded Details" })).toBeInTheDocument();
  });

  it("searches and assigns Linear issues directly", async () => {
    getIssueMock.mockResolvedValue(null);
    searchIssuesMock.mockResolvedValue([
      {
        id: "issue-2",
        key: "LIN-456",
        title: "Assign this issue",
        url: "https://linear.app/acme/issue/LIN-456/assign-this-issue",
        excerpt: "Candidate",
        stateName: "Todo",
      },
    ]);
    assignIssueMock.mockResolvedValue(issue({ issueId: "issue-2", issueKey: "LIN-456" }));

    renderPanel();

    fireEvent.change(await screen.findByPlaceholderText("Search Linear issues"), {
      target: { value: "LIN-456" },
    });
    fireEvent.click(await screen.findByRole("button", { name: /LIN-456/i }));

    await waitFor(() =>
      expect(assignIssueMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        issueId: "issue-2",
        issueKey: "LIN-456",
        title: "Assign this issue",
        issueUrl: "https://linear.app/acme/issue/LIN-456/assign-this-issue",
      }),
    );
  });

  it("exposes accessible refresh, unlink, and open-link actions", async () => {
    const openSpy = vi.spyOn(window, "open").mockImplementation(() => null);
    getIssueMock.mockResolvedValue(issue());
    refreshIssueMock.mockResolvedValue(issue({ title: "Refreshed Linear issue" }));
    clearIssueMock.mockResolvedValue(null);

    renderPanel();

    fireEvent.click(await screen.findByRole("button", { name: "Open Linear issue" }));
    expect(openSpy).toHaveBeenCalledWith(
      "https://linear.app/acme/issue/LIN-123/fix-linear-tab",
      "_blank",
      "noopener,noreferrer",
    );

    fireEvent.click(screen.getByRole("button", { name: "Refresh Linear issue" }));
    await waitFor(() =>
      expect(refreshIssueMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Unlink Linear issue" }));
    await waitFor(() =>
      expect(clearIssueMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
      }),
    );

    openSpy.mockRestore();
  });
});
