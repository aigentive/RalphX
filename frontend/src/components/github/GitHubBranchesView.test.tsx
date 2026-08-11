import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { githubApi, type GitHubBranchOverviewItem } from "@/api/github";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useIntegrationDashboardStore } from "@/stores/integrationDashboardStore";
import type { Project } from "@/types/project";

import { GitHubBranchesView } from "./GitHubBranchesView";

const { openExternalTicketUrlMock } = vi.hoisted(() => ({
  openExternalTicketUrlMock: vi.fn(),
}));

vi.mock("@/api/github", () => ({
  githubApi: {
    getConnectionStatus: vi.fn(),
    getBranchOverview: vi.fn(),
  },
}));

vi.mock("@/components/ticketing/ticketing-open-external", () => ({
  openExternalTicketUrl: (...args: unknown[]) => openExternalTicketUrlMock(...args),
}));

vi.mock("@/components/pr/PullRequestDetailSheet", () => ({
  PullRequestDetailSheet: ({
    open,
    selector,
    shell,
    onClose,
  }: {
    open: boolean;
    selector: { projectId: string; prNumber?: number; branch?: string } | null;
    shell: { branch?: string | null; prNumber?: number | null; title?: string | null } | null;
    onClose: () => void;
  }) =>
    open ? (
      <div role="dialog" data-testid="pr-detail-sheet">
        <span>{shell?.branch}</span>
        <span>{selector?.prNumber ?? selector?.branch}</span>
        <span>{shell?.title}</span>
        <button type="button" onClick={onClose}>
          Close
        </button>
      </div>
    ) : null,
}));

const project: Project = {
  id: "project-1",
  name: "Current Project",
  workingDirectory: "/repo/current",
  gitMode: "worktree",
  baseBranch: "main",
  worktreeParentDirectory: null,
  useFeatureBranches: true,
  mergeValidationMode: "block",
  detectedAnalysis: null,
  customAnalysis: null,
  analyzedAt: null,
  githubPrEnabled: true,
  createdAt: "2026-06-19T22:00:00.000Z",
  updatedAt: "2026-06-19T22:00:00.000Z",
};

const branches: GitHubBranchOverviewItem[] = [
  {
    branchName: "feature/current",
    isCurrent: true,
    prNumber: 466,
    prTitle: "Fix GitHub branch view",
    prUrl: "https://github.com/aigentive/ralphx.app/pull/466",
    prStatus: "open",
    prIsDraft: false,
    prUpdatedAt: "2026-06-28T07:00:00.000Z",
    prAuthorLogin: "reefagent",
    prAssigneeLogins: ["lazabogdan"],
    prReviewDecision: "REVIEW_REQUIRED",
    prLatestReviewAuthorLogins: ["reviewer"],
    prReviewRequestLogins: ["lazabogdan"],
    prBaseRefName: "main",
    rxConversationCount: 1,
    rxConversations: [{ conversationId: "conversation-1", title: "Branch work" }],
    ticketCount: 1,
    ticketLinks: [
      {
        provider: "jira",
        label: "RX-77",
        title: "Branch ticket",
        url: "https://example.atlassian.net/browse/RX-77",
      },
    ],
    ticketLabels: ["Jira RX-77"],
  },
  {
    branchName: "feature/no-pr",
    isCurrent: false,
    prNumber: null,
    prTitle: null,
    prUrl: null,
    prStatus: null,
    prIsDraft: false,
    prUpdatedAt: null,
    prAuthorLogin: null,
    prAssigneeLogins: [],
    prReviewDecision: null,
    prLatestReviewAuthorLogins: [],
    prReviewRequestLogins: [],
    prBaseRefName: null,
    rxConversationCount: 0,
    rxConversations: [],
    ticketCount: 0,
    ticketLinks: [],
    ticketLabels: [],
  },
  {
    branchName: "feature/merged",
    isCurrent: false,
    prNumber: 465,
    prTitle: "Merged branch view",
    prUrl: "https://github.com/aigentive/ralphx.app/pull/465",
    prStatus: "merged",
    prIsDraft: false,
    prUpdatedAt: "2026-06-27T07:00:00.000Z",
    prAuthorLogin: "reefagent",
    prAssigneeLogins: ["adriandemian"],
    prReviewDecision: "APPROVED",
    prLatestReviewAuthorLogins: ["lazabogdan"],
    prReviewRequestLogins: [],
    prBaseRefName: "main",
    rxConversationCount: 0,
    rxConversations: [],
    ticketCount: 0,
    ticketLinks: [],
    ticketLabels: [],
  },
  {
    branchName: "ralphx/ticket/clickup-cu-1",
    isCurrent: false,
    prNumber: null,
    prTitle: null,
    prUrl: null,
    prStatus: null,
    prIsDraft: false,
    prUpdatedAt: null,
    prAuthorLogin: null,
    prAssigneeLogins: [],
    prReviewDecision: null,
    prLatestReviewAuthorLogins: [],
    prReviewRequestLogins: [],
    prBaseRefName: null,
    rxConversationCount: 0,
    rxConversations: [],
    ticketCount: 1,
    ticketLinks: [
      {
        provider: "clickup",
        label: "cu-1",
        title: null,
        url: null,
      },
    ],
    ticketLabels: ["ClickUp cu-1"],
  },
];

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
    },
  });

  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>{children}</TooltipProvider>
      </QueryClientProvider>
    );
  };
}

function renderBranchesView(
  props: Partial<Parameters<typeof GitHubBranchesView>[0]> = {},
) {
  const Wrapper = createWrapper();
  return render(
    <GitHubBranchesView
      projectId="project-1"
      project={project}
      {...props}
    />,
    { wrapper: Wrapper },
  );
}

describe("GitHubBranchesView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useIntegrationDashboardStore.getState().reset();
    openExternalTicketUrlMock.mockResolvedValue(undefined);
    vi.mocked(githubApi.getConnectionStatus).mockResolvedValue({
      state: "authenticated",
      diagnostic: null,
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "lazabogdan",
    });
    vi.mocked(githubApi.getBranchOverview).mockResolvedValue({
      currentBranch: "feature/current",
      sourcesUnavailable: [],
      branches,
    });
  });

  it("renders grouped branch rows with PR, ticket, and RalphX indicators", async () => {
    renderBranchesView();

    expect(await screen.findByTestId("github-branches-view")).toBeInTheDocument();
    const currentRow = screen.getByTestId("github-branch-row-feature/current");
    expect(within(currentRow).getByText("Fix GitHub branch view")).toHaveClass("text-sm");
    expect(within(currentRow).getByText("feature/current")).toHaveClass("text-xs");
    expect(currentRow).toHaveTextContent("#466");
    expect(currentRow).toHaveTextContent("reefagent");
    expect(within(currentRow).getByLabelText("1 attached ticket")).toBeInTheDocument();
    expect(within(currentRow).getByLabelText("1 RalphX conversation")).toBeInTheDocument();
    expect(screen.getByTestId("github-branch-row-feature/merged")).toHaveTextContent("#465");
    expect(screen.queryByTestId("github-branch-row-feature/no-pr")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("github-branch-row-ralphx/ticket/clickup-cu-1"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Branches 4/ }));
    const currentBranchRow = screen.getByTestId("github-branch-row-feature/current");
    expect(within(currentBranchRow).getByText("feature/current")).toHaveClass("text-sm");
    expect(within(currentBranchRow).getByText("Fix GitHub branch view")).toHaveClass("text-xs");
    expect(screen.getByTestId("github-branch-row-feature/no-pr")).toHaveTextContent("No PR");
    expect(screen.getByTestId("github-branch-row-ralphx/ticket/clickup-cu-1")).toHaveTextContent(
      "ClickUp cu-1",
    );
    expect(githubApi.getBranchOverview).toHaveBeenCalledWith({ projectId: "project-1" });
  });

  it("filters by PR status and by attached ticket or RalphX metadata", async () => {
    renderBranchesView();

    await screen.findByTestId("github-branch-row-feature/current");
    expect(screen.getByTestId("github-branch-row-feature/current")).toBeInTheDocument();
    expect(screen.getByTestId("github-branch-row-feature/merged")).toBeInTheDocument();
    expect(screen.queryByTestId("github-branch-row-feature/no-pr")).not.toBeInTheDocument();
    expect(screen.queryByTestId("github-branch-row-ralphx/ticket/clickup-cu-1")).not.toBeInTheDocument();

    fireEvent.click(screen.getAllByRole("button", { name: /Merged 1/ })[0]);
    expect(screen.getByTestId("github-branch-row-feature/merged")).toBeInTheDocument();
    expect(screen.queryByTestId("github-branch-row-feature/current")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Tickets 2/ }));
    expect(screen.getByTestId("github-branch-row-feature/current")).toBeInTheDocument();
    expect(screen.getByTestId("github-branch-row-ralphx/ticket/clickup-cu-1")).toBeInTheDocument();
    expect(screen.queryByTestId("github-branch-row-feature/merged")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /RX 1/ }));
    expect(screen.getByTestId("github-branch-row-feature/current")).toBeInTheDocument();
    expect(screen.queryByTestId("github-branch-row-ralphx/ticket/clickup-cu-1")).not.toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("Search branches, PRs, tickets, assignees, or authors"), {
      target: { value: "missing" },
    });
    expect(screen.getByText("No branches match these filters.")).toBeInTheDocument();
  });

  it("filters pull requests by assignees, authors, and review state", async () => {
    renderBranchesView();

    await screen.findByTestId("github-branch-row-feature/current");
    expect(screen.getByTestId("github-branch-row-feature/current")).toHaveTextContent("lazabogdan");
    expect(screen.getByTestId("github-branch-row-feature/merged")).toHaveTextContent("adriandemian");

    fireEvent.click(screen.getByRole("combobox", { name: "GitHub assignee" }));
    fireEvent.click(screen.getByRole("option", { name: /adriandemian/ }));
    expect(screen.getByTestId("github-branch-row-feature/merged")).toBeInTheDocument();
    expect(screen.queryByTestId("github-branch-row-feature/current")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Clear GitHub assignee filter" }));
    fireEvent.click(screen.getByRole("combobox", { name: "GitHub author" }));
    fireEvent.click(screen.getByRole("option", { name: /reefagent/ }));
    expect(screen.getByTestId("github-branch-row-feature/current")).toBeInTheDocument();
    expect(screen.getByTestId("github-branch-row-feature/merged")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("combobox", { name: "GitHub reviews" }));
    fireEvent.click(screen.getByRole("option", { name: /Review required/ }));
    expect(screen.getByTestId("github-branch-row-feature/current")).toBeInTheDocument();
    expect(screen.queryByTestId("github-branch-row-feature/merged")).not.toBeInTheDocument();
  });

  it("opens PR details from row clicks and follows ticket or RX controls", async () => {
    const onNavigateToAssociation = vi.fn();
    renderBranchesView({ onNavigateToAssociation });

    const currentRow = await screen.findByTestId("github-branch-row-feature/current");
    fireEvent.click(currentRow);
    expect(screen.getByTestId("pr-detail-sheet")).toHaveTextContent("feature/current");
    expect(screen.getByTestId("pr-detail-sheet")).toHaveTextContent("466");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    fireEvent.click(within(currentRow).getByLabelText("1 attached ticket"));
    expect(openExternalTicketUrlMock).toHaveBeenCalledWith(
      "https://example.atlassian.net/browse/RX-77",
    );

    fireEvent.click(within(currentRow).getByLabelText("1 RalphX conversation"));
    expect(onNavigateToAssociation).toHaveBeenCalledWith({
      view: "agents",
      id: "conversation-1",
      projectId: "project-1",
    });

    fireEvent.click(screen.getByRole("button", { name: /Branches 4/ }));
    const ticketOnlyRow = screen.getByTestId("github-branch-row-ralphx/ticket/clickup-cu-1");
    fireEvent.click(within(ticketOnlyRow).getByLabelText("1 attached ticket"));
    expect(screen.getByTestId("pr-detail-sheet")).toHaveTextContent(
      "ralphx/ticket/clickup-cu-1",
    );
  });

  it("restores filters and the selected branch after remounting from sidebar navigation", async () => {
    const firstRender = renderBranchesView();

    await screen.findByTestId("github-branch-row-feature/current");
    fireEvent.click(screen.getByRole("button", { name: /Tickets 2/ }));
    fireEvent.change(screen.getByPlaceholderText("Search branches, PRs, tickets, assignees, or authors"), {
      target: { value: "cu-1" },
    });

    const ticketOnlyRow = screen.getByTestId("github-branch-row-ralphx/ticket/clickup-cu-1");
    fireEvent.click(ticketOnlyRow);
    expect(screen.getByTestId("pr-detail-sheet")).toHaveTextContent(
      "ralphx/ticket/clickup-cu-1",
    );

    firstRender.unmount();
    renderBranchesView();

    expect(await screen.findByTestId("github-branch-row-ralphx/ticket/clickup-cu-1")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Search branches, PRs, tickets, assignees, or authors")).toHaveValue(
      "cu-1",
    );
    expect(screen.queryByTestId("github-branch-row-feature/current")).not.toBeInTheDocument();
    expect(screen.getByTestId("pr-detail-sheet")).toHaveTextContent(
      "ralphx/ticket/clickup-cu-1",
    );
  });

  it("opens a selected PR when navigation stores the PR number instead of the branch name", async () => {
    useIntegrationDashboardStore.getState().setGitHubState("project-1", {
      associationFilter: "pull_requests",
      searchQuery: "466",
      selectedBranchName: "466",
    });

    renderBranchesView();

    expect(await screen.findByTestId("github-branch-row-feature/current")).toBeInTheDocument();
    expect(screen.getByTestId("pr-detail-sheet")).toHaveTextContent("feature/current");
    expect(screen.getByTestId("pr-detail-sheet")).toHaveTextContent("466");
  });
});
