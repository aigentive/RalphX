import { createElement, type ReactNode } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { PullRequestDetail } from "@/api/github";
import { diffApi } from "@/api/diff";
import { usePullRequestDetail } from "@/hooks/usePullRequestDetail";
import { TooltipProvider } from "@/components/ui/tooltip";
import { openExternalTicketUrl } from "@/components/ticketing/ticketing-open-external";

import { PullRequestDetailBody } from "./PullRequestDetailBody";

vi.mock("@/hooks/usePullRequestDetail", () => ({
  usePullRequestDetail: vi.fn(),
}));

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspacePrAnnotations: vi.fn(),
  },
}));

vi.mock("@/components/ticketing/ticketing-open-external", () => ({
  openExternalTicketUrl: vi.fn(),
}));

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({ conversationIdOverride }: { conversationIdOverride?: string }) => (
    <div data-testid="rx-chat-panel">RX {conversationIdOverride}</div>
  ),
}));

function loadedDetail(overrides: Partial<PullRequestDetail> = {}): PullRequestDetail {
  return {
    state: "loaded",
    origin: "ownedOutbound",
    description: {
      number: 42,
      title: "Add PR detail",
      body: "## Summary\nReady for review.",
      author: "octocat",
      createdAt: "2026-06-24T08:00:00Z",
      url: "https://github.com/acme/app/pull/42",
      state: "open",
      isDraft: false,
      headRefName: "feat/pr-detail",
      baseRefName: "main",
    },
    checks: [{ name: "ci", status: "completed", conclusion: "success", detailsUrl: null }],
    reviewSummary: {
      reviewDecision: "CHANGES_REQUESTED",
      latestChangesRequestedAuthor: "alice",
      latestChangesRequestedBody: "Please address feedback.",
      latestChangesRequestedSubmittedAt: "2026-06-24T10:00:00Z",
      latestChangesRequestedComments: [],
    },
    issueComments: [
      {
        id: "comment-1",
        author: "reviewer",
        body: "Looks good",
        url: null,
        createdAt: "2026-06-24T09:00:00Z",
        updatedAt: null,
        isBot: false,
        isCodecov: false,
        source: "live",
      },
      {
        id: "codecov-1",
        author: "codecov[bot]",
        body: "Coverage 90%",
        url: null,
        createdAt: "2026-06-24T09:30:00Z",
        updatedAt: null,
        isBot: true,
        isCodecov: true,
        source: "live",
      },
    ],
    reviewThread: [
      {
        id: "thread-1",
        author: "reviewer",
        body: "Inline note",
        path: "src/app.ts",
        side: "RIGHT",
        line: 12,
        url: null,
        createdAt: null,
        inReplyToId: null,
        isOutdated: false,
      },
    ],
    rxConversations: [
      {
        conversationId: "conversation-1",
        branchName: "feat/pr-detail",
        linkedIdeationSessionId: null,
        publicationPrNumber: 42,
        publicationPrStatus: "open",
      },
    ],
    linkedTickets: [],
    sourcesUnavailable: [],
    ...overrides,
  };
}

function renderBody(
  detail: PullRequestDetail | null = null,
  shellOverrides: Partial<NonNullable<Parameters<typeof PullRequestDetailBody>[0]["shell"]>> = {},
  bodyOverrides: {
    showRxConversation?: boolean;
    presentation?: "default" | "agentsWorkspace";
  } = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  vi.mocked(usePullRequestDetail).mockReturnValue({
    data: detail,
    isLoading: false,
    isError: false,
    fetchStatus: "idle",
  } as ReturnType<typeof usePullRequestDetail>);

  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(
      QueryClientProvider,
      { client: queryClient },
      createElement(TooltipProvider, null, children),
    );

  return render(
    <PullRequestDetailBody
      selector={{ projectId: "project-1", prNumber: 42 }}
      shell={{
        projectId: "project-1",
        prNumber: 42,
        title: "PR #42",
        url: "https://github.com/acme/app/pull/42",
        status: "open",
        branch: "feat/pr-detail",
        conversationId: "conversation-1",
        ...shellOverrides,
      }}
      {...bodyOverrides}
    />,
    { wrapper },
  );
}

describe("PullRequestDetailBody", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useRealTimers();
    vi.mocked(diffApi.getAgentConversationWorkspacePrAnnotations).mockResolvedValue({
      prNumber: 42,
      headSha: "abc123",
      annotations: [],
      sourcesUnavailable: [],
    });
  });

  it("paints the shell before enabling the GitHub detail fetch", () => {
    renderBody(null);

    expect(screen.getByText("PR #42")).toBeInTheDocument();
    expect(screen.getByTestId("pull-request-detail-body")).toBeInTheDocument();
    expect(usePullRequestDetail).toHaveBeenCalledWith(
      { projectId: "project-1", prNumber: 42 },
      { enabled: false },
    );
  });

  it("enables the detail fetch after a paint boundary", async () => {
    renderBody(null);

    await waitFor(() =>
      expect(usePullRequestDetail).toHaveBeenCalledWith(
        { projectId: "project-1", prNumber: 42 },
        { enabled: true },
      ),
    );
  });

  it("renders the status strip, review, summarized checks, and human comments", async () => {
    renderBody(loadedDetail());

    expect(screen.getByText("Add PR detail")).toBeInTheDocument();
    expect(screen.getByText("Summary")).toBeInTheDocument();

    // At-a-glance status strip surfaces the review decision.
    expect(screen.getByTestId("pr-status-strip")).toBeInTheDocument();
    expect(screen.getAllByText("Changes requested").length).toBeGreaterThan(0);

    // Review section surfaces the changes-requested body + inline thread.
    expect(screen.getByText("Please address feedback.")).toBeInTheDocument();
    expect(screen.getByText("Inline note")).toBeInTheDocument();

    // Checks are summarized (strip + checks section); the passing check is not
    // listed by default.
    expect(screen.getAllByText("1 passed").length).toBeGreaterThan(0);
    expect(screen.queryByText("ci")).not.toBeInTheDocument();

    // Human comments stay; the Codecov bot comment is hidden with a count.
    expect(screen.getByText("Looks good")).toBeInTheDocument();
    expect(screen.queryByText("Coverage 90%")).not.toBeInTheDocument();
    expect(screen.getByText("1 automated comment hidden.")).toBeInTheDocument();

    expect(await screen.findByTestId("rx-chat-panel")).toHaveTextContent("conversation-1");
  });

  it("adds an accessible check-details disclosure only when a URL exists", async () => {
    const user = userEvent.setup();
    renderBody(
      loadedDetail({
        checks: [
          {
            name: "lint",
            status: "completed",
            conclusion: "failure",
            detailsUrl: "https://github.com/acme/app/actions/runs/1",
          },
          {
            name: "types",
            status: "completed",
            conclusion: "failure",
            detailsUrl: null,
          },
        ],
      }),
      {},
      { showRxConversation: false },
    );

    const detailsButton = screen.getByRole("button", {
      name: "Open lint check details",
    });
    expect(
      screen.queryByRole("button", { name: "Open types check details" }),
    ).not.toBeInTheDocument();

    await user.click(detailsButton);

    expect(openExternalTicketUrl).toHaveBeenCalledWith(
      "https://github.com/acme/app/actions/runs/1",
    );
  });

  it("renders GitHub details blocks in the pull request description", () => {
    const detail = loadedDetail();
    const { container } = renderBody(
      {
        ...detail,
        description: {
          ...detail.description!,
          body: [
            "## Summary",
            "Ready for review.",
            "",
            "<details>",
            "<summary>View full plan</summary>",
            "",
            "### Implementation",
            "- Keep markdown formatting",
            "</details>",
          ].join("\n"),
        },
      },
      {},
      { showRxConversation: false },
    );

    const details = screen.getByTestId("pr-markdown-details");
    expect(details).toHaveTextContent("View full plan");
    expect(screen.getByRole("heading", { name: "Implementation" })).toBeInTheDocument();
    expect(screen.getByText("Keep markdown formatting")).toBeInTheDocument();
    expect(container.textContent).not.toContain("<details>");
    expect(container.textContent).not.toContain("<summary>");
  });

  it("orders the sections description -> review -> checks -> comments", () => {
    renderBody(loadedDetail(), {}, { showRxConversation: false });

    const order = screen
      .getAllByRole("heading")
      .map((heading) => heading.textContent ?? "");
    const index = (prefix: string) => order.findIndex((text) => text.startsWith(prefix));

    expect(index("Description")).toBeGreaterThanOrEqual(0);
    expect(index("Review")).toBeGreaterThan(index("Description"));
    expect(index("Checks")).toBeGreaterThan(index("Review"));
    expect(index("Comments")).toBeGreaterThan(index("Checks"));
  });

  it("hides the embedded RX conversation when showRxConversation is false", async () => {
    renderBody(loadedDetail(), {}, { showRxConversation: false });

    // Review section still renders so the RX embed is gone, not the whole body.
    expect(screen.getByText("Inline note")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Review/ })).toBeInTheDocument();
    expect(screen.queryByText(/Conversation \(RX\)/)).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByTestId("rx-chat-panel")).not.toBeInTheDocument(),
    );
  });

  it("removes duplicated workspace chrome only in the Agents presentation", () => {
    renderBody(
      loadedDetail(),
      {},
      {
        presentation: "agentsWorkspace",
        showRxConversation: false,
      },
    );

    expect(
      screen.getByRole("heading", { name: "Add PR detail" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Open")).not.toBeInTheDocument();
    expect(screen.queryByText("#42")).not.toBeInTheDocument();
    expect(screen.queryByText("head feat/pr-detail")).not.toBeInTheDocument();
    expect(screen.queryByText("feat/pr-detail")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Open pull request in GitHub" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("pr-status-strip")).not.toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Description" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Review/ })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Checks/ })).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: /Comments/ }),
    ).toBeInTheDocument();
    expect(usePullRequestDetail).toHaveBeenCalled();
    for (const [, options] of vi.mocked(usePullRequestDetail).mock.calls) {
      expect(options).not.toHaveProperty("preferCachedData");
    }
  });

  it("does not claim an unknown shell-only pull request is open", () => {
    renderBody(null, { status: null });

    expect(screen.queryByText("Open")).not.toBeInTheDocument();
    expect(screen.getByText("PR #42")).toBeInTheDocument();
  });

  it("loads annotations only after the checks section is expanded", async () => {
    const user = userEvent.setup();
    renderBody(loadedDetail());

    expect(diffApi.getAgentConversationWorkspacePrAnnotations).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Annotations" }));

    await waitFor(() =>
      expect(diffApi.getAgentConversationWorkspacePrAnnotations).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
  });
});
