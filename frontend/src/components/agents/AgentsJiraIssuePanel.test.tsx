import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { atlassianApi, type AgentConversationJiraIssue } from "@/api/atlassian";
import { TooltipProvider } from "@/components/ui/tooltip";

import { AgentsJiraIssuePanel } from "./AgentsJiraIssuePanel";

vi.mock("@/api/atlassian", async () => {
  const actual = await vi.importActual<typeof import("@/api/atlassian")>(
    "@/api/atlassian"
  );
  return {
    ...actual,
    atlassianApi: {
      getAgentConversationJiraIssue: vi.fn(),
      assignAgentConversationJiraIssue: vi.fn(),
      refreshAgentConversationJiraIssue: vi.fn(),
      assignAgentConversationJiraIssueToMe: vi.fn(),
      clearAgentConversationJiraIssue: vi.fn(),
    },
  };
});

vi.mock("@/hooks/useAgentComposerResources", () => ({
  useAgentComposerIntegrationResources: () => ({
    data: [],
    isFetching: false,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}));

const getIssueMock = vi.mocked(atlassianApi.getAgentConversationJiraIssue);
const refreshIssueMock = vi.mocked(atlassianApi.refreshAgentConversationJiraIssue);
const assignToMeMock = vi.mocked(atlassianApi.assignAgentConversationJiraIssueToMe);

function issue(overrides: Partial<AgentConversationJiraIssue> = {}): AgentConversationJiraIssue {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    provider: "atlassian",
    issueKey: "RX-42",
    issueId: "10042",
    issueUrl: "https://jira.test/browse/RX-42",
    title: "Fix Jira tab",
    status: "To Do",
    assignee: "A. User",
    reporter: "R. User",
    updatedAtRemote: "2026-06-18T08:00:00Z",
    descriptionMarkdown: "# Follow-up\n\nBody text",
    descriptionText: "Follow-up\n\nBody text",
    acceptanceCriteriaMarkdown: "## Acceptance Criteria\n\n- Visible",
    acceptanceCriteriaText: "Acceptance Criteria\nVisible",
    comments: [],
    attachments: [],
    lastRefreshedAt: "2026-06-18T08:05:00Z",
    refreshStatus: "loaded",
    refreshError: null,
    assignedAt: "2026-06-18T08:00:00Z",
    assignedFromMessageId: null,
    manuallyAssigned: false,
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
    <AgentsJiraIssuePanel conversationId="conversation-1" projectId="project-1" />,
    { wrapper }
  );
  return queryClient;
}

describe("AgentsJiraIssuePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("removes line selection while keeping Jira details selectable", async () => {
    getIssueMock.mockResolvedValue(issue());

    renderPanel();

    const body = await screen.findByText("Body text");
    expect(
      screen.queryByRole("button", { name: "Select ticket lines" }),
    ).not.toBeInTheDocument();
    expect(
      body.closest("[data-artifact-selectable-region='true']"),
    ).not.toBeNull();
  });

  it("renders Jira markdown headings with theme color and full-width body text", async () => {
    getIssueMock.mockResolvedValue(issue());

    renderPanel();

    const heading = await screen.findByRole("heading", { name: "Follow-up" });
    expect(heading).toHaveStyle({ color: "var(--text-primary)" });

    const paragraph = screen.getByText("Body text").closest("p");
    expect(paragraph?.style.maxWidth).toBe("");
  });

  it("renders Jira markdown code blocks with Jira-local document styling", async () => {
    getIssueMock.mockResolvedValue(
      issue({
        descriptionMarkdown: "```ts\nconst status = 'ready';\n```",
        descriptionText: "const status = 'ready';",
        acceptanceCriteriaMarkdown: null,
        acceptanceCriteriaText: null,
      }),
    );

    renderPanel();

    const code = await screen.findByText("const status = 'ready';");
    const pre = code.closest("pre");
    expect(pre).toHaveClass("overflow-x-auto");
    expect(pre?.getAttribute("style")).toContain("background-color: var(--bg-surface)");
    expect(pre?.getAttribute("style")).toContain("border-color: var(--border-subtle)");
    expect(screen.queryByRole("button", { name: /copy code/i })).not.toBeInTheDocument();
  });

  it("shows a clear empty state after Jira loads without acceptance criteria", async () => {
    getIssueMock.mockResolvedValue(
      issue({
        acceptanceCriteriaMarkdown: null,
        acceptanceCriteriaText: null,
        comments: [],
        attachments: [],
      }),
    );

    renderPanel();

    expect(await screen.findByRole("heading", { name: "Description" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Acceptance Criteria" })).toBeInTheDocument();
    expect(screen.getByText("No acceptance criteria on this issue.")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Comments (0)" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Attachments (0)" })).not.toBeInTheDocument();
  });

  it("hides the acceptance criteria empty state while Jira details are not loaded", async () => {
    getIssueMock.mockResolvedValue(
      issue({
        acceptanceCriteriaMarkdown: null,
        acceptanceCriteriaText: null,
        refreshStatus: "not_loaded",
      }),
    );
    refreshIssueMock.mockReturnValue(new Promise(() => {}));

    renderPanel();

    expect(await screen.findByRole("heading", { name: "Description" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Acceptance Criteria" })).not.toBeInTheDocument();
    expect(screen.queryByText("No acceptance criteria on this issue.")).not.toBeInTheDocument();
  });

  it("renders Jira comments with author and body", async () => {
    getIssueMock.mockResolvedValue(
      issue({
        comments: [
          {
            id: "comment-1",
            author: "Jira Reviewer",
            bodyMarkdown: "Please cover the **custom field**.",
            bodyText: "Please cover the custom field.",
            createdAt: "2026-06-18T08:06:00Z",
            updatedAt: null,
          },
        ],
      }),
    );

    renderPanel();

    expect(await screen.findByRole("heading", { name: "Comments (1)" })).toBeInTheDocument();
    expect(screen.getByText("Jira Reviewer")).toBeInTheDocument();
    expect(screen.getByText("custom field")).toBeInTheDocument();
  });

  it("renders Jira metadata as a compact summary row", async () => {
    getIssueMock.mockResolvedValue(issue());

    renderPanel();

    const metadata = await screen.findByLabelText("Jira issue metadata");
    expect(metadata).toHaveClass("flex");
    expect(metadata).toHaveClass("flex-wrap");
    expect(metadata).not.toHaveClass("grid");
    expect(within(metadata).getByText("Assignee")).toBeInTheDocument();
    expect(within(metadata).getByText("A. User")).toBeInTheDocument();
    expect(within(metadata).getByText("Reporter")).toBeInTheDocument();
    expect(within(metadata).getByText("R. User")).toBeInTheDocument();
  });

  it("shows unknown Jira assignees as unassigned with an assign-to-me action", async () => {
    getIssueMock.mockResolvedValue(issue({ assignee: "Unknown" }));
    assignToMeMock.mockResolvedValue(issue({ assignee: "A. User" }));

    renderPanel();

    expect(await screen.findByText("Unassigned")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Assign to me" }));

    await waitFor(() =>
      expect(assignToMeMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
      }),
    );
    expect(await screen.findByText("A. User")).toBeInTheDocument();
  });

  it("refreshes newly assigned not-loaded Jira issues without a manual click", async () => {
    getIssueMock.mockResolvedValue(
      issue({
        descriptionMarkdown: null,
        descriptionText: null,
        acceptanceCriteriaMarkdown: null,
        acceptanceCriteriaText: null,
        refreshStatus: "not_loaded",
        lastRefreshedAt: null,
      })
    );
    refreshIssueMock.mockResolvedValue(
      issue({
        descriptionMarkdown: "## Loaded Details\n\nFetched from Jira",
        descriptionText: "Loaded Details\nFetched from Jira",
      })
    );

    renderPanel();

    await waitFor(() =>
      expect(refreshIssueMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
      })
    );
    expect(await screen.findByRole("heading", { name: "Loaded Details" })).toBeInTheDocument();
  });
});
