import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { atlassianApi } from "@/api/atlassian";
import { linearApi } from "@/api/linear";
import * as chatHooks from "@/hooks/useChat";
import * as ticketingHooks from "@/hooks/useTicketing";
import * as projectHooks from "@/hooks/useProjects";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useTicketingStore } from "@/stores/ticketingStore";

import { TicketingDashboardView } from "./TicketingDashboardView";

vi.mock("@/api/atlassian", () => ({
  atlassianApi: {
    assignAgentConversationJiraIssue: vi.fn().mockResolvedValue(null),
  },
}));

vi.mock("@/api/linear", () => ({
  linearApi: {
    assignAgentConversationLinearIssue: vi.fn().mockResolvedValue(null),
  },
}));

vi.mock("@/hooks/useChat", () => ({
  useConversations: vi.fn(),
}));

vi.mock("@/hooks/useTicketing", () => ({
  ticketingKeys: {
    all: ["ticketing"],
    detail: (input: { provider: string; ticketRef: { id: string; key?: string | null } }) => [
      "ticketing",
      "detail",
      input.provider,
      input.ticketRef.id,
      input.ticketRef.key ?? null,
    ],
    associations: (input: {
      provider: string;
      ticketRef: { id: string; key?: string | null };
      projectId: string;
    }) => [
      "ticketing",
      "detail",
      input.provider,
      input.ticketRef.id,
      input.ticketRef.key ?? null,
      "associations",
      input.projectId,
    ],
    conversationTicket: (conversationId: string) => [
      "ticketing",
      "conversation-ticket",
      conversationId,
    ],
  },
  fetchTicketTransitionsForMove: vi.fn(),
  findTicketTransitionForColumn: vi.fn(),
  flattenTicketPages: vi.fn((data) => data?.pages.flatMap((page) => page.items) ?? []),
  useRefreshTickets: vi.fn(),
  useStartWorkFromTicket: vi.fn(),
  useTicketAssociations: vi.fn(),
  useTicketDetail: vi.fn(),
  useTicketingMutations: vi.fn(),
  useTicketingColumns: vi.fn(),
  useTicketingContainers: vi.fn(),
  useTicketingProviders: vi.fn(),
  useTicketLabelOptions: vi.fn(),
  useTicketTransitions: vi.fn(),
  useTickets: vi.fn(),
}));

vi.mock("@/hooks/useProjects", () => ({
  useProjects: vi.fn(),
}));

const capabilities = {
  supportsBoards: true,
  supportsKanban: true,
  kanbanWrite: false,
  statusWrite: false,
  assignmentWrite: false,
  commentWrite: false,
  freshness: "manual" as const,
};

const writableCapabilities = {
  ...capabilities,
  kanbanWrite: true,
  statusWrite: true,
  assignmentWrite: true,
  commentWrite: true,
};

const ticket = {
  ref: { provider: "jira" as const, id: "10001", key: "RX-1" },
  title: "Fix merge race in transition handler",
  state: { id: "todo", name: "To Do", category: "todo" as const },
  labels: ["backend"],
  updatedAt: "2026-06-19T22:00:00.000Z",
  url: "https://example.atlassian.net/browse/RX-1",
  associationCount: 2,
  openPrCount: 0,
};

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
    },
  });

  return function Wrapper({ children }: { children: ReactNode }) {
    return createElement(
      QueryClientProvider,
      { client: queryClient },
      createElement(TooltipProvider, null, children),
    );
  };
}

function renderDashboard() {
  const Wrapper = createWrapper();
  return render(<TicketingDashboardView projectId="project-1" />, {
    wrapper: Wrapper,
  });
}

function mockConnectedDashboard() {
  vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
    data: [
      {
        provider: "jira",
        label: "Jira",
        enabled: true,
        connectionStatus: "connected",
        capabilities: writableCapabilities,
        fetchedAt: "2026-06-19T22:00:00.000Z",
      },
    ],
    isLoading: false,
    isError: false,
    error: null,
  } as ReturnType<typeof ticketingHooks.useTicketingProviders>);
  vi.mocked(ticketingHooks.useTicketingContainers).mockReturnValue({
    data: [{ provider: "jira", id: "board-1", name: "Sprint Board", kind: "board" }],
    isLoading: false,
    isError: false,
    error: null,
  } as ReturnType<typeof ticketingHooks.useTicketingContainers>);
  vi.mocked(ticketingHooks.useTicketingColumns).mockReturnValue({
    data: [{ id: "todo", name: "To Do", category: "todo", order: 0 }],
    isLoading: false,
    isError: false,
    error: null,
  } as ReturnType<typeof ticketingHooks.useTicketingColumns>);
  vi.mocked(ticketingHooks.useTickets).mockReturnValue({
    data: {
      pages: [{ items: [ticket], nextCursor: null, total: 1 }],
      pageParams: [null],
    },
    isLoading: false,
    isFetching: false,
    isError: false,
    error: null,
    hasNextPage: false,
    fetchNextPage: vi.fn(),
    isFetchingNextPage: false,
  } as unknown as ReturnType<typeof ticketingHooks.useTickets>);
  vi.mocked(ticketingHooks.useTicketDetail).mockReturnValue({
    data: {
      ...ticket,
      descriptionMarkdown: "When two agents transition the same task.",
      comments: [],
      attachments: [],
      transitions: [
        {
          toStateId: "done",
          providerTransitionId: "transition-31",
          name: "Done",
          category: "done",
        },
      ],
    },
    isLoading: false,
    isError: false,
    error: null,
  } as ReturnType<typeof ticketingHooks.useTicketDetail>);
  vi.mocked(ticketingHooks.useTicketTransitions).mockReturnValue({
    data: [
      {
        toStateId: "done",
        providerTransitionId: "transition-31",
        name: "Done",
        category: "done",
      },
    ],
    isLoading: false,
    isError: false,
    error: null,
  } as ReturnType<typeof ticketingHooks.useTicketTransitions>);
  vi.mocked(ticketingHooks.useTicketAssociations).mockReturnValue({
    data: {
      tasks: [
        {
          id: "task-1",
          title: "Fix merge race",
          status: "executing",
          active: true,
          deepLink: { view: "kanban", id: "task-1" },
        },
      ],
      proposals: [],
      sessions: [],
      conversations: [],
      pullRequests: [],
      checks: [],
      qa: [],
      specs: [],
    },
    isLoading: false,
    isError: false,
    error: null,
  } as ReturnType<typeof ticketingHooks.useTicketAssociations>);
  vi.mocked(ticketingHooks.useRefreshTickets).mockReturnValue({
    mutate: vi.fn(),
    isPending: false,
  } as unknown as ReturnType<typeof ticketingHooks.useRefreshTickets>);
  vi.mocked(ticketingHooks.useTicketLabelOptions).mockReturnValue({
    data: [],
    isLoading: false,
    isError: false,
    error: null,
  } as unknown as ReturnType<typeof ticketingHooks.useTicketLabelOptions>);
  vi.mocked(ticketingHooks.useTicketingMutations).mockReturnValue({
    transitionStatus: vi.fn().mockResolvedValue(undefined),
    assignToMe: vi.fn().mockResolvedValue(undefined),
    clearAssignee: vi.fn().mockResolvedValue(undefined),
    addComment: vi.fn().mockResolvedValue(undefined),
    setLabels: vi.fn().mockResolvedValue(undefined),
    transitionStatusMutation: { isPending: false },
    assignToMeMutation: { isPending: false },
    clearAssigneeMutation: { isPending: false },
    addCommentMutation: { isPending: false },
    setLabelsMutation: { isPending: false },
  } as unknown as ReturnType<typeof ticketingHooks.useTicketingMutations>);
  vi.mocked(ticketingHooks.useStartWorkFromTicket).mockReturnValue({
    mutate: vi.fn(),
    isPending: false,
    error: null,
  } as unknown as ReturnType<typeof ticketingHooks.useStartWorkFromTicket>);
}

describe("TicketingDashboardView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.HTMLElement.prototype.scrollIntoView = vi.fn();
    useTicketingStore.getState().reset();
    vi.mocked(projectHooks.useProjects).mockReturnValue({
      data: [
        {
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
          githubPrEnabled: false,
          createdAt: "2026-06-19T22:00:00.000Z",
          updatedAt: "2026-06-19T22:00:00.000Z",
        },
        {
          id: "project-2",
          name: "Target Project",
          workingDirectory: "/repo/target",
          gitMode: "worktree",
          baseBranch: "main",
          worktreeParentDirectory: null,
          useFeatureBranches: true,
          mergeValidationMode: "block",
          detectedAnalysis: null,
          customAnalysis: null,
          analyzedAt: null,
          githubPrEnabled: false,
          createdAt: "2026-06-19T22:00:00.000Z",
          updatedAt: "2026-06-19T22:00:00.000Z",
        },
      ],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof projectHooks.useProjects>);
    vi.mocked(ticketingHooks.useTicketingContainers).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);
    vi.mocked(ticketingHooks.useTicketingColumns).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingColumns>);
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: { pages: [{ items: [], nextCursor: null, total: 0 }], pageParams: [null] },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);
    vi.mocked(ticketingHooks.useTicketDetail).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketDetail>);
    vi.mocked(ticketingHooks.useTicketTransitions).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketTransitions>);
    vi.mocked(ticketingHooks.useTicketAssociations).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketAssociations>);
    vi.mocked(ticketingHooks.useRefreshTickets).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof ticketingHooks.useRefreshTickets>);
    vi.mocked(ticketingHooks.useTicketLabelOptions).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useTicketLabelOptions>);
    vi.mocked(ticketingHooks.useTicketingMutations).mockReturnValue({
      transitionStatus: vi.fn().mockResolvedValue(undefined),
      assignToMe: vi.fn().mockResolvedValue(undefined),
      clearAssignee: vi.fn().mockResolvedValue(undefined),
      addComment: vi.fn().mockResolvedValue(undefined),
      setLabels: vi.fn().mockResolvedValue(undefined),
      transitionStatusMutation: { isPending: false },
      assignToMeMutation: { isPending: false },
      clearAssigneeMutation: { isPending: false },
      addCommentMutation: { isPending: false },
      setLabelsMutation: { isPending: false },
    } as unknown as ReturnType<typeof ticketingHooks.useTicketingMutations>);
    vi.mocked(ticketingHooks.useStartWorkFromTicket).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useStartWorkFromTicket>);
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);
  });

  it("renders provider-specific disconnected state without blanking the shell", () => {
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [
        {
          provider: "jira",
          label: "Jira",
          enabled: false,
          connectionStatus: "disconnected",
          capabilities,
          errorMessage: "Connect Jira from Settings.",
        },
      ],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingProviders>);
    vi.mocked(ticketingHooks.useRefreshTickets).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof ticketingHooks.useRefreshTickets>);

    renderDashboard();

    expect(screen.getByRole("heading", { name: "Ticketing" })).toBeInTheDocument();
    expect(screen.getByText("Jira is disconnected")).toBeInTheDocument();
    expect(screen.getByText("Connect Jira from Settings.")).toBeInTheDocument();
  });

  it("does not load tickets for permission-limited providers", () => {
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [
        {
          provider: "linear",
          label: "Linear",
          enabled: false,
          connectionStatus: "permission_limited",
          capabilities,
          permissionMessage: "Linear issue search is not available for this connection.",
        },
      ],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingProviders>);

    renderDashboard();

    expect(screen.getByText("Linear ticket access is limited")).toBeInTheDocument();
    expect(screen.getAllByText("Linear issue search is not available for this connection.").length).toBeGreaterThan(0);
    expect(ticketingHooks.useTickets).toHaveBeenCalledWith(null, { enabled: false });
  });

  it("auto-selects the only enabled provider and hides provider tabs", async () => {
    useTicketingStore.getState().setProvider("jira");
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [
        {
          provider: "jira",
          label: "Jira",
          enabled: false,
          connectionStatus: "disconnected",
          capabilities,
        },
        {
          provider: "linear",
          label: "Linear",
          enabled: true,
          connectionStatus: "connected",
          capabilities: { ...writableCapabilities, freshness: "webhook" },
          fetchedAt: "2026-06-19T22:00:00.000Z",
        },
      ],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingProviders>);

    renderDashboard();

    await waitFor(() => {
      expect(ticketingHooks.useTickets).toHaveBeenLastCalledWith(
        expect.objectContaining({ provider: "linear" }),
        { enabled: true },
      );
    });
    expect(screen.queryByRole("tablist", { name: "Ticketing provider" })).not.toBeInTheDocument();
  });

  it("surfaces stale provider freshness without hiding ticket content", () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [
        {
          provider: "jira",
          label: "Jira",
          enabled: true,
          connectionStatus: "connected",
          capabilities: writableCapabilities,
          fetchedAt: "2026-06-19T21:00:00.000Z",
          staleAt: "2026-06-19T22:00:00.000Z",
        },
      ],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingProviders>);

    renderDashboard();

    expect(screen.getByRole("status")).toHaveTextContent("Jira data is stale");
    expect(screen.getByRole("button", { name: /RX-1/ })).toBeInTheDocument();
  });

  it("shows metadata refresh failures without blanking tickets", () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTicketingContainers).mockReturnValue({
      data: [{ provider: "jira", id: "board-1", name: "Sprint Board", kind: "board" }],
      isLoading: false,
      isError: true,
      error: new Error("Rate limit exceeded"),
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);

    renderDashboard();

    expect(screen.getByRole("status")).toHaveTextContent("Ticket containers failed to refresh");
    expect(screen.getByText("Rate limit exceeded")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /RX-1/ })).toBeInTheDocument();
  });

  it("shows ticket refresh failures without blanking cached tickets", () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: {
        pages: [{ items: [ticket], nextCursor: null, total: 1 }],
        pageParams: [null],
      },
      isLoading: false,
      isFetching: false,
      isError: true,
      error: new Error("Provider rate limit exceeded"),
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);

    renderDashboard();

    expect(screen.getByRole("status")).toHaveTextContent("Tickets failed to refresh");
    expect(screen.getByText("Provider rate limit exceeded")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /RX-1/ })).toBeInTheDocument();
  });

  it("renders list and kanban views, then opens ticket detail with RalphX associations", async () => {
    mockConnectedDashboard();

    renderDashboard();

    expect(screen.getByRole("button", { name: /RX-1/ })).toBeInTheDocument();
    expect(screen.getByText("Fix merge race in transition handler")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Kanban view" }));

    const column = await screen.findByTestId("ticket-column-todo");
    expect(within(column).getByText("To Do")).toBeInTheDocument();
    expect(within(column).getByText("RX-1")).toBeInTheDocument();

    fireEvent.click(within(column).getByRole("button", { name: /Fix merge race/ }));

    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /RX-1/ })).toBeInTheDocument();
    expect(screen.getByText("RalphX Work")).toBeInTheDocument();
    expect(await screen.findByText("Fix merge race")).toBeInTheDocument();
  });

  it("renders ticket description and comments as markdown in the detail sheet", async () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTicketDetail).mockReturnValue({
      data: {
        ...ticket,
        descriptionMarkdown: "Investigate **transition race**\n\n- Preserve ordering\n- Avoid duplicate writes",
        comments: [
          {
            id: "comment-1",
            bodyMarkdown: "Reviewer said `retry` is ready.",
            bodyText: "",
            author: { id: "user-1", name: "A. Reviewer" },
            createdAt: "2026-06-19T22:00:01.000Z",
          },
        ],
        attachments: [],
        transitions: [],
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketDetail>);

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));

    const strong = await screen.findByText("transition race");
    expect(strong.tagName.toLowerCase()).toBe("strong");
    expect(screen.getByRole("list")).toHaveTextContent("Preserve ordering");
    const code = screen.getByText("retry");
    expect(code.tagName.toLowerCase()).toBe("code");
  });

  it("passes changed filters to the ticket query", async () => {
    mockConnectedDashboard();

    renderDashboard();

    fireEvent.change(screen.getByLabelText("Search tickets"), {
      target: { value: "merge" },
    });
    await waitFor(() => {
      expect(ticketingHooks.useTickets).toHaveBeenLastCalledWith(
        expect.objectContaining({
          filters: expect.objectContaining({ text: "merge" }),
        }),
        { enabled: true },
      );
    });

    fireEvent.change(screen.getByLabelText("Status"), {
      target: { value: "todo" },
    });
    await waitFor(() => {
      expect(ticketingHooks.useTickets).toHaveBeenLastCalledWith(
        expect.objectContaining({
          filters: expect.objectContaining({
            text: "merge",
            stateIds: ["todo"],
          }),
        }),
        { enabled: true },
      );
    });
  });

  it("loads status options from provider and ticket statuses", async () => {
    mockConnectedDashboard();
    const blockedTicket = {
      ...ticket,
      state: { id: "blocked", name: "Blocked", category: "other" as const },
    };
    vi.mocked(ticketingHooks.useTicketingColumns).mockReturnValue({
      data: [{ id: "todo", name: "To Do", category: "todo", order: 0 }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingColumns>);
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: {
        pages: [{ items: [blockedTicket], nextCursor: null, total: 1 }],
        pageParams: [null],
      },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);

    renderDashboard();

    expect(screen.getByRole("option", { name: "Blocked" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "To Do" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Kanban view" }));

    expect(await screen.findByTestId("ticket-column-blocked")).toBeInTheDocument();
  });

  it("renders assignee, RX association count, and provider updated time from ticket rows", () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: {
        pages: [{
          items: [{
            ...ticket,
            assignee: { id: "user-1", name: "A. User" },
            labels: ["backend", "linear"],
            project: "Platform",
            associationCount: 3,
            openPrCount: 0,
            updatedAt: "2026-06-18T08:15:00.000Z",
          }],
          nextCursor: null,
          total: 1,
        }],
        pageParams: [null],
      },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);

    renderDashboard();

    // Scope to the ticket list so filter-bar options (Unassigned / assignee
    // names) do not collide with row content.
    const list = within(document.querySelector("[data-ticket-list]") as HTMLElement);
    expect(list.getByText("A. User")).toBeInTheDocument();
    expect(list.getByText("Platform")).toBeInTheDocument();
    expect(list.getByText("linear")).toBeInTheDocument();
    expect(list.getByRole("img", { name: /3 RalphX conversation/i })).toBeInTheDocument();
    expect(list.queryByText("Unassigned")).not.toBeInTheDocument();
  });

  it("filters the list client-side by the selected assignee", () => {
    mockConnectedDashboard();
    useTicketingStore.setState({
      filters: { text: "", assignee: "Someone Else", stateIds: [], labels: [] },
    });
    renderDashboard();

    // The only ticket (RX-1) is unassigned, so the named-assignee filter empties the list.
    expect(screen.getByText("No tickets match these filters")).toBeInTheDocument();
  });

  it("shows a non-filter empty state when there are no tickets and no active filters", () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: { pages: [{ items: [], nextCursor: null, total: 0 }], pageParams: [null] },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);
    useTicketingStore.setState({
      filters: { text: "", assignee: null, stateIds: [], labels: [] },
    });
    renderDashboard();

    expect(screen.getByText("No tickets here yet")).toBeInTheDocument();
    expect(screen.queryByText("No tickets match these filters")).not.toBeInTheDocument();
  });

  it("shows the detail preloader instead of stale content until the detail matches", async () => {
    mockConnectedDashboard();
    // Detail has not resolved for the opened ticket yet (no matching data).
    vi.mocked(ticketingHooks.useTicketDetail).mockReturnValue({
      data: undefined,
      isLoading: true,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useTicketDetail>);

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));

    const skeletons = await screen.findAllByRole("status", { name: /loading ticket details/i });
    expect(skeletons.length).toBeGreaterThan(0);
    // The (mocked) detail body is not shown while pending.
    expect(
      screen.queryByText("When two agents transition the same task."),
    ).not.toBeInTheDocument();
  });

  it("records a ticket as opened when its row is clicked", () => {
    mockConnectedDashboard();
    renderDashboard();

    expect(useTicketingStore.getState().lastOpenedAt["jira:10001"]).toBeUndefined();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));
    expect(useTicketingStore.getState().lastOpenedAt["jira:10001"]).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });

  it("assigns a ticket to me from the list row without opening the detail overlay", () => {
    mockConnectedDashboard();
    const assignToMe = vi.fn().mockResolvedValue(undefined);
    vi.mocked(ticketingHooks.useTicketingMutations).mockReturnValue({
      transitionStatus: vi.fn(),
      assignToMe,
      clearAssignee: vi.fn(),
      addComment: vi.fn(),
      transitionStatusMutation: { isPending: false },
      assignToMeMutation: { isPending: false },
      clearAssigneeMutation: { isPending: false },
      addCommentMutation: { isPending: false },
      setLabelsMutation: { isPending: false },
    } as unknown as ReturnType<typeof ticketingHooks.useTicketingMutations>);

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: "Assign to me" }));

    expect(assignToMe).toHaveBeenCalledWith({
      provider: "jira",
      ticketRef: ticket.ref,
      projectId: "project-1",
    });
    // The quick action does not open the detail overlay.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("flags a row as updated when it changed since the last open", () => {
    mockConnectedDashboard();
    useTicketingStore.setState({
      lastOpenedAt: { "jira:10001": "2020-01-01T00:00:00.000Z" },
    });
    renderDashboard();

    expect(
      screen.getByRole("img", { name: /updated since you last opened/i }),
    ).toBeInTheDocument();
  });

  it("opens ticket detail when a kanban card is clicked", async () => {
    mockConnectedDashboard();
    useTicketingStore.getState().setViewMode("kanban");

    renderDashboard();

    fireEvent.click(await screen.findByRole("button", { name: /RX-1/ }));

    expect(await screen.findByRole("button", { name: "Start RalphX work" })).toBeInTheDocument();
    expect(screen.getByText("When two agents transition the same task.")).toBeInTheDocument();
  });

  it("opens the detail shell before hydrating association queries", () => {
    mockConnectedDashboard();

    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    const associationCalls = vi.mocked(ticketingHooks.useTicketAssociations).mock.calls;
    expect(associationCalls[associationCalls.length - 1]?.[0]).toBeNull();
  });

  it("routes status, assignment, comment, and comments jump controls through ticket mutations", async () => {
    mockConnectedDashboard();
    const transitionStatus = vi.fn().mockResolvedValue(undefined);
    const assignToMe = vi.fn().mockResolvedValue(undefined);
    const clearAssignee = vi.fn().mockResolvedValue(undefined);
    const addComment = vi.fn().mockResolvedValue(undefined);
    vi.mocked(ticketingHooks.useTicketDetail).mockReturnValue({
      data: {
        ...ticket,
        descriptionMarkdown: "When two agents transition the same task.",
        comments: [],
        attachments: [],
        transitions: [
          {
            toStateId: "done",
            providerTransitionId: "transition-31",
            name: "Done",
            category: "done",
          },
        ],
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketDetail>);
    vi.mocked(ticketingHooks.useTicketingMutations).mockReturnValue({
      transitionStatus,
      assignToMe,
      clearAssignee,
      addComment,
      transitionStatusMutation: { isPending: false },
      assignToMeMutation: { isPending: false },
      clearAssigneeMutation: { isPending: false },
      addCommentMutation: { isPending: false },
      setLabelsMutation: { isPending: false },
    } as unknown as ReturnType<typeof ticketingHooks.useTicketingMutations>);

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));

    fireEvent.change(await screen.findByRole("combobox", { name: "Ticket status" }), {
      target: { value: "done" },
    });
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Assign to me" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Ticket comment" }), {
      target: { value: "Pushed a fix." },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add comment" }));
    fireEvent.click(screen.getByRole("button", { name: "Comments (1)" }));

    expect(await screen.findByText("Pushed a fix.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Comments (1)" })).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "Ticket comment" })).toHaveValue("");
    });
    expect(transitionStatus).toHaveBeenCalledWith({
      provider: "jira",
      ticketRef: ticket.ref,
      projectId: "project-1",
      transition: {
        toStateId: "done",
        providerTransitionId: "transition-31",
        name: "Done",
        category: "done",
      },
    });
    expect(assignToMe).toHaveBeenCalledWith({
      provider: "jira",
      ticketRef: ticket.ref,
      projectId: "project-1",
    });
    expect(addComment).toHaveBeenCalledWith({
      provider: "jira",
      ticketRef: ticket.ref,
      bodyMarkdown: "Pushed a fix.",
      projectId: "project-1",
    });
    // "Assign to me" is only offered while unassigned, so "Clear assignee" is absent here.
    expect(clearAssignee).not.toHaveBeenCalled();
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalledWith({
      behavior: "smooth",
      block: "start",
    });
  });

  it("clears the assignee from the detail sheet when one is set", async () => {
    mockConnectedDashboard();
    const clearAssignee = vi.fn().mockResolvedValue(undefined);
    vi.mocked(ticketingHooks.useTicketDetail).mockReturnValue({
      data: {
        ...ticket,
        assignee: { id: "user-1", name: "A. User" },
        descriptionMarkdown: "Already assigned.",
        comments: [],
        attachments: [],
        transitions: [],
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketDetail>);
    vi.mocked(ticketingHooks.useTicketingMutations).mockReturnValue({
      transitionStatus: vi.fn(),
      assignToMe: vi.fn(),
      clearAssignee,
      addComment: vi.fn(),
      transitionStatusMutation: { isPending: false },
      assignToMeMutation: { isPending: false },
      clearAssigneeMutation: { isPending: false },
      addCommentMutation: { isPending: false },
      setLabelsMutation: { isPending: false },
    } as unknown as ReturnType<typeof ticketingHooks.useTicketingMutations>);

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));

    // Assignee is shown and "Assign to me" is hidden in the sheet when assigned.
    const sheet = within(screen.getByRole("dialog"));
    expect(await screen.findByText("A. User")).toBeInTheDocument();
    expect(sheet.queryByRole("button", { name: "Assign to me" })).not.toBeInTheDocument();

    fireEvent.click(sheet.getByRole("button", { name: "Clear assignee" }));
    expect(clearAssignee).toHaveBeenCalledWith({
      provider: "jira",
      ticketRef: ticket.ref,
      projectId: "project-1",
    });
  });

  it("starts RalphX work from the ticket detail sheet", async () => {
    const startWork = vi.fn();
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useStartWorkFromTicket).mockReturnValue({
      mutate: startWork,
      isPending: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useStartWorkFromTicket>);

    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Start RalphX work" }),
    );
    expect(await screen.findByRole("dialog")).toHaveTextContent("Start RalphX Work");

    fireEvent.change(screen.getByLabelText("Project"), {
      target: { value: "project-2" },
    });
    fireEvent.change(screen.getByLabelText("Conversation type"), {
      target: { value: "plan" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));

    expect(startWork).toHaveBeenCalledWith(
      {
        projectId: "project-2",
        ticketRef: ticket.ref,
        mode: "plan",
        content: "Start RalphX work for RX-1: Fix merge race in transition handler",
      },
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
  });

  it("applies the unified md select treatment to the Start Work dialog pickers", async () => {
    mockConnectedDashboard();

    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Start RalphX work" }),
    );
    expect(await screen.findByRole("dialog")).toHaveTextContent("Start RalphX Work");

    const project = screen.getByRole("combobox", { name: "Project" });
    const conversationType = screen.getByRole("combobox", { name: "Conversation type" });

    for (const select of [project, conversationType]) {
      expect(select.className).toContain("h-9");
      expect(select.className).toContain("appearance-none");
      expect((select as HTMLSelectElement).style.backgroundColor).toBe("var(--bg-elevated)");
    }
  });

  it("binds an existing conversation to the ticket via the provider-correct assign API", async () => {
    mockConnectedDashboard();
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [
        { id: "conv-9", title: "Pair on the merge race" },
        { id: "conv-10", title: "Unrelated thread" },
      ],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));

    fireEvent.click(
      await screen.findByRole("button", { name: "Bind existing conversation" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Pair on the merge race" }));

    await waitFor(() => {
      expect(atlassianApi.assignAgentConversationJiraIssue).toHaveBeenCalledWith(
        expect.objectContaining({
          conversationId: "conv-9",
          projectId: "project-1",
          issueId: "10001",
          issueKey: "RX-1",
          title: "Fix merge race in transition handler",
          issueUrl: "https://example.atlassian.net/browse/RX-1",
          refresh: true,
        }),
      );
    });
    // The Jira ticket must not route through the Linear assign API.
    expect(linearApi.assignAgentConversationLinearIssue).not.toHaveBeenCalled();
  });
});
