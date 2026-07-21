import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";
import type { PullRequestDetail } from "@/api/github";
import { usePullRequestDetail } from "@/hooks/usePullRequestDetail";

import { AgentWorkspaceToolbar } from "./AgentWorkspaceToolbar";
import { useAgentWorkspaceFullFreshness } from "./useAgentWorkspaceFullFreshness";

vi.mock("@/hooks/usePullRequestDetail", () => ({
  usePullRequestDetail: vi.fn(),
}));

vi.mock("./useAgentWorkspaceFullFreshness", () => ({
  useAgentWorkspaceFullFreshness: vi.fn(),
}));

vi.mock("@/components/ticketing/ticketing-open-external", () => ({
  openExternalTicketUrl: vi.fn(),
}));

function workspace(
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "edit",
    branchMode: "isolated",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    baseCommit: "base-sha",
    branchName: "ralphx/demo/agent-conversation-1",
    worktreePath: "/tmp/ralphx/conversation-1",
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: "pushed",
    status: "active",
    createdAt: "2026-07-20T10:00:00Z",
    updatedAt: "2026-07-20T10:00:00Z",
    ...overrides,
  };
}

function fullFreshness(
  overrides: Partial<AgentConversationWorkspaceFreshness> = {},
): AgentConversationWorkspaceFreshness {
  return {
    conversationId: "conversation-1",
    freshnessScope: "full",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    targetRef: "origin/main",
    capturedBaseCommit: "base-sha",
    targetBaseCommit: "base-sha",
    isBaseAhead: false,
    hasUncommittedChanges: false,
    unpublishedCommitCount: 0,
    remoteRefreshed: true,
    worktreeStatusChecked: true,
    baseStatus: "valid",
    effectiveBaseRef: null,
    effectiveBaseDisplayName: null,
    baseBlockReason: null,
    ...overrides,
  };
}

function detail(
  number: number,
  overrides: Partial<PullRequestDetail> = {},
): PullRequestDetail {
  return {
    state: "loaded",
    origin: "ownedOutbound",
    description: {
      number,
      title: `PR ${number}`,
      body: "Ready",
      author: "octocat",
      createdAt: "2026-07-20T10:00:00Z",
      url: `https://github.com/acme/app/pull/${number}`,
      state: "open",
      isDraft: false,
      headRefName: `feature/pr-${number}`,
      baseRefName: "main",
    },
    checks: [
      {
        name: "ci",
        status: "completed",
        conclusion: "success",
        detailsUrl: null,
      },
      {
        name: "build",
        status: "in_progress",
        conclusion: null,
        detailsUrl: null,
      },
    ],
    reviewSummary: {
      reviewDecision: "APPROVED",
      latestChangesRequestedAuthor: null,
      latestChangesRequestedBody: null,
      latestChangesRequestedSubmittedAt: null,
      latestChangesRequestedComments: [],
    },
    issueComments: [],
    reviewThread: [],
    rxConversations: [],
    linkedTickets: [],
    sourcesUnavailable: [],
    ...overrides,
  };
}

describe("AgentWorkspaceToolbar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useAgentWorkspaceFullFreshness).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useAgentWorkspaceFullFreshness>);
    vi.mocked(usePullRequestDetail).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: false,
      fetchStatus: "idle",
    } as ReturnType<typeof usePullRequestDetail>);
  });

  it("renders no toolbar without a normal workspace", () => {
    const { container } = render(<AgentWorkspaceToolbar workspace={null} />);

    expect(container).toBeEmptyDOMElement();
    expect(usePullRequestDetail).not.toHaveBeenCalled();
  });

  it("keeps immediate workspace context when there is no pull request", () => {
    render(<AgentWorkspaceToolbar workspace={workspace()} />);

    const toolbar = screen.getByRole("region", { name: "Workspace status" });
    expect(toolbar).toHaveTextContent("ralphx/demo/agent-conversation-1");
    expect(toolbar).toHaveTextContent("Project default (main)");
    expect(toolbar).toHaveTextContent("No PR yet");
    expect(toolbar).toHaveTextContent("Edit");
    expect(toolbar).toHaveTextContent("Pushed");
    expect(usePullRequestDetail).not.toHaveBeenCalled();
  });

  it("paints the PR shell before enabling deferred health and then shows status", async () => {
    vi.mocked(usePullRequestDetail).mockImplementation(
      (selector, options) =>
        ({
          data: options.enabled ? detail(selector?.prNumber ?? 42) : undefined,
          isLoading: !options.enabled,
          isError: false,
          fetchStatus: options.enabled ? "idle" : "idle",
        }) as ReturnType<typeof usePullRequestDetail>,
    );

    render(
      <AgentWorkspaceToolbar
        workspace={workspace({
          publicationPrNumber: 42,
          publicationPrUrl: "https://github.com/acme/app/pull/42",
          publicationPrStatus: "open",
        })}
      />,
    );

    const prLink = screen.getByRole("button", {
      name: "Open PR #42 in GitHub",
    });
    expect(prLink).toBeInTheDocument();
    expect(prLink).toHaveClass(
      "focus-visible:[outline:2px_solid_var(--border-focus)]",
    );
    expect(screen.getByTestId("pr-status-strip-skeleton")).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-workspace-pr-health"),
    ).toHaveAttribute("aria-live", "polite");
    expect(screen.getByTestId("agents-workspace-pr-health").tagName).toBe(
      "DIV",
    );
    expect(usePullRequestDetail).toHaveBeenCalledWith(
      { projectId: "project-1", prNumber: 42 },
      { enabled: false },
    );

    await waitFor(() =>
      expect(usePullRequestDetail).toHaveBeenCalledWith(
        { projectId: "project-1", prNumber: 42 },
        { enabled: true },
      ),
    );
    expect(screen.getByText("Approved")).toBeInTheDocument();
    expect(screen.getByText("1 passed")).toBeInTheDocument();
    expect(screen.getByText("1 pending")).toBeInTheDocument();
  });

  it("clears old PR health immediately when selector identity changes", async () => {
    vi.mocked(usePullRequestDetail).mockImplementation(
      (selector, options) =>
        ({
          data: options.enabled ? detail(selector?.prNumber ?? 0) : undefined,
          isLoading: !options.enabled,
          isError: false,
          fetchStatus: options.enabled ? "idle" : "idle",
        }) as ReturnType<typeof usePullRequestDetail>,
    );
    const first = workspace({
      publicationPrNumber: 42,
      publicationPrUrl: "https://github.com/acme/app/pull/42",
      publicationPrStatus: "open",
    });
    const { rerender } = render(<AgentWorkspaceToolbar workspace={first} />);

    await screen.findByText("Approved");

    rerender(
      <AgentWorkspaceToolbar
        workspace={workspace({
          conversationId: "conversation-2",
          publicationPrNumber: 77,
          publicationPrUrl: "https://github.com/acme/app/pull/77",
          publicationPrStatus: "draft",
        })}
      />,
    );

    expect(
      screen.getByRole("button", { name: "Open PR #77 in GitHub" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Approved")).not.toBeInTheDocument();
    expect(screen.getByTestId("pr-status-strip-skeleton")).toBeInTheDocument();
    expect(usePullRequestDetail).toHaveBeenLastCalledWith(
      { projectId: "project-1", prNumber: 77 },
      { enabled: false },
    );
  });

  it("uses publication PR over a source PR and does not duplicate terminal state", () => {
    vi.mocked(useAgentWorkspaceFullFreshness).mockReturnValue({
      data: fullFreshness({ isBaseAhead: true }),
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useAgentWorkspaceFullFreshness>);

    render(
      <AgentWorkspaceToolbar
        workspace={workspace({
          sourcePullRequest: {
            number: 11,
            url: "https://github.com/acme/app/pull/11",
            title: "Source PR",
            headRefName: "source-pr",
          },
          publicationPrNumber: 42,
          publicationPrUrl: "https://github.com/acme/app/pull/42",
          publicationPrStatus: "merged",
          publicationPushStatus: "pushed",
          prSupervisionStatus: "blocked",
          prSupervisionSummary: "GitHub reported merge conflicts",
        })}
      />,
    );

    expect(screen.getByText("PR #42")).toBeInTheDocument();
    expect(screen.queryByText("PR #11")).not.toBeInTheDocument();
    expect(screen.getAllByText("Merged")).toHaveLength(1);
    expect(screen.queryByText("Conflicting")).not.toBeInTheDocument();
    expect(screen.queryByText("Behind base")).not.toBeInTheDocument();
  });

  it("suppresses stale attention state when source PR detail is terminal", async () => {
    vi.mocked(useAgentWorkspaceFullFreshness).mockReturnValue({
      data: fullFreshness({ isBaseAhead: true }),
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useAgentWorkspaceFullFreshness>);
    const mergedDetail = detail(11);
    vi.mocked(usePullRequestDetail).mockReturnValue({
      data: {
        ...mergedDetail,
        description: {
          ...mergedDetail.description!,
          state: "merged",
        },
      },
      isLoading: false,
      isError: false,
      fetchStatus: "idle",
    } as ReturnType<typeof usePullRequestDetail>);

    render(
      <AgentWorkspaceToolbar
        workspace={workspace({
          publicationPrNumber: null,
          publicationPrUrl: null,
          publicationPrStatus: null,
          sourcePullRequest: {
            number: 11,
            url: "https://github.com/acme/app/pull/11",
            title: "Source PR",
            headRefName: "source-pr",
          },
          prSupervisionStatus: "blocked",
          prSupervisionSummary: "GitHub reported merge conflicts",
        })}
      />,
    );

    expect(await screen.findByText("Merged")).toBeInTheDocument();
    expect(screen.queryByText("Conflicting")).not.toBeInTheDocument();
    expect(screen.queryByText("Behind base")).not.toBeInTheDocument();
  });

  it("shows focused workspace loading and unavailable shells without querying", () => {
    const { rerender } = render(
      <AgentWorkspaceToolbar workspace={null} resolutionState="loading" />,
    );

    expect(
      screen.getByRole("status", { name: "Loading workspace status" }),
    ).toBeInTheDocument();

    rerender(
      <AgentWorkspaceToolbar workspace={null} resolutionState="unavailable" />,
    );

    expect(
      screen.getByText("Workspace status unavailable"),
    ).toBeInTheDocument();
    expect(useAgentWorkspaceFullFreshness).toHaveBeenLastCalledWith(null, {
      enabled: false,
    });
    expect(usePullRequestDetail).not.toHaveBeenCalled();
  });

  it("re-defers full freshness when the workspace conversation changes", async () => {
    const { rerender } = render(
      <AgentWorkspaceToolbar workspace={workspace()} />,
    );

    await waitFor(() =>
      expect(useAgentWorkspaceFullFreshness).toHaveBeenLastCalledWith(
        "conversation-1",
        { enabled: true },
      ),
    );

    rerender(
      <AgentWorkspaceToolbar
        workspace={workspace({ conversationId: "conversation-2" })}
      />,
    );

    expect(useAgentWorkspaceFullFreshness).toHaveBeenLastCalledWith(
      "conversation-2",
      { enabled: false },
    );
  });

  it("uses the established Review PR mode label", () => {
    render(
      <AgentWorkspaceToolbar workspace={workspace({ mode: "review_pr" })} />,
    );

    expect(screen.getByTestId("agents-workspace-mode-status")).toHaveTextContent(
      "Review PR",
    );
  });
});
