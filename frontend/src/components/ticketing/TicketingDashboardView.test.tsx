import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as ticketingHooks from "@/hooks/useTicketing";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useTicketingStore } from "@/stores/ticketingStore";

import { TicketingDashboardView } from "./TicketingDashboardView";

vi.mock("@/hooks/useTicketing", () => ({
  flattenTicketPages: vi.fn((data) => data?.pages.flatMap((page) => page.items) ?? []),
  useRefreshTickets: vi.fn(),
  useTicketAssociations: vi.fn(),
  useTicketDetail: vi.fn(),
  useTicketingColumns: vi.fn(),
  useTicketingContainers: vi.fn(),
  useTicketingProviders: vi.fn(),
  useTicketTransitions: vi.fn(),
  useTickets: vi.fn(),
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

const ticket = {
  ref: { provider: "jira" as const, id: "10001", key: "RX-1" },
  title: "Fix merge race in transition handler",
  state: { id: "todo", name: "To Do", category: "todo" as const },
  labels: ["backend"],
  updatedAt: "2026-06-19T22:00:00.000Z",
  url: "https://example.atlassian.net/browse/RX-1",
  associationCount: 2,
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
        capabilities,
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
      transitions: [],
    },
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
}

describe("TicketingDashboardView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useTicketingStore.getState().reset();
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

  it("opens the detail shell before hydrating association queries", () => {
    mockConnectedDashboard();

    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    const associationCalls = vi.mocked(ticketingHooks.useTicketAssociations).mock.calls;
    expect(associationCalls[associationCalls.length - 1]?.[0]).toBeNull();
  });
});
