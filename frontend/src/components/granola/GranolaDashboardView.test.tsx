import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { atlassianApi } from "@/api/atlassian";
import { githubApi } from "@/api/github";
import { granolaApi } from "@/api/granola";
import { linearApi } from "@/api/linear";
import { ticketingApi } from "@/api/ticketing";
import * as chatHooks from "@/hooks/useChat";
import * as ticketingHooks from "@/hooks/useTicketing";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useIntegrationDashboardStore } from "@/stores/integrationDashboardStore";
import type { Project } from "@/types/project";

import { GranolaDashboardView } from "./GranolaDashboardView";

vi.mock("@/api/granola", () => ({
  granolaApi: {
    getSettings: vi.fn(),
    listNotes: vi.fn(),
    getNoteDetail: vi.fn(),
    assignAgentConversationGranolaNote: vi.fn(),
  },
}));

vi.mock("@/api/github", () => ({
  githubApi: {
    getBranchOverview: vi.fn(),
  },
}));

vi.mock("@/api/atlassian", () => ({
  atlassianApi: {
    assignAgentConversationJiraIssue: vi.fn(),
  },
}));

vi.mock("@/api/linear", () => ({
  linearApi: {
    assignAgentConversationLinearIssue: vi.fn(),
  },
}));

vi.mock("@/api/ticketing", () => ({
  ticketingApi: {
    listProviders: vi.fn(),
    listTickets: vi.fn(),
  },
}));

vi.mock("@/hooks/useChat", () => ({
  useConversations: vi.fn(),
}));

vi.mock("@/hooks/useTicketing", () => ({
  ticketingKeys: {
    all: ["ticketing"],
    providers: (projectId?: string) => ["ticketing", "providers", projectId ?? null],
    associations: (input: { provider: string; ticketRef: { id: string; key?: string | null }; projectId: string }) => [
      "ticketing",
      "detail",
      input.provider,
      input.ticketRef.id,
      input.ticketRef.key ?? null,
      "associations",
      input.projectId,
    ],
    conversationTicket: (conversationId: string) => ["ticketing", "conversation-ticket", conversationId],
    ticketLists: () => ["ticketing", "tickets"],
  },
  useTicketAssociations: vi.fn(),
  useTicketDetail: vi.fn(),
  useTicketTransitions: vi.fn(),
}));

vi.mock("@/components/agents/agentGranolaNoteQueries", () => ({
  invalidateAgentConversationGranolaNote: vi.fn().mockResolvedValue(undefined),
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
  githubPrEnabled: false,
  repositoryCapability: {
    kind: "github",
    fetchUrl: "https://github.com/ralphx/ralphx.git",
    pushUrl: "git@github.com:ralphx/ralphx.git",
  },
  createdAt: "2026-06-19T22:00:00.000Z",
  updatedAt: "2026-06-19T22:00:00.000Z",
};

const targetProject: Project = {
  ...project,
  id: "project-2",
  name: "Target Project",
  workingDirectory: "/repo/target",
};

const granolaNote = {
  id: "not_1234567890ABCD",
  title: "Weekly planning",
  url: "https://granola.ai/notes/not_1234567890ABCD",
  summary: "Discussed release priorities.",
  createdAt: "2026-06-19T21:00:00.000Z",
  updatedAt: "2026-06-19T22:00:00.000Z",
};

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

function renderGranolaView(
  props: Partial<Parameters<typeof GranolaDashboardView>[0]> = {},
) {
  const Wrapper = createWrapper();
  return render(
    <GranolaDashboardView
      projectId="project-1"
      project={project}
      projects={[project, targetProject]}
      onStartConversation={vi.fn()}
      {...props}
    />,
    { wrapper: Wrapper },
  );
}

describe("GranolaDashboardView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useIntegrationDashboardStore.getState().reset();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);
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
      lastValidatedAt: "2026-06-19T22:00:00.000Z",
      lastError: null,
      updatedAt: "2026-06-19T22:00:00.000Z",
    });
    vi.mocked(granolaApi.listNotes).mockResolvedValue({
      notes: [granolaNote],
      hasMore: false,
      cursor: null,
    });
    vi.mocked(granolaApi.getNoteDetail).mockResolvedValue({
      id: granolaNote.id,
      title: granolaNote.title,
      url: granolaNote.url,
      summary: "### Release priorities\n\n- Ship Granola note browsing.",
      transcript: [
        {
          speaker: "Ada",
          text: "We should finish the standalone Granola dashboard.",
          startMs: 0,
          endMs: 2400,
        },
      ],
    });
    vi.mocked(granolaApi.assignAgentConversationGranolaNote).mockResolvedValue({
      conversationId: "conversation-1",
      projectId: "project-1",
      provider: "granola",
      noteId: granolaNote.id,
      noteUrl: granolaNote.url,
      title: granolaNote.title,
      summaryMarkdown: "### Release priorities\n\n- Ship Granola note browsing.",
      transcript: [],
      includeTranscript: true,
      lastRefreshedAt: "2026-06-19T22:00:00.000Z",
      refreshStatus: "loaded",
      refreshError: null,
      assignedAt: "2026-06-19T22:00:00.000Z",
      assignedFromMessageId: null,
      manuallyAssigned: true,
      createdAt: "2026-06-19T22:00:00.000Z",
      updatedAt: "2026-06-19T22:00:00.000Z",
    });
    vi.mocked(githubApi.getBranchOverview).mockResolvedValue({
      currentBranch: "main",
      branches: [],
      sourcesUnavailable: [],
    });
    vi.mocked(ticketingApi.listProviders).mockResolvedValue([]);
    vi.mocked(ticketingApi.listTickets).mockResolvedValue({
      items: [],
      nextCursor: null,
      total: 0,
      fetchedAt: "2026-06-19T22:00:00.000Z",
    });
    vi.mocked(atlassianApi.assignAgentConversationJiraIssue).mockResolvedValue(null);
    vi.mocked(linearApi.assignAgentConversationLinearIssue).mockResolvedValue(null);
  });

  it("renders grouped notes and copies summary and transcript text", async () => {
    renderGranolaView();

    expect(await screen.findByTestId("granola-dashboard-view")).toBeInTheDocument();
    const row = await screen.findByTestId(`granola-note-row-${granolaNote.id}`);
    expect(row).toHaveTextContent("Weekly planning");
    expect(row).toHaveTextContent(/Jun (19|20)/);
    expect(row).toHaveTextContent(/\d{1,2}:00/);
    expect(granolaApi.listNotes).toHaveBeenCalledWith({
      pageSize: 30,
      projectId: "project-1",
    });
    expect(chatHooks.useConversations).toHaveBeenCalledWith({
      view: "granola",
      projectId: "project-1",
    });

    fireEvent.click(row);
    expect(await screen.findByRole("heading", { name: "Release priorities" })).toBeInTheDocument();
    const markdown = await screen.findByTestId("granola-dashboard-note-markdown");
    expect(markdown).toHaveClass("theme-aware-prose");
    expect(
      within(markdown).getByRole("heading", { name: "Release priorities" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("We should finish the standalone Granola dashboard.")).toBeInTheDocument();
    expect(granolaApi.getNoteDetail).toHaveBeenCalledWith({
      noteId: granolaNote.id,
      includeTranscript: true,
    });

    fireEvent.click(screen.getByRole("button", { name: "Copy summary" }));

    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
        "### Release priorities\n\n- Ship Granola note browsing.",
      );
    });

    fireEvent.click(screen.getByRole("button", { name: "Copy full transcript" }));

    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
        "Ada: We should finish the standalone Granola dashboard.",
      );
    });
  });

  it("filters notes from the standalone header controls", async () => {
    renderGranolaView();

    await screen.findByTestId(`granola-note-row-${granolaNote.id}`);

    fireEvent.change(screen.getByPlaceholderText("Search notes, summaries, or links"), {
      target: { value: "missing" },
    });
    expect(screen.getByText("No Granola notes match these filters.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Reset filters" }));
    expect(screen.getByTestId(`granola-note-row-${granolaNote.id}`)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "No summary 0" }));
    expect(screen.getByText("No Granola notes match these filters.")).toBeInTheDocument();
  });

  it.each([
    { kind: "localOnly" as const },
    {
      kind: "otherRemote" as const,
      fetchUrl: "https://gitlab.com/ralphx/ralphx.git",
      pushUrl: "git@gitlab.com:ralphx/ralphx.git",
    },
    { kind: "inspectionFailed" as const, message: "Unable to inspect" },
  ])("does not load GitHub overview for a $kind project", async (repositoryCapability) => {
    renderGranolaView({ project: { ...project, repositoryCapability } });

    fireEvent.click(await screen.findByTestId(`granola-note-row-${granolaNote.id}`));
    await screen.findByText("We should finish the standalone Granola dashboard.");
    fireEvent.click(screen.getByRole("button", { name: "Add as context" }));

    await screen.findByRole("dialog", { name: "Add Granola Context" });
    expect(githubApi.getBranchOverview).not.toHaveBeenCalled();
  });

  it("loads GitHub overview when the selected project is GitHub capable", async () => {
    renderGranolaView();

    fireEvent.click(await screen.findByTestId(`granola-note-row-${granolaNote.id}`));
    await screen.findByText("We should finish the standalone Granola dashboard.");
    fireEvent.click(screen.getByRole("button", { name: "Add as context" }));

    await waitFor(() =>
      expect(githubApi.getBranchOverview).toHaveBeenCalledWith({ projectId: "project-1" }),
    );
  });

  it("shows RX conversation, ticket, and PR associations for a Granola note", async () => {
    const onNavigateToAssociation = vi.fn();
    vi.mocked(granolaApi.listNotes).mockResolvedValue({
      notes: [
        {
          ...granolaNote,
          rxConversationCount: 1,
          rxConversations: [
            {
              conversationId: "conversation-1",
              title: "Planning agent",
            },
          ],
          ticketCount: 1,
          ticketLinks: [
            {
              provider: "clickup",
              label: "TASK-123",
              title: "ClickUp implementation ticket",
              url: "https://app.clickup.com/t/TASK-123",
            },
          ],
          prCount: 1,
          pullRequests: [
            {
              number: 466,
              url: "https://github.com/aigentive/ralphx.app/pull/466",
              status: "merged",
            },
          ],
        },
      ],
      hasMore: false,
      cursor: null,
    });

    renderGranolaView({ onNavigateToAssociation });

    const row = await screen.findByTestId(`granola-note-row-${granolaNote.id}`);
    fireEvent.click(row);
    await waitFor(() => {
      expect(screen.getAllByLabelText("1 RalphX conversation attached")).toHaveLength(2);
      expect(screen.getAllByLabelText("1 ticket attached")).toHaveLength(2);
      expect(screen.getAllByLabelText("1 pull request attached")).toHaveLength(2);
    });
    expect(screen.getByText("Planning agent")).toBeInTheDocument();
    expect(screen.getByText("ClickUp TASK-123")).toBeInTheDocument();
    expect(screen.getByText("PR #466")).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("Search notes, summaries, or links"), {
      target: { value: "TASK-123" },
    });

    expect(screen.getByTestId(`granola-note-row-${granolaNote.id}`)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Planning agent/ }));
    expect(onNavigateToAssociation).toHaveBeenCalledWith({
      view: "agents",
      id: "conversation-1",
      projectId: "project-1",
    });

    fireEvent.click(screen.getByRole("button", { name: /ClickUp TASK-123/ }));
    expect(onNavigateToAssociation).toHaveBeenCalledTimes(1);
    expect(screen.getByText("TASK-123 · ClickUp")).toBeInTheDocument();
    expect(screen.getAllByText("ClickUp implementation ticket").length).toBeGreaterThan(0);
    await waitFor(() => {
      expect(ticketingHooks.useTicketDetail).toHaveBeenCalledWith({
        provider: "clickup",
        ticketRef: {
          provider: "clickup",
          id: "TASK-123",
          key: "TASK-123",
        },
      }, {
        enabled: true,
      });
    });
  });

  it("shows association actions when a Granola note has no linked RX, ticket, or PR", async () => {
    const onNavigateToAssociation = vi.fn();

    renderGranolaView({ onNavigateToAssociation });

    fireEvent.click(await screen.findByTestId(`granola-note-row-${granolaNote.id}`));
    await screen.findByText("We should finish the standalone Granola dashboard.");
    expect(screen.getByText("No RalphX conversation attached.")).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole("button", { name: "Add context" })[0]!);
    expect(screen.getByRole("dialog", { name: "Add Granola Context" })).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole("button", { name: "Close" })[0]!);
    fireEvent.click(screen.getByRole("button", { name: "Open Ticketing" }));
    expect(onNavigateToAssociation).toHaveBeenCalledWith({
      view: "ticketing",
      id: "",
      projectId: "project-1",
    });

    fireEvent.click(screen.getByRole("button", { name: "Open GitHub" }));
    expect(onNavigateToAssociation).toHaveBeenCalledWith({
      view: "github",
      id: "",
      projectId: "project-1",
    });
  });

  it("restores filters and the selected note after remounting from sidebar navigation", async () => {
    const ticketNote = {
      ...granolaNote,
      id: "not_ticket_123456",
      title: "Ticket follow-up",
      summary: "Discussed ticket implementation.",
      ticketCount: 1,
      ticketLinks: [
        {
          provider: "clickup",
          label: "TASK-123",
          title: "ClickUp implementation ticket",
          url: "https://app.clickup.com/t/TASK-123",
        },
      ],
    };
    vi.mocked(granolaApi.listNotes).mockResolvedValue({
      notes: [granolaNote, ticketNote],
      hasMore: false,
      cursor: null,
    });
    vi.mocked(granolaApi.getNoteDetail).mockImplementation(async ({ noteId }) => ({
      id: noteId,
      title: noteId === ticketNote.id ? ticketNote.title : granolaNote.title,
      url: noteId === ticketNote.id ? ticketNote.url : granolaNote.url,
      summary: noteId === ticketNote.id
        ? "### Ticket follow-up\n\n- Keep the selected note visible."
        : "### Release priorities\n\n- Ship Granola note browsing.",
      transcript: [],
    }));

    const firstRender = renderGranolaView();

    await screen.findByTestId(`granola-note-row-${ticketNote.id}`);
    fireEvent.click(screen.getByRole("button", { name: "Tickets 1" }));
    fireEvent.change(screen.getByPlaceholderText("Search notes, summaries, or links"), {
      target: { value: "TASK-123" },
    });
    fireEvent.click(screen.getByTestId(`granola-note-row-${ticketNote.id}`));
    expect(screen.getAllByText("Ticket follow-up").length).toBeGreaterThan(0);

    firstRender.unmount();
    renderGranolaView();

    expect(await screen.findByTestId(`granola-note-row-${ticketNote.id}`)).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Search notes, summaries, or links")).toHaveValue("TASK-123");
    expect(screen.queryByTestId(`granola-note-row-${granolaNote.id}`)).not.toBeInTheDocument();
    expect(screen.getAllByText("Ticket follow-up").length).toBeGreaterThan(0);
  });

  it("starts a new conversation from a note or binds it to an existing conversation", async () => {
    const onStartConversation = vi.fn();
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [{ id: "conversation-1", title: "Existing agent conversation" }],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);
    vi.mocked(githubApi.getBranchOverview).mockResolvedValue({
      currentBranch: "feature/pr",
      sourcesUnavailable: [],
      branches: [
        {
          branchName: "feature/pr",
          isCurrent: false,
          prNumber: 466,
          prTitle: "Plan GitHub PR and conversation integration",
          prUrl: "https://github.com/aigentive/ralphx.app/pull/466",
          prStatus: "open",
          prIsDraft: false,
          prUpdatedAt: "2026-06-19T22:00:00.000Z",
          prAuthorLogin: "reefagent",
          prBaseRefName: "main",
          rxConversationCount: 1,
          rxConversations: [{ conversationId: "conversation-pr", title: "PR agent" }],
          ticketCount: 0,
          ticketLinks: [],
          ticketLabels: [],
        },
      ],
    });

    renderGranolaView({ onStartConversation });

    fireEvent.click(await screen.findByTestId(`granola-note-row-${granolaNote.id}`));
    await screen.findByText("We should finish the standalone Granola dashboard.");

    fireEvent.click(screen.getByRole("button", { name: "Add as context" }));

    expect(screen.getByRole("dialog", { name: "Add Granola Context" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open composer" }));

    expect(onStartConversation).toHaveBeenCalledWith(
      expect.objectContaining({
        id: granolaNote.id,
        title: granolaNote.title,
      }),
      "project-1",
    );

    fireEvent.click(screen.getByRole("button", { name: "Add as context" }));
    fireEvent.click(screen.getByRole("combobox", { name: "Existing conversation" }));
    fireEvent.click(
      within(screen.getByRole("listbox", { name: "Existing conversation" })).getByRole(
        "option",
        { name: "Existing agent conversation" },
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Bind existing conversation" }));

    await waitFor(() => {
      expect(granolaApi.assignAgentConversationGranolaNote).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        noteId: granolaNote.id,
        title: granolaNote.title,
        noteUrl: granolaNote.url,
        summary: "### Release priorities\n\n- Ship Granola note browsing.",
        includeTranscript: true,
        refresh: true,
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "Add as context" }));
    fireEvent.click(screen.getByRole("combobox", { name: "Existing PR conversation" }));
    fireEvent.click(
      within(screen.getByRole("listbox", { name: "Existing PR conversation" })).getByRole(
        "option",
        { name: /PR #466 Plan GitHub PR and conversation integration/ },
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Bind selected PR" }));

    await waitFor(() => {
      expect(granolaApi.assignAgentConversationGranolaNote).toHaveBeenCalledWith({
        conversationId: "conversation-pr",
        projectId: "project-1",
        noteId: granolaNote.id,
        title: granolaNote.title,
        noteUrl: granolaNote.url,
        summary: "### Release priorities\n\n- Ship Granola note browsing.",
        includeTranscript: true,
        refresh: true,
      });
    });
  });

  it("binds an existing Linear ticket to a Granola note through an existing conversation", async () => {
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [{ id: "conversation-1", title: "Existing agent conversation" }],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);
    vi.mocked(ticketingApi.listProviders).mockResolvedValue([
      {
        provider: "linear",
        label: "Linear",
        enabled: true,
        connectionStatus: "connected",
        capabilities: {
          supportsBoards: false,
          supportsKanban: false,
          kanbanWrite: false,
          statusWrite: true,
          assignmentWrite: true,
          commentWrite: true,
          labelWrite: true,
          freshness: "manual",
        },
        fetchedAt: "2026-06-19T22:00:00.000Z",
        staleAt: null,
        permissionMessage: null,
        errorMessage: null,
      },
    ]);
    vi.mocked(ticketingApi.listTickets).mockResolvedValue({
      items: [
        {
          ref: { provider: "linear", id: "issue-27", key: "WISE-27" },
          title: "Build association binding",
          state: { id: "todo", name: "Todo", category: "todo" },
          assignee: null,
          assignees: [],
          watchers: [],
          reporter: null,
          labels: [],
          sprints: [],
          project: "WISE",
          priority: null,
          updatedAt: "2026-06-19T22:00:00.000Z",
          url: "https://linear.app/team/issue/WISE-27",
          associationCount: 0,
          openPrCount: 0,
          openPrNumber: null,
          openPrUrl: null,
          openPrStatus: null,
          currentUserAssigned: false,
          currentUserWatching: false,
        },
      ],
      nextCursor: null,
      total: 1,
      fetchedAt: "2026-06-19T22:00:00.000Z",
    });

    renderGranolaView();

    fireEvent.click(await screen.findByTestId(`granola-note-row-${granolaNote.id}`));
    await screen.findByText("We should finish the standalone Granola dashboard.");
    fireEvent.click(screen.getByRole("button", { name: "Add as context" }));

    await waitFor(() => {
      expect(screen.getByRole("combobox", { name: "Ticket provider" })).toHaveTextContent("Linear");
    });
    await waitFor(() => {
      expect(ticketingApi.listTickets).toHaveBeenCalledWith({
        provider: "linear",
        projectId: "project-1",
        limit: 80,
        sort: "updated_desc",
      });
      expect(screen.getByRole("combobox", { name: "Existing ticket" })).not.toBeDisabled();
    });
    fireEvent.click(screen.getByRole("combobox", { name: "Existing ticket" }));
    fireEvent.click(
      within(screen.getByRole("listbox", { name: "Existing ticket" })).getByRole(
        "option",
        { name: /WISE-27 Build association binding/ },
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Bind selected ticket" }));

    await waitFor(() => {
      expect(linearApi.assignAgentConversationLinearIssue).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        issueId: "issue-27",
        issueKey: "WISE-27",
        title: "Build association binding",
        issueUrl: "https://linear.app/team/issue/WISE-27",
        refresh: true,
      });
      expect(granolaApi.assignAgentConversationGranolaNote).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        noteId: granolaNote.id,
        title: granolaNote.title,
        noteUrl: granolaNote.url,
        summary: "### Release priorities\n\n- Ship Granola note browsing.",
        includeTranscript: true,
        refresh: true,
      });
    });
  });
});
