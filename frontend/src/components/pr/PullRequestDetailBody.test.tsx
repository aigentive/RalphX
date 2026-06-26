import { createElement, type ReactNode } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { PullRequestDetail } from "@/api/github";
import { diffApi } from "@/api/diff";
import { usePullRequestDetail } from "@/hooks/usePullRequestDetail";
import { TooltipProvider } from "@/components/ui/tooltip";

import { PullRequestDetailBody } from "./PullRequestDetailBody";

vi.mock("@/hooks/usePullRequestDetail", () => ({
  usePullRequestDetail: vi.fn(),
}));

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspacePrAnnotations: vi.fn(),
  },
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
    reviewSummary: null,
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

function renderBody(detail: PullRequestDetail | null = null) {
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
      }}
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

  it("renders description, comments, RX conversation, and checks", async () => {
    renderBody(loadedDetail());

    expect(screen.getByText("Add PR detail")).toBeInTheDocument();
    expect(screen.getByText("Summary")).toBeInTheDocument();
    expect(screen.getByText("Looks good")).toBeInTheDocument();
    expect(screen.getByText("Inline note")).toBeInTheDocument();
    expect(await screen.findByTestId("rx-chat-panel")).toHaveTextContent("conversation-1");
    expect(screen.getByText("ci")).toBeInTheDocument();
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
