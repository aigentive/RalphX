import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { granolaApi } from "@/api/granola";
import type { PullRequestDetail } from "@/api/github";
import { TooltipProvider } from "@/components/ui/tooltip";
import { usePullRequestDetail } from "@/hooks/usePullRequestDetail";
import * as ticketingHooks from "@/hooks/useTicketing";

import { PullRequestDetailSheet } from "./PullRequestDetailSheet";

vi.mock("@/hooks/usePullRequestDetail", () => ({
  usePullRequestDetail: vi.fn(),
}));

vi.mock("@/hooks/useTicketing", () => ({
  useTicketAssociations: vi.fn(),
  useTicketDetail: vi.fn(),
  useTicketTransitions: vi.fn(),
}));

vi.mock("@/api/granola", () => ({
  granolaApi: {
    getSettings: vi.fn(),
    listNotes: vi.fn(),
    assignAgentConversationGranolaNote: vi.fn(),
  },
}));

vi.mock("@/components/agents/agentGranolaNoteQueries", () => ({
  invalidateAgentConversationGranolaNote: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("./PullRequestDetailBody", () => ({
  PullRequestDetailBody: () => <div data-testid="pr-detail-body">PR body</div>,
}));

function loadedDetail(overrides: Partial<PullRequestDetail> = {}): PullRequestDetail {
  return {
    state: "loaded",
    origin: "ownedOutbound",
    description: {
      number: 466,
      title: "Plan GitHub PR and conversation integration",
      body: null,
      author: "reefagent",
      createdAt: "2026-06-28T08:00:00Z",
      url: "https://github.com/aigentive/ralphx.app/pull/466",
      state: "open",
      isDraft: false,
      headRefName: "ralphx/printspeak/agent-54e3266d",
      baseRefName: "main",
    },
    checks: [],
    reviewSummary: null,
    issueComments: [],
    reviewThread: [],
    rxConversations: [
      {
        conversationId: "conversation-1",
        branchName: "ralphx/printspeak/agent-54e3266d",
        linkedIdeationSessionId: null,
        publicationPrNumber: 466,
        publicationPrStatus: "open",
      },
    ],
    linkedTickets: [
      {
        provider: "linear",
        issueKey: "WISE-27",
        url: "https://linear.app/acme/issue/WISE-27",
      },
    ],
    sourcesUnavailable: [],
    ...overrides,
  };
}

function renderSheet(props: Partial<Parameters<typeof PullRequestDetailSheet>[0]> = {}) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <PullRequestDetailSheet
          open
          selector={{ projectId: "project-1", prNumber: 466 }}
          shell={{
            projectId: "project-1",
            prNumber: 466,
            title: "Plan GitHub PR and conversation integration",
            url: "https://github.com/aigentive/ralphx.app/pull/466",
            status: "open",
            branch: "ralphx/printspeak/agent-54e3266d",
            conversationId: "conversation-1",
            rxConversations: [
              {
                conversationId: "conversation-1",
                title: "Branch work",
              },
            ],
            ticketLinks: [
              {
                provider: "linear",
                label: "WISE-27",
                title: "Branch ticket",
                url: "https://linear.app/acme/issue/WISE-27",
              },
            ],
          }}
          onClose={vi.fn()}
          {...props}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("PullRequestDetailSheet", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(usePullRequestDetail).mockReturnValue({
      data: loadedDetail(),
      isLoading: false,
      fetchStatus: "idle",
    } as ReturnType<typeof usePullRequestDetail>);
    vi.mocked(ticketingHooks.useTicketAssociations).mockReturnValue({
      data: undefined,
      isLoading: false,
    } as ReturnType<typeof ticketingHooks.useTicketAssociations>);
    vi.mocked(ticketingHooks.useTicketDetail).mockReturnValue({
      data: undefined,
      isLoading: false,
      isFetching: false,
    } as ReturnType<typeof ticketingHooks.useTicketDetail>);
    vi.mocked(ticketingHooks.useTicketTransitions).mockReturnValue({
      data: [],
    } as ReturnType<typeof ticketingHooks.useTicketTransitions>);
    vi.mocked(granolaApi.getSettings).mockResolvedValue({
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      lastValidatedAt: "2026-06-28T08:00:00Z",
      lastError: null,
      updatedAt: "2026-06-28T08:00:00Z",
    });
    vi.mocked(granolaApi.listNotes).mockResolvedValue({
      notes: [],
      hasMore: false,
      cursor: null,
    });
    vi.mocked(granolaApi.assignAgentConversationGranolaNote).mockResolvedValue(null);
  });

  it("shows PR-side RX, ticket, and Granola association sections", async () => {
    renderSheet();

    expect(screen.getByText("Associations")).toBeInTheDocument();
    expect(screen.getByText("RX Conversations (1)")).toBeInTheDocument();
    expect(screen.getByText("ralphx/printspeak/agent-54e3266d")).toBeInTheDocument();
    expect(screen.getByText("Tickets (1)")).toBeInTheDocument();
    expect(screen.getByText("Linear WISE-27")).toBeInTheDocument();
    expect(await screen.findByText("Granola (0)")).toBeInTheDocument();
    expect(await screen.findByText("No Granola notes linked.")).toBeInTheDocument();
  });

  it("falls back to branch overview associations when PR detail links are empty", () => {
    vi.mocked(usePullRequestDetail).mockReturnValue({
      data: loadedDetail({ rxConversations: [], linkedTickets: [] }),
      isLoading: false,
      fetchStatus: "idle",
    } as ReturnType<typeof usePullRequestDetail>);

    renderSheet();

    expect(screen.getByText("RX Conversations (1)")).toBeInTheDocument();
    expect(screen.getByText("Branch work")).toBeInTheDocument();
    expect(screen.getByText("Tickets (1)")).toBeInTheDocument();
    expect(screen.getByText("Linear WISE-27")).toBeInTheDocument();
  });

  it("opens linked tickets in an in-app sheet and deep-links RX conversations", async () => {
    const user = userEvent.setup();
    const onNavigateToAssociation = vi.fn();
    renderSheet({ onNavigateToAssociation });

    await user.click(screen.getByRole("button", { name: /ralphx\/printspeak\/agent-54e3266d/ }));
    expect(onNavigateToAssociation).toHaveBeenCalledWith({
      view: "agents",
      id: "conversation-1",
      projectId: "project-1",
    });

    await user.click(screen.getByRole("button", { name: /Linear WISE-27/ }));
    expect(screen.getByText("WISE-27 · Linear")).toBeInTheDocument();
    expect(screen.getAllByText("Branch ticket").length).toBeGreaterThan(0);
    await waitFor(() => {
      expect(ticketingHooks.useTicketDetail).toHaveBeenCalledWith({
        provider: "linear",
        ticketRef: {
          provider: "linear",
          id: "WISE-27",
          key: "WISE-27",
        },
      }, {
        enabled: true,
      });
    });
  });

  it("shows multiple Granola notes linked through PR, RX, or ticket associations", async () => {
    const user = userEvent.setup();
    const onNavigateToAssociation = vi.fn();
    vi.mocked(granolaApi.listNotes).mockResolvedValue({
      notes: [
        {
          id: "not_pr",
          title: "PR planning",
          url: "https://granola.ai/notes/not_pr",
          summary: "Covered the PR.",
          createdAt: "2026-06-28T08:00:00Z",
          updatedAt: "2026-06-28T08:00:00Z",
          rxConversationCount: 0,
          rxConversations: [],
          ticketCount: 0,
          ticketLinks: [],
          prCount: 1,
          pullRequests: [{ number: 466, status: "open", url: null }],
        },
        {
          id: "not_rx",
          title: "Conversation follow-up",
          url: null,
          summary: null,
          createdAt: "2026-06-28T09:00:00Z",
          updatedAt: "2026-06-28T09:00:00Z",
          rxConversationCount: 1,
          rxConversations: [{ conversationId: "conversation-1", title: "Branch work" }],
          ticketCount: 0,
          ticketLinks: [],
          prCount: 0,
          pullRequests: [],
        },
        {
          id: "not_ticket",
          title: "Ticket sync",
          url: null,
          summary: null,
          createdAt: "2026-06-28T10:00:00Z",
          updatedAt: "2026-06-28T10:00:00Z",
          rxConversationCount: 0,
          rxConversations: [],
          ticketCount: 1,
          ticketLinks: [{ provider: "linear", label: "WISE-27", title: "Branch ticket", url: null }],
          prCount: 0,
          pullRequests: [],
        },
      ],
      hasMore: false,
      cursor: null,
    });

    renderSheet({ onNavigateToAssociation });

    expect(await screen.findByText("Granola (3)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /PR planning/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Conversation follow-up/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Ticket sync/ })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Ticket sync/ }));
    expect(onNavigateToAssociation).toHaveBeenCalledWith({
      view: "granola",
      id: "not_ticket",
      projectId: "project-1",
    });
  });

  it("binds an existing Granola note through a PR conversation", async () => {
    const user = userEvent.setup();
    vi.mocked(granolaApi.listNotes).mockResolvedValue({
      notes: [
        {
          id: "not_available",
          title: "Available PR note",
          url: "https://granola.ai/notes/not_available",
          summary: "Context from a meeting.",
          createdAt: "2026-06-28T11:00:00Z",
          updatedAt: "2026-06-28T11:00:00Z",
          rxConversationCount: 0,
          rxConversations: [],
          ticketCount: 0,
          ticketLinks: [],
          prCount: 0,
          pullRequests: [],
        },
      ],
      hasMore: false,
      cursor: null,
    });

    renderSheet();

    expect(await screen.findByText("Granola (0)")).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Add Granola" }));
    await user.click(screen.getByRole("combobox", { name: "Granola note" }));
    await user.click(screen.getByRole("option", { name: /Available PR note/ }));
    await user.click(screen.getByRole("button", { name: "Bind Granola note" }));

    await waitFor(() => {
      expect(granolaApi.assignAgentConversationGranolaNote).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        noteId: "not_available",
        title: "Available PR note",
        noteUrl: "https://granola.ai/notes/not_available",
        summary: "Context from a meeting.",
        includeTranscript: true,
        refresh: true,
      });
    });
  });
});
