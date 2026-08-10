import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement, type ComponentProps, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { atlassianApi } from "@/api/atlassian";
import { linearApi } from "@/api/linear";
import * as chatHooks from "@/hooks/useChat";
import { usePullRequestDetail } from "@/hooks/usePullRequestDetail";
import * as ticketingHooks from "@/hooks/useTicketing";
import * as projectHooks from "@/hooks/useProjects";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useTicketingStore } from "@/stores/ticketingStore";

import { TicketingDashboardView } from "./TicketingDashboardView";

const { toastErrorMock } = vi.hoisted(() => ({ toastErrorMock: vi.fn() }));

vi.mock("sonner", () => ({ toast: { error: toastErrorMock } }));

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

vi.mock("@/hooks/usePullRequestDetail", () => ({
  prKeys: {
    all: ["github-pr"],
    detail: (selector: { projectId: string; prNumber?: number; branch?: string }) => [
      "github-pr",
      "detail",
      selector.projectId,
      selector.prNumber ?? null,
      selector.branch ?? null,
    ],
  },
  usePullRequestDetail: vi.fn(),
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
  useRefreshTicketingStatusCatalog: vi.fn(),
  useTicketAssociations: vi.fn(),
  useTicketDetail: vi.fn(),
  useTicketingMutations: vi.fn(),
  useTicketingColumns: vi.fn(),
  useTicketingContainers: vi.fn(),
  useTicketingProviders: vi.fn(),
  useTicketingStatusCatalog: vi.fn(),
  useTicketFilterOptions: vi.fn(),
  useTicketLabelOptions: vi.fn(),
  useTicketTransitions: vi.fn(),
  useTickets: vi.fn(),
  useUpdateTicketingStatusPresentation: vi.fn(),
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

function renderDashboard(props: {
  onNavigateToAssociation?: ComponentProps<
    typeof TicketingDashboardView
  >["onNavigateToAssociation"];
} = {}) {
  const Wrapper = createWrapper();
  return render(
    <TicketingDashboardView
      projectId="project-1"
      onNavigateToAssociation={props.onNavigateToAssociation}
    />,
    {
      wrapper: Wrapper,
    },
  );
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
    data: [{ provider: "jira", id: "RX", key: "RX", name: "RalphX", kind: "project" }],
    isLoading: false,
    isError: false,
    error: null,
  } as ReturnType<typeof ticketingHooks.useTicketingContainers>);
  // With containers present, the dashboard forces a container selection before
  // loading tickets/columns; pre-select one so the connected-state assertions see
  // tickets. Set the provider first (otherwise the auto-provider effect would
  // clear the selection) then the container.
  useTicketingStore.getState().setProvider("jira");
  useTicketingStore.getState().setContainerId("RX");
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
  vi.mocked(ticketingHooks.useTicketFilterOptions).mockReturnValue({
    data: {
      assignees: ["A. Dev"],
      sprints: [],
      complete: true,
      truncated: false,
    },
    isLoading: false,
    isError: false,
    error: null,
  } as ReturnType<typeof ticketingHooks.useTicketFilterOptions>);
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
}

describe("TicketingDashboardView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.HTMLElement.prototype.scrollIntoView = vi.fn();
    useTicketingStore.getState().reset();
    useAgentSessionStore.getState().clearSelection();
    useAgentSessionStore.setState({ startConversationDraft: null });
    useChatStore.setState({ activeConversationIds: {} });
    vi.mocked(usePullRequestDetail).mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
      error: null,
      fetchStatus: "idle",
    } as unknown as ReturnType<typeof usePullRequestDetail>);
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
    vi.mocked(ticketingHooks.useTicketFilterOptions).mockReturnValue({
      data: {
        assignees: [],
        sprints: [],
        complete: true,
        truncated: false,
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketFilterOptions>);
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
    vi.mocked(ticketingHooks.useTicketingStatusCatalog).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useTicketingStatusCatalog>);
    vi.mocked(ticketingHooks.useRefreshTicketingStatusCatalog).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useRefreshTicketingStatusCatalog>);
    vi.mocked(ticketingHooks.useUpdateTicketingStatusPresentation).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useUpdateTicketingStatusPresentation>);
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
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);
  });

  it("renders a no-valid-integration state without blanking the shell", () => {
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
    expect(screen.getByText("No valid ticketing integration")).toBeInTheDocument();
    expect(
      screen.getByText("Connect a valid Jira, Linear, or ClickUp integration from Settings to browse tickets."),
    ).toBeInTheDocument();
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

    expect(screen.getByText("No valid ticketing integration")).toBeInTheDocument();
    expect(ticketingHooks.useTickets).toHaveBeenCalledWith(null, { enabled: false });
  });

  it("forces a project selection and skips ticket/column queries when containers exist but none is selected", () => {
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
    // Readable provider exposes a project container, but nothing is selected.
    vi.mocked(ticketingHooks.useTicketingContainers).mockReturnValue({
      data: [{ provider: "jira", id: "RX", key: "RX", name: "RalphX", kind: "project" }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);

    renderDashboard();

    // The "Select a project" prompt renders (Jira containers are projects).
    expect(screen.getByText("Select a project")).toBeInTheDocument();
    expect(
      screen.getByText("Choose a project to load its tickets and statuses."),
    ).toBeInTheDocument();
    // No provider-wide unfiltered fetch fires for tickets or columns.
    expect(ticketingHooks.useTickets).toHaveBeenCalledWith(null, { enabled: false });
    expect(ticketingHooks.useTicketingColumns).toHaveBeenCalledWith(null, { enabled: false });
    // And no statuses are offered in the filter until a project is selected
    // (the remembered last-non-empty columns must not leak prior statuses).
    fireEvent.click(screen.getByRole("combobox", { name: "Status" }));
    expect(
      within(screen.getByRole("listbox", { name: "Status" })).getAllByRole("option"),
    ).toHaveLength(1);
  });

  it("loads tickets and columns with the containerId once a project is selected", async () => {
    mockConnectedDashboard();

    renderDashboard();

    // mockConnectedDashboard pre-selects the "RX" project; both queries fire with
    // the containerId and the project's tickets render.
    await waitFor(() => {
      expect(ticketingHooks.useTickets).toHaveBeenLastCalledWith(
        expect.objectContaining({ provider: "jira", containerId: "RX" }),
        { enabled: true },
      );
    });
    expect(ticketingHooks.useTicketingColumns).toHaveBeenLastCalledWith(
      expect.objectContaining({ provider: "jira", containerId: "RX" }),
      { enabled: true },
    );
    expect(screen.getByRole("button", { name: /RX-1/ })).toBeInTheDocument();
    expect(screen.getByText("Fix merge race in transition handler")).toBeInTheDocument();
  });

  it("opens scoped status management from the header and syncs the selected project", async () => {
    const syncStatuses = vi.fn();
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTicketingStatusCatalog).mockReturnValue({
      data: [
        {
          id: "catalog-1",
          provider: "jira",
          scopeKind: "jira_project",
          scopeId: "RX",
          providerStatusId: "todo",
          providerStatusName: "To Do",
          providerCategory: "todo",
          providerColor: null,
          providerOrder: 0,
          displayOrder: 0,
          colorOverride: null,
          color: null,
          isVisible: true,
          isTerminal: false,
          stale: false,
          lastSeenAt: "2026-06-19T22:00:00.000Z",
          staleSince: null,
          updatedAt: "2026-06-19T22:00:00.000Z",
        },
      ],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useTicketingStatusCatalog>);
    vi.mocked(ticketingHooks.useRefreshTicketingStatusCatalog).mockReturnValue({
      mutate: syncStatuses,
      isPending: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useRefreshTicketingStatusCatalog>);

    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: "Statuses" }));

    const dialog = screen.getByRole("dialog");
    expect(dialog).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Statuses" })).toBeInTheDocument();
    expect(within(dialog).getByText("To Do")).toBeInTheDocument();
    expect(within(dialog).getByLabelText("1 ticket in To Do")).toBeInTheDocument();
    await waitFor(() => {
      expect(syncStatuses).toHaveBeenCalledWith({
        provider: "jira",
        scopeKind: "jira_project",
        scopeId: "RX",
      });
    });
  });

  it("keeps GitHub and Granola out of the ticketing dashboard", () => {
    mockConnectedDashboard();

    renderDashboard();

    expect(screen.queryByRole("tablist", { name: "Ticketing content" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Tickets" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Granola" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("ticketing-github-branches")).not.toBeInTheDocument();
    expect(screen.queryByTestId("granola-dashboard-view")).not.toBeInTheDocument();
  });

  it("auto-selects the only valid provider and hides provider tabs", async () => {
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

  it("auto-loads Linear tickets without forcing a project pick when Linear is the only enabled provider", async () => {
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
    // Linear exposes projects as containers, but with a single enabled provider the
    // dashboard must auto-load rather than gating behind a project selection.
    vi.mocked(ticketingHooks.useTicketingContainers).mockReturnValue({
      data: [{ provider: "linear", id: "proj-1", key: null, name: "Roadmap", kind: "project" }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);

    renderDashboard();

    await waitFor(() => {
      expect(ticketingHooks.useTickets).toHaveBeenLastCalledWith(
        expect.objectContaining({ provider: "linear" }),
        { enabled: true },
      );
    });
    expect(screen.queryByText("Select a project")).not.toBeInTheDocument();
  });

  it("forces a Space selection before loading ClickUp tickets when ClickUp locations exist", async () => {
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
          enabled: false,
          connectionStatus: "disconnected",
          capabilities,
        },
        {
          provider: "clickup",
          label: "ClickUp",
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
      data: [{ provider: "clickup", id: "space-1", key: null, name: "Engineering", kind: "space" }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);

    renderDashboard();

    expect(screen.getByText("Select a space")).toBeInTheDocument();
    expect(ticketingHooks.useTicketingColumns).toHaveBeenCalledWith(null, { enabled: false });
    expect(ticketingHooks.useTickets).toHaveBeenCalledWith(null, { enabled: false });
  });

  it("forwards ClickUp sprint filters without changing the selected Space scope", async () => {
    useTicketingStore.getState().setProvider("clickup");
    useTicketingStore.getState().setContainerId("space-sprints");
    const sprintTicket = {
      ...ticket,
      ref: { provider: "clickup" as const, id: "cu-current", key: "CU-42" },
      title: "Current sprint task",
      project: "Continuous Improvement",
      sprints: ["Sprint 42"],
      currentUserAssigned: true,
    };
    const backlogTicket = {
      ...ticket,
      ref: { provider: "clickup" as const, id: "cu-backlog", key: "CU-7" },
      title: "Backlog task",
      project: "Backlog",
      sprints: ["Backlog Sprint"],
      currentUserAssigned: false,
    };
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [
        {
          provider: "clickup",
          label: "ClickUp",
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
    vi.mocked(ticketingHooks.useTicketingContainers).mockImplementation((input) => ({
      data: input?.parentContainerId
        ? [{ provider: "clickup", id: "list:sprint-42", key: "List", name: "Sprint 42", kind: "list", parentId: "space-sprints" }]
        : [{ provider: "clickup", id: "space-sprints", key: "Space", name: "Sprints", kind: "space" }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>));
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: { pages: [{ items: [sprintTicket, backlogTicket], nextCursor: null, total: 2 }], pageParams: [null] },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);
    vi.mocked(ticketingHooks.useTicketFilterOptions).mockReturnValue({
      data: {
        assignees: [],
        sprints: ["Sprint 42"],
        complete: true,
        truncated: false,
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketFilterOptions>);

    renderDashboard();

    const sprintSelect = await screen.findByRole("combobox", { name: "Sprint" });
    fireEvent.click(sprintSelect);
    const sprintListbox = screen.getByRole("listbox", { name: "Sprint" });
    expect(within(sprintListbox).getByRole("option", { name: "Sprint 42" })).toBeInTheDocument();
    expect(within(sprintListbox).queryByRole("option", { name: "Backlog" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Current sprint task/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Backlog task/ })).toBeInTheDocument();

    fireEvent.click(within(sprintListbox).getByRole("option", { name: "Sprint 42" }));

    await waitFor(() => {
      expect(ticketingHooks.useTickets).toHaveBeenLastCalledWith(
        expect.objectContaining({
          containerId: "space-sprints",
          filters: expect.objectContaining({ sprint: "Sprint 42" }),
        }),
        { enabled: true },
      );
    });
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
      data: [{ provider: "jira", id: "RX", key: "RX", name: "RalphX", kind: "project" }],
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

  it("shows the visible ticket count beside the dashboard title", () => {
    mockConnectedDashboard();
    useTicketingStore.getState().setFilters({
      text: "",
      assignees: ["Ada"],
      stateIds: [],
      labels: [],
      sprint: null,
      watcherMe: false,
    });
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: {
        pages: [
          {
            items: [
              { ...ticket, assignee: { id: "ada", name: "Ada" } },
            ],
            nextCursor: null,
            total: 1,
          },
        ],
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

    expect(screen.getByRole("heading", { name: "Ticketing" })).toBeInTheDocument();
    expect(screen.getByTestId("ticketing-visible-count")).toHaveTextContent("1");
    expect(screen.getByTestId("ticketing-visible-count")).toHaveAttribute(
      "aria-label",
      "1 visible ticket",
    );
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

    fireEvent.click(screen.getByRole("combobox", { name: "Status" }));
    fireEvent.click(screen.getByRole("option", { name: "To Do" }));
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

    fireEvent.click(screen.getByRole("combobox", { name: "Status" }));
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
    expect(list.getByLabelText("A. User")).toHaveAttribute("title", "A. User");
    expect(list.queryByText("A. User")).not.toBeInTheDocument();
    expect(list.getByText("Platform")).toBeInTheDocument();
    expect(list.getByText("linear")).toBeInTheDocument();
    expect(list.getByRole("img", { name: /3 RalphX conversation/i })).toBeInTheDocument();
    expect(list.queryByText("Unassigned")).not.toBeInTheDocument();
  });

  it("forwards the selected assignee to the ticket query", () => {
    mockConnectedDashboard();
    useTicketingStore.setState({
      filters: { text: "", assignees: ["Someone Else"], stateIds: [], labels: [], sprint: null, watcherMe: false },
    });
    renderDashboard();

    expect(screen.getByRole("combobox", { name: "Assignee" })).toHaveTextContent("Someone Else");
    expect(ticketingHooks.useTickets).toHaveBeenLastCalledWith(
      expect.objectContaining({
        filters: expect.objectContaining({ assignees: ["Someone Else"] }),
      }),
      { enabled: true },
    );
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
      filters: { text: "", assignees: [], stateIds: [], labels: [], sprint: null, watcherMe: false },
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

  it("accepts ClickUp detail when a custom-id selection resolves to an opaque task id", async () => {
    const clickupTicket = {
      ...ticket,
      ref: { provider: "clickup" as const, id: "opaque-task-1", key: "TASK-123" },
      title: "Demo task",
      url: "https://app.clickup.com/t/workspace-1/TASK-123",
    };
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [
        {
          provider: "clickup",
          label: "ClickUp",
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
      data: [{ provider: "clickup", id: "space-1", key: null, name: "Engineering", kind: "space" }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);
    useTicketingStore.getState().setProvider("clickup");
    useTicketingStore.getState().setContainerId("space-1");
    useTicketingStore.getState().setSelectedTicketRef({
      provider: "clickup",
      id: "TASK-123",
    });
    vi.mocked(ticketingHooks.useTicketingColumns).mockReturnValue({
      data: [{ id: "todo", name: "To Do", category: "todo", order: 0 }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingColumns>);
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: { pages: [{ items: [clickupTicket], nextCursor: null, total: 1 }], pageParams: [null] },
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
        ...clickupTicket,
        descriptionMarkdown: "Custom id detail body.",
        comments: [],
        attachments: [],
        transitions: [],
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketDetail>);

    renderDashboard();

    expect(await screen.findByText("Custom id detail body.")).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: /loading ticket details/i })).not.toBeInTheDocument();
  });

  it("keeps waiting when loaded detail belongs to another provider", async () => {
    const clickupTicket = {
      ...ticket,
      ref: { provider: "clickup" as const, id: "opaque-task-1", key: "TASK-123" },
      title: "Demo task",
      url: "https://app.clickup.com/t/workspace-1/TASK-123",
    };
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [
        {
          provider: "clickup",
          label: "ClickUp",
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
      data: [{ provider: "clickup", id: "space-1", key: null, name: "Engineering", kind: "space" }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);
    useTicketingStore.getState().setProvider("clickup");
    useTicketingStore.getState().setContainerId("space-1");
    useTicketingStore.getState().setSelectedTicketRef({
      provider: "clickup",
      id: "TASK-123",
    });
    vi.mocked(ticketingHooks.useTicketingColumns).mockReturnValue({
      data: [{ id: "todo", name: "To Do", category: "todo", order: 0 }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingColumns>);
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: { pages: [{ items: [clickupTicket], nextCursor: null, total: 1 }], pageParams: [null] },
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
        descriptionMarkdown: "Wrong provider detail body.",
        comments: [],
        attachments: [],
        transitions: [],
      },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketDetail>);

    renderDashboard();

    const skeletons = await screen.findAllByRole("status", { name: /loading ticket details/i });
    expect(skeletons.length).toBeGreaterThan(0);
    expect(screen.queryByText("Wrong provider detail body.")).not.toBeInTheDocument();
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

  it("surfaces a failed quick assignment", async () => {
    mockConnectedDashboard();
    const assignToMe = vi.fn().mockRejectedValue(new Error("Assignment rejected"));
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

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledWith("Assignment rejected"));
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

    expect(await screen.findByRole("button", { name: "Start conversation" })).toBeInTheDocument();
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

    fireEvent.click(await screen.findByRole("combobox", { name: "Ticket status" }));
    fireEvent.click(screen.getByRole("option", { name: "Done" }));
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
    expect(await sheet.findByLabelText("A. User")).toHaveAttribute("title", "A. User");
    expect(sheet.queryByText("A. User")).not.toBeInTheDocument();
    expect(sheet.queryByRole("button", { name: "Assign to me" })).not.toBeInTheDocument();

    fireEvent.click(sheet.getByRole("button", { name: "Clear assignee" }));
    expect(clearAssignee).toHaveBeenCalledWith({
      provider: "jira",
      ticketRef: ticket.ref,
      projectId: "project-1",
    });
  });

  it("opens the agent starter with the selected ticket as a composer reference", async () => {
    mockConnectedDashboard();

    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Start conversation" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("Start Conversation");
    expect(within(dialog).getByText("Start Conversation").parentElement).toHaveClass(
      "block",
      "space-y-1.5",
      "pr-14",
    );
    expect(dialog).toHaveTextContent(
      "The new composer will open with RX-1 attached as a reference.",
    );

    // Scope to the dialog: the container filter bar is also labeled "Project"
    // now that Jira containers are projects.
    fireEvent.click(within(dialog).getByRole("combobox", { name: "Project" }));
    fireEvent.click(screen.getByRole("option", { name: /Target Project.*repo\/target/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Open composer" }));

    expect(useAgentSessionStore.getState().startConversationDraft).toEqual({
      projectId: "project-2",
      content: "",
      mode: "edit",
      composerIntegrationReferences: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "RX-1",
          key: "RX-1",
          title: "Fix merge race in transition handler",
          url: "https://example.atlassian.net/browse/RX-1",
        },
      ],
    });
    expect(useAgentSessionStore.getState().focusedProjectId).toBe("project-2");
    expect(useAgentSessionStore.getState().selectedConversationId).toBeNull();
    expect(useChatStore.getState().activeConversationIds["project:project-2"]).toBeNull();
  });

  it("applies the unified md select treatment to the start conversation project picker", async () => {
    mockConnectedDashboard();

    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Start conversation" }),
    );
    expect(await screen.findByRole("dialog")).toHaveTextContent("Start Conversation");

    const project = screen.getByRole("combobox", { name: "Project" });
    expect(project.className).toContain("h-9");
    expect(project.className).toContain("appearance-none");
    expect((project as HTMLElement).style.backgroundColor).toBe("var(--bg-elevated)");
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

  it("renders the provider loading state", () => {
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: undefined,
      isLoading: true,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingProviders>);

    renderDashboard();

    expect(screen.getByText("Loading ticketing providers")).toBeInTheDocument();
  });

  it("renders the provider load-error state with the error message", () => {
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: true,
      error: new Error("Provider catalog unavailable"),
    } as ReturnType<typeof ticketingHooks.useTicketingProviders>);

    renderDashboard();

    expect(screen.getByText("Ticketing providers failed to load")).toBeInTheDocument();
    expect(screen.getByText("Provider catalog unavailable")).toBeInTheDocument();
  });

  it("renders the no-providers empty state", () => {
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingProviders>);

    renderDashboard();

    expect(screen.getByText("No ticketing providers available")).toBeInTheDocument();
  });

  it("does not select providers with errored connections", () => {
    useTicketingStore.getState().setProvider("jira");
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [
        {
          provider: "jira",
          label: "Jira",
          enabled: true,
          connectionStatus: "error",
          capabilities,
          errorMessage: "Token expired.",
        },
      ],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingProviders>);

    renderDashboard();

    expect(screen.getByText("No valid ticketing integration")).toBeInTheDocument();
    expect(ticketingHooks.useTickets).toHaveBeenCalledWith(null, { enabled: false });
  });

  it("renders the tickets loading state while data hydrates", () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: undefined,
      isLoading: true,
      isFetching: true,
      isError: false,
      error: null,
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);

    renderDashboard();

    expect(screen.getByText("Loading tickets")).toBeInTheDocument();
  });

  it("renders a full error panel with a Refresh action when tickets fail and none are cached", () => {
    mockConnectedDashboard();
    const refreshMutate = vi.fn();
    vi.mocked(ticketingHooks.useRefreshTickets).mockReturnValue({
      mutate: refreshMutate,
      isPending: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useRefreshTickets>);
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: { pages: [{ items: [], nextCursor: null, total: 0 }], pageParams: [null] },
      isLoading: false,
      isFetching: false,
      isError: true,
      error: new Error("Tickets endpoint down"),
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);

    renderDashboard();

    expect(screen.getByText("Tickets failed to load")).toBeInTheDocument();
    expect(screen.getByText("Tickets endpoint down")).toBeInTheDocument();
    // The panel's Refresh action routes through the manual refresh mutation.
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(refreshMutate).toHaveBeenCalledWith({ provider: "jira", containerId: "RX" });
  });

  it("surfaces a manual-refresh failure as an error notice", () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useRefreshTickets).mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
      isError: true,
      error: new Error("Refresh rejected"),
    } as unknown as ReturnType<typeof ticketingHooks.useRefreshTickets>);

    renderDashboard();

    expect(screen.getByRole("status")).toHaveTextContent("Manual refresh failed");
    expect(screen.getByText("Refresh rejected")).toBeInTheDocument();
  });

  it("triggers a manual refresh with the active provider and container", () => {
    mockConnectedDashboard();
    const refreshMutate = vi.fn();
    vi.mocked(ticketingHooks.useRefreshTickets).mockReturnValue({
      mutate: refreshMutate,
      isPending: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useRefreshTickets>);

    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: /refresh/i }));
    expect(refreshMutate).toHaveBeenCalledWith({ provider: "jira", containerId: "RX" });
  });

  it("surfaces a column-refresh failure as a warning notice", () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTicketingColumns).mockReturnValue({
      data: [{ id: "todo", name: "To Do", category: "todo", order: 0 }],
      isLoading: false,
      isError: true,
      error: new Error("Columns endpoint down"),
    } as ReturnType<typeof ticketingHooks.useTicketingColumns>);

    renderDashboard();

    expect(screen.getByRole("status")).toHaveTextContent("Ticket statuses failed to refresh");
    expect(screen.getByText("Columns endpoint down")).toBeInTheDocument();
  });

  it("moves a ticket through the resolved transition from the list status control", async () => {
    mockConnectedDashboard();
    const transition = {
      toStateId: "done",
      providerTransitionId: "transition-31",
      name: "Done",
      category: "done" as const,
    };
    vi.mocked(ticketingHooks.fetchTicketTransitionsForMove).mockResolvedValue([transition]);
    vi.mocked(ticketingHooks.findTicketTransitionForColumn).mockReturnValue(transition);
    const transitionStatus = vi.fn().mockResolvedValue(undefined);
    vi.mocked(ticketingHooks.useTicketingMutations).mockReturnValue({
      transitionStatus,
      assignToMe: vi.fn(),
      clearAssignee: vi.fn(),
      addComment: vi.fn(),
      setLabels: vi.fn(),
      transitionStatusMutation: { isPending: false },
      assignToMeMutation: { isPending: false },
      clearAssigneeMutation: { isPending: false },
      addCommentMutation: { isPending: false },
      setLabelsMutation: { isPending: false },
    } as unknown as ReturnType<typeof ticketingHooks.useTicketingMutations>);

    renderDashboard();

    // Open the per-row status control (writable provider) and pick a target column.
    // Radix DropdownMenu opens on pointerDown, not click.
    fireEvent.pointerDown(screen.getByRole("button", { name: /change status/i }), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(await screen.findByRole("menuitem", { name: /to do/i }));

    await waitFor(() => {
      expect(ticketingHooks.fetchTicketTransitionsForMove).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(transitionStatus).toHaveBeenCalledWith(
        expect.objectContaining({ provider: "jira", projectId: "project-1", transition }),
      );
    });
  });

  it("ignores a status move when no enabled transition resolves for the column", async () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.fetchTicketTransitionsForMove).mockResolvedValue([]);
    // No matching/enabled transition → handleMoveTicket short-circuits.
    vi.mocked(ticketingHooks.findTicketTransitionForColumn).mockReturnValue(null);
    const transitionStatus = vi.fn().mockResolvedValue(undefined);
    vi.mocked(ticketingHooks.useTicketingMutations).mockReturnValue({
      transitionStatus,
      assignToMe: vi.fn(),
      clearAssignee: vi.fn(),
      addComment: vi.fn(),
      setLabels: vi.fn(),
      transitionStatusMutation: { isPending: false },
      assignToMeMutation: { isPending: false },
      clearAssigneeMutation: { isPending: false },
      addCommentMutation: { isPending: false },
      setLabelsMutation: { isPending: false },
    } as unknown as ReturnType<typeof ticketingHooks.useTicketingMutations>);

    renderDashboard();

    fireEvent.pointerDown(screen.getByRole("button", { name: /change status/i }), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(await screen.findByRole("menuitem", { name: /to do/i }));

    await waitFor(() => {
      expect(ticketingHooks.findTicketTransitionForColumn).toHaveBeenCalled();
    });
    expect(transitionStatus).not.toHaveBeenCalled();
  });

  it("surfaces a failed ticket move", async () => {
    mockConnectedDashboard();
    const transition = {
      toStateId: "done",
      providerTransitionId: "transition-31",
      name: "Done",
      category: "done" as const,
    };
    vi.mocked(ticketingHooks.fetchTicketTransitionsForMove).mockResolvedValue([transition]);
    vi.mocked(ticketingHooks.findTicketTransitionForColumn).mockReturnValue(transition);
    const transitionStatus = vi.fn().mockRejectedValue(new Error("Move rejected"));
    vi.mocked(ticketingHooks.useTicketingMutations).mockReturnValue({
      transitionStatus,
      assignToMe: vi.fn(),
      clearAssignee: vi.fn(),
      addComment: vi.fn(),
      setLabels: vi.fn(),
      transitionStatusMutation: { isPending: false },
      assignToMeMutation: { isPending: false },
      clearAssigneeMutation: { isPending: false },
      addCommentMutation: { isPending: false },
      setLabelsMutation: { isPending: false },
    } as unknown as ReturnType<typeof ticketingHooks.useTicketingMutations>);

    renderDashboard();
    fireEvent.pointerDown(screen.getByRole("button", { name: /change status/i }), {
      button: 0,
      ctrlKey: false,
    });
    fireEvent.click(await screen.findByRole("menuitem", { name: /to do/i }));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledWith("Move rejected"));
  });

  it("refreshes with only the provider when no container is selected", () => {
    mockConnectedDashboard();
    // Drop back to the "All projects" selection so the refresh omits containerId.
    useTicketingStore.getState().setContainerId(null);
    // No containers means no forced selection gate, so content (and the refresh
    // affordance) still renders.
    vi.mocked(ticketingHooks.useTicketingContainers).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);
    const refreshMutate = vi.fn();
    vi.mocked(ticketingHooks.useRefreshTickets).mockReturnValue({
      mutate: refreshMutate,
      isPending: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof ticketingHooks.useRefreshTickets>);

    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: /refresh/i }));
    expect(refreshMutate).toHaveBeenCalledWith({ provider: "jira" });
  });

  it("binds an existing conversation to a Linear ticket through the Linear assign API", async () => {
    mockConnectedDashboard();
    useTicketingStore.getState().setProvider("linear");
    useTicketingStore.getState().setContainerId("TEAM");
    const linearTicket = {
      ...ticket,
      ref: { provider: "linear" as const, id: "LIN-1", key: "ENG-1" },
      // Linear filters tickets client-side by the selected container name, so the
      // row's project must match the active team container ("Engineering").
      project: "Engineering",
    };
    vi.mocked(ticketingHooks.useTicketingProviders).mockReturnValue({
      data: [
        {
          provider: "linear",
          label: "Linear",
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
      data: [{ provider: "linear", id: "TEAM", key: "TEAM", name: "Engineering", kind: "team" }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: { pages: [{ items: [linearTicket], nextCursor: null, total: 1 }], pageParams: [null] },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);
    vi.mocked(ticketingHooks.useTicketDetail).mockReturnValue({
      data: { ...linearTicket, descriptionMarkdown: "Linear ticket.", comments: [], attachments: [], transitions: [] },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketDetail>);
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [{ id: "conv-lin", title: "Linear pairing" }],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /ENG-1/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Bind existing conversation" }));
    fireEvent.click(screen.getByRole("button", { name: "Linear pairing" }));

    await waitFor(() => {
      expect(linearApi.assignAgentConversationLinearIssue).toHaveBeenCalledWith(
        expect.objectContaining({
          conversationId: "conv-lin",
          projectId: "project-1",
          issueId: "LIN-1",
          issueKey: "ENG-1",
          refresh: true,
        }),
      );
    });
    expect(atlassianApi.assignAgentConversationJiraIssue).not.toHaveBeenCalled();
  });

  it("surfaces ClickUp in the switcher, loads its tasks, and allows starting new RalphX work", async () => {
    mockConnectedDashboard();
    // Make ClickUp the active provider in a 3-provider dashboard so the switcher
    // renders a ClickUp tab (the switcher only shows when >1 provider is enabled).
    useTicketingStore.getState().setProvider("clickup");
    useTicketingStore.getState().setContainerId("space-eng");
    const clickupTicket = {
      ...ticket,
      ref: { provider: "clickup" as const, id: "cu-1001", key: "CU-1001" },
      title: "Demo ClickUp dashboard task",
      url: "https://app.clickup.com/t/CU-1001",
    };
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
        {
          provider: "linear",
          label: "Linear",
          enabled: true,
          connectionStatus: "connected",
          capabilities: { ...writableCapabilities, freshness: "webhook" },
          fetchedAt: "2026-06-19T22:00:00.000Z",
        },
        {
          provider: "clickup",
          label: "ClickUp",
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
    // ClickUp containers are Spaces; one is pre-selected so tasks load (ClickUp is
    // server-scoped like Jira, so the client container-name filter is skipped).
    vi.mocked(ticketingHooks.useTicketingContainers).mockReturnValue({
      data: [{ provider: "clickup", id: "space-eng", key: null, name: "Engineering", kind: "space" }],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingContainers>);
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: { pages: [{ items: [clickupTicket], nextCursor: null, total: 1 }], pageParams: [null] },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
      hasNextPage: false,
      fetchNextPage: vi.fn(),
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);
    vi.mocked(ticketingHooks.useTicketDetail).mockReturnValue({
      data: { ...clickupTicket, descriptionMarkdown: "ClickUp task.", comments: [], attachments: [], transitions: [] },
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketDetail>);

    renderDashboard();

    // ClickUp appears as a provider tab (labelled "ClickUp", not "Linear").
    expect(screen.getByRole("tab", { name: "ClickUp" })).toBeInTheDocument();
    // ClickUp tasks load and render their rows.
    expect(await screen.findByRole("button", { name: /CU-1001/ })).toBeInTheDocument();

    // Open the ticket detail.
    fireEvent.click(screen.getByRole("button", { name: /CU-1001/ }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();

    // Starting new RalphX work is provider-neutral; binding an existing
    // conversation stays hidden until ClickUp link persistence exists.
    fireEvent.click(screen.getByRole("button", { name: "Start conversation" }));
    expect(await screen.findByRole("dialog", { name: "Start Conversation" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open composer" }));
    expect(useAgentSessionStore.getState().startConversationDraft).toEqual({
      projectId: "project-1",
      content: "",
      mode: "edit",
      composerIntegrationReferences: [
        {
          provider: "clickup",
          kind: "clickup",
          id: "cu-1001",
          key: "CU-1001",
          title: "Demo ClickUp dashboard task",
          url: "https://app.clickup.com/t/CU-1001",
        },
      ],
    });
    expect(
      screen.queryByRole("button", { name: /bind existing conversation/i }),
    ).not.toBeInTheDocument();
  });

  it("navigates to the starter composer without selecting a conversation", async () => {
    mockConnectedDashboard();
    useAgentSessionStore.getState().selectConversation("project-1", "conv-old");
    useChatStore.getState().setActiveConversation("project:project-1", "conv-old");

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Start conversation" }));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Open composer" }),
    );

    await waitFor(() => {
      expect(useChatStore.getState().activeConversationIds["project:project-1"]).toBeNull();
    });
    expect(useAgentSessionStore.getState().selectedConversationId).toBeNull();
    expect(useAgentSessionStore.getState().focusedProjectId).toBe("project-1");
  });

  it("surfaces a bind error in the detail sheet when the assign API rejects", async () => {
    mockConnectedDashboard();
    vi.mocked(atlassianApi.assignAgentConversationJiraIssue).mockRejectedValueOnce(
      new Error("Issue link rejected"),
    );
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [{ id: "conv-err", title: "Failing bind" }],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Bind existing conversation" }));
    fireEvent.click(screen.getByRole("button", { name: "Failing bind" }));

    expect(await screen.findByText("Issue link rejected")).toBeInTheDocument();
  });

  it("loads more tickets from the list view when a next page is available", () => {
    mockConnectedDashboard();
    const fetchNextPage = vi.fn();
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: { pages: [{ items: [ticket], nextCursor: "cursor-2", total: 5 }], pageParams: [null] },
      isLoading: false,
      isFetching: false,
      isError: false,
      error: null,
      hasNextPage: true,
      fetchNextPage,
      isFetchingNextPage: false,
    } as unknown as ReturnType<typeof ticketingHooks.useTickets>);

    renderDashboard();

    const loadMore = screen.getByRole("button", { name: /load more/i });
    fireEvent.click(loadMore);
    expect(fetchNextPage).toHaveBeenCalled();
  });

  it("closes the start conversation dialog without creating a draft when cancelled", async () => {
    mockConnectedDashboard();

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Start conversation" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(within(dialog).queryByText("Start Conversation")).not.toBeInTheDocument();
    });
    expect(useAgentSessionStore.getState().startConversationDraft).toBeNull();
  });

  it("dedupes ticket-derived status columns when rows share a state", () => {
    mockConnectedDashboard();
    vi.mocked(ticketingHooks.useTicketingColumns).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as ReturnType<typeof ticketingHooks.useTicketingColumns>);
    vi.mocked(ticketingHooks.useTickets).mockReturnValue({
      data: {
        pages: [
          {
            items: [
              ticket,
              { ...ticket, ref: { provider: "jira", id: "10002", key: "RX-2" }, title: "Second" },
            ],
            nextCursor: null,
            total: 2,
          },
        ],
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

    // Both rows share the "To Do" state, so the status filter offers exactly one
    // "To Do" option (the duplicate is skipped in columnsFromTickets).
    const statusFilter = screen.getByRole("combobox", { name: "Status" });
    fireEvent.click(statusFilter);
    expect(
      within(screen.getByRole("listbox", { name: "Status" })).getAllByRole(
        "option",
        { name: "To Do" },
      ),
    ).toHaveLength(1);
  });

  it("closes the ticket detail sheet and clears the selected ref", async () => {
    mockConnectedDashboard();

    renderDashboard();
    fireEvent.click(screen.getByRole("button", { name: /RX-1/ }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(useTicketingStore.getState().selectedTicketRef).not.toBeNull();

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    await waitFor(() => {
      expect(useTicketingStore.getState().selectedTicketRef).toBeNull();
    });
  });
});
