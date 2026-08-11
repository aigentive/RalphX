import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render as rtlRender, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement, ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { openUrlMock } = vi.hoisted(() => ({ openUrlMock: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));

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

import type {
  TicketAssociations,
  TicketDetail,
  TicketingCapabilities,
  TicketSummary,
  TicketTransitionOption,
} from "@/api/ticketing";
import { granolaApi } from "@/api/granola";
import { TooltipProvider } from "@/components/ui/tooltip";

import { TicketDetailSheet } from "./TicketDetailSheet";

const baseCapabilities: TicketingCapabilities = {
  supportsBoards: true,
  supportsKanban: true,
  kanbanWrite: true,
  statusWrite: true,
  assignmentWrite: true,
  commentWrite: true,
  labelWrite: true,
  freshness: "manual",
};

const baseTicket: TicketSummary = {
  ref: { provider: "linear", id: "ABC-1", key: "ABC-1" },
  title: "Polish the ticket detail overlay",
  state: { id: "todo", name: "To Do", category: "todo" },
  labels: [],
  updatedAt: "2026-06-19T22:00:00.000Z",
  url: null,
  associationCount: 0,
  openPrCount: 0,
  currentUserAssigned: false,
};

const writableTransition: TicketTransitionOption = {
  toStateId: "done",
  name: "Done",
  category: "done",
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

function render(ui: ReactElement, options?: Parameters<typeof rtlRender>[1]) {
  return rtlRender(ui, { wrapper: createWrapper(), ...options });
}

function renderSheet(
  overrides: {
    ticket?: TicketSummary | TicketDetail;
    capabilities?: TicketingCapabilities;
    associations?: TicketAssociations;
    onStartWork?: () => void;
  } = {},
) {
  const Wrapper = createWrapper();
  return render(
    <TicketDetailSheet
      open
      ticket={overrides.ticket ?? baseTicket}
      capabilities={overrides.capabilities ?? baseCapabilities}
      transitions={[writableTransition]}
      associations={overrides.associations}
      projectId="project-1"
      isDetailLoading={false}
      isAssociationsLoading={false}
      isTransitionPending={false}
      isAssignPending={false}
      isCommentPending={false}
      {...(overrides.onStartWork ? { onStartWork: overrides.onStartWork } : {})}
      onClose={vi.fn()}
    />,
    { wrapper: Wrapper },
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(granolaApi.getSettings).mockResolvedValue({
    enabled: false,
    hasApiToken: false,
    validationStatus: "not_configured",
    lastValidatedAt: null,
    lastError: null,
    updatedAt: "2026-06-19T22:00:00.000Z",
  });
  vi.mocked(granolaApi.listNotes).mockResolvedValue({
    notes: [],
    hasMore: false,
    cursor: null,
  });
  vi.mocked(granolaApi.assignAgentConversationGranolaNote).mockResolvedValue(null);
});

describe("TicketDetailSheet assignee control", () => {
  it("offers 'Assign to me' only when the ticket is unassigned", () => {
    renderSheet();

    expect(screen.getByRole("button", { name: /assign to me/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /clear assignee/i })).not.toBeInTheDocument();
  });

  it("shows the assignee and hides 'Assign to me' when assigned", () => {
    renderSheet({
      ticket: { ...baseTicket, assignee: { name: "Alex Developer" } },
    });

    expect(screen.getByText("Assignee")).toBeInTheDocument();
    expect(screen.getByLabelText("Alex Developer")).toHaveAttribute("title", "Alex Developer");
    expect(screen.queryByText("Alex Developer")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /assign to me/i })).not.toBeInTheDocument();
    // Clear assignee stays available because assignment write-back is enabled.
    expect(screen.getByRole("button", { name: /clear assignee/i })).toBeInTheDocument();
  });

  it("shows the assignee read-only when assignment write-back is unavailable", () => {
    renderSheet({
      ticket: { ...baseTicket, assignee: { name: "Alex Developer" } },
      capabilities: { ...baseCapabilities, assignmentWrite: false },
    });

    expect(screen.getByLabelText("Alex Developer")).toHaveAttribute("title", "Alex Developer");
    expect(screen.queryByText("Alex Developer")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /assign to me/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /clear assignee/i })).not.toBeInTheDocument();
  });

  it("applies the unified select treatment to the status control", () => {
    renderSheet();

    const status = screen.getByRole("combobox", { name: "Ticket status" });
    // Caret + visible focus ring added by the shared helper.
    expect(status.className).toContain("appearance-none");
    expect(status.className).toContain(
      "focus-visible:[outline:2px_solid_var(--border-focus)]",
    );
    // Denser sheet typography preserved.
    expect(status.className).toContain("text-xs");
    // Unified off the former --bg-surface outlier onto --bg-elevated.
    expect((status as HTMLElement).style.backgroundColor).toBe("var(--bg-elevated)");
  });
});

describe("TicketDetailSheet ticket git metadata", () => {
  it("shows the empty RalphX links prompt without a synthetic ticket branch", () => {
    const onStartWork = vi.fn();
    renderSheet({
      onStartWork,
      associations: {
        tasks: [],
        proposals: [],
        sessions: [],
        conversations: [],
        pullRequests: [],
        checks: [],
        qa: [],
        specs: [],
        fetchedAt: null,
      },
    });

    expect(screen.queryByText("Ticket Git")).not.toBeInTheDocument();
    expect(screen.queryByText("ralphx/ticket/linear-abc-1")).not.toBeInTheDocument();
    expect(
      screen.getByText("No RalphX links yet. Start a conversation with this ticket attached."),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Start conversation" }));
    expect(onStartWork).toHaveBeenCalledTimes(1);
  });

  it("loads pull request metadata from the ticket even without association links", async () => {
    openUrlMock.mockClear();
    renderSheet({
      ticket: {
        ...baseTicket,
        openPrNumber: 42,
        openPrUrl: "https://github.com/acme/app/pull/42",
        openPrStatus: "open",
      },
      associations: {
        tasks: [],
        proposals: [],
        sessions: [],
        conversations: [],
        pullRequests: [],
        checks: [],
        qa: [],
        specs: [],
        fetchedAt: null,
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "PR #42 (open)" }));

    await waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith("https://github.com/acme/app/pull/42");
    });
  });
});

describe("TicketDetailSheet Granola associations", () => {
  it("binds an existing Granola note through a linked ticket conversation", async () => {
    const user = userEvent.setup();
    vi.mocked(granolaApi.getSettings).mockResolvedValue({
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      lastValidatedAt: "2026-06-19T22:00:00.000Z",
      lastError: null,
      updatedAt: "2026-06-19T22:00:00.000Z",
    });
    vi.mocked(granolaApi.listNotes).mockResolvedValue({
      notes: [
        {
          id: "not_ticket",
          title: "Planning notes",
          url: "https://granola.ai/notes/not_ticket",
          summary: "Ticket planning context",
          createdAt: "2026-06-19T21:00:00.000Z",
          updatedAt: "2026-06-19T22:00:00.000Z",
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
    renderSheet({
      associations: {
        tasks: [],
        proposals: [],
        sessions: [],
        conversations: [
          {
            id: "conversation-1",
            title: "Ticket conversation",
            subtitle: "conversation-1",
            status: null,
            active: true,
            deepLink: { view: "agents", id: "conversation-1", projectId: "project-1" },
          },
        ],
        pullRequests: [],
        checks: [],
        qa: [],
        specs: [],
        fetchedAt: null,
      },
    });

    expect(await screen.findByText("Granola (0)")).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Add Granola" }));
    await user.click(screen.getByRole("combobox", { name: "Granola note" }));
    await user.click(
      screen.getByRole("option", { name: /Planning notes/ }),
    );
    await user.click(screen.getByRole("button", { name: "Bind Granola note" }));

    await waitFor(() => {
      expect(granolaApi.assignAgentConversationGranolaNote).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        noteId: "not_ticket",
        title: "Planning notes",
        noteUrl: "https://granola.ai/notes/not_ticket",
        summary: "Ticket planning context",
        includeTranscript: true,
        refresh: true,
      });
    });
  });
});

describe("TicketDetailSheet new-comment awareness", () => {
  const SEEN = "2026-06-20T12:00:00.000Z";

  const detailWithComments: TicketDetail = {
    ...baseTicket,
    descriptionMarkdown: null,
    comments: [
      {
        id: "c-new",
        author: { name: "Reviewer" },
        bodyMarkdown: "Fresh take",
        bodyText: "Fresh take",
        createdAt: "2026-06-20T13:00:00.000Z",
      },
      {
        id: "c-old",
        author: { name: "Reporter" },
        bodyMarkdown: "Earlier note",
        bodyText: "Earlier note",
        createdAt: "2026-06-20T11:00:00.000Z",
      },
    ],
    attachments: [],
    transitions: [],
  };

  function renderDetail(seenUntil: string | null) {
    return render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={detailWithComments}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          seenUntil={seenUntil}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );
  }

  it("flags only comments created after the last open and counts them", () => {
    renderDetail(SEEN);

    expect(screen.getByText("Fresh take")).toBeInTheDocument();
    expect(screen.getByText("Earlier note")).toBeInTheDocument();
    // Exactly one "New" badge (the comment created after SEEN).
    expect(screen.getAllByText("New")).toHaveLength(1);
    // The new-comment count is surfaced (jump button + section header).
    expect(screen.getAllByText(/1 new/).length).toBeGreaterThan(0);
  });

  it("flags nothing when the ticket was never opened before", () => {
    renderDetail(null);

    expect(screen.queryByText("New")).not.toBeInTheDocument();
    expect(screen.queryByText(/new$/)).not.toBeInTheDocument();
  });

  it("expands threaded replies when a provider supplies nested comments", () => {
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={{
            ...detailWithComments,
            comments: [
              {
                id: "thread-root",
                author: { name: "Reviewer" },
                bodyMarkdown: "Root comment",
                bodyText: "Root comment",
                createdAt: "2026-06-20T13:00:00.000Z",
                replies: [
                  {
                    id: "thread-reply",
                    author: { name: "Responder" },
                    bodyMarkdown: "Thread reply",
                    bodyText: "Thread reply",
                    createdAt: "2026-06-20T13:05:00.000Z",
                  },
                ],
              },
            ],
          }}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    expect(screen.getByText("Root comment")).toBeInTheDocument();
    expect(screen.queryByText("Thread reply")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "View thread (1)" }));

    expect(screen.getByText("Thread reply")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hide thread (1)" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("renders ClickUp epoch-millisecond comments and replies oldest first", () => {
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={{
            ...detailWithComments,
            comments: [
              {
                id: "newer-root",
                author: { name: "Reviewer" },
                bodyMarkdown: "Newer root",
                bodyText: "Newer root",
                createdAt: "1700000002000",
              },
              {
                id: "older-root",
                author: { name: "Reviewer" },
                bodyMarkdown: "Older root",
                bodyText: "Older root",
                createdAt: "1700000000000",
                replies: [
                  {
                    id: "newer-reply",
                    author: { name: "Responder" },
                    bodyMarkdown: "Newer reply",
                    bodyText: "Newer reply",
                    createdAt: "1700000003000",
                  },
                  {
                    id: "older-reply",
                    author: { name: "Responder" },
                    bodyMarkdown: "Older reply",
                    bodyText: "Older reply",
                    createdAt: "1700000001000",
                  },
                ],
              },
            ],
          }}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    const bodyText = document.body.textContent ?? "";
    expect(bodyText.indexOf("Older root")).toBeLessThan(bodyText.indexOf("Newer root"));

    fireEvent.click(screen.getByRole("button", { name: "View thread (2)" }));

    const expandedText = document.body.textContent ?? "";
    expect(expandedText.indexOf("Older reply")).toBeLessThan(expandedText.indexOf("Newer reply"));
  });
});

describe("TicketDetailSheet bind existing conversation", () => {
  const bindableConversations = [
    { id: "conv-1", title: "Refactor merge engine" },
    { id: "conv-2", title: "Investigate flaky test" },
    { id: "conv-3", title: null },
  ];

  function renderBindSheet(
    overrides: {
      onBindConversation?: (conversationId: string) => void;
      isBindPending?: boolean;
      bindError?: string | null;
      conversations?: { id: string; title: string | null }[];
    } = {},
  ) {
    return render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={baseTicket}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          bindableConversations={overrides.conversations ?? bindableConversations}
          onBindConversation={overrides.onBindConversation ?? vi.fn()}
          {...(overrides.isBindPending !== undefined && { isBindPending: overrides.isBindPending })}
          {...(overrides.bindError !== undefined && { bindError: overrides.bindError })}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );
  }

  it("renders the bind button in the RalphX Work panel", () => {
    renderBindSheet();

    expect(
      screen.getByRole("button", { name: /bind existing conversation/i }),
    ).toBeInTheDocument();
  });

  it("opens the picker and lists conversations by title with an untitled fallback", () => {
    renderBindSheet();

    fireEvent.click(screen.getByRole("button", { name: /bind existing conversation/i }));

    expect(screen.getByText("Refactor merge engine")).toBeInTheDocument();
    expect(screen.getByText("Investigate flaky test")).toBeInTheDocument();
    expect(screen.getByText("Untitled agent")).toBeInTheDocument();
  });

  it("filters the picker list by title, case-insensitively", () => {
    renderBindSheet();

    fireEvent.click(screen.getByRole("button", { name: /bind existing conversation/i }));
    fireEvent.change(screen.getByPlaceholderText(/search conversations/i), {
      target: { value: "FLAKY" },
    });

    expect(screen.getByText("Investigate flaky test")).toBeInTheDocument();
    expect(screen.queryByText("Refactor merge engine")).not.toBeInTheDocument();
  });

  it("calls onBindConversation with the chosen id and closes the picker", () => {
    const onBindConversation = vi.fn();
    renderBindSheet({ onBindConversation });

    fireEvent.click(screen.getByRole("button", { name: /bind existing conversation/i }));
    fireEvent.click(screen.getByRole("button", { name: "Investigate flaky test" }));

    expect(onBindConversation).toHaveBeenCalledWith("conv-2");
    // Picker closes after a successful pick.
    expect(screen.queryByPlaceholderText(/search conversations/i)).not.toBeInTheDocument();
  });

  it("shows an empty-state message when there are no conversations to bind", () => {
    renderBindSheet({ conversations: [] });

    fireEvent.click(screen.getByRole("button", { name: /bind existing conversation/i }));

    expect(screen.getByText("No conversations to bind.")).toBeInTheDocument();
  });

  it("surfaces a bind error in an alert", () => {
    renderBindSheet({ bindError: "Could not bind conversation." });

    expect(screen.getByRole("alert")).toHaveTextContent("Could not bind conversation.");
  });
});

describe("TicketDetailSheet loading state", () => {
  it("shows a skeleton preloader instead of empty states while detail loads", () => {
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={baseTicket}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    // The summary header is shown immediately, but the body is a preloader, not
    // "no description"/"no comments" (which would imply loaded-but-empty).
    expect(screen.getByText("Polish the ticket detail overlay")).toBeInTheDocument();
    expect(
      screen.getAllByRole("status", { name: /loading ticket details/i }).length,
    ).toBeGreaterThan(0);
    expect(screen.queryByText("No description provided.")).not.toBeInTheDocument();
    expect(screen.queryByText("No comments yet.")).not.toBeInTheDocument();
  });
});

describe("TicketDetailSheet branch vs pull-request distinction", () => {
  function renderWithPullRequests() {
    const associations: TicketAssociations = {
      tasks: [],
      proposals: [],
      sessions: [],
      conversations: [],
      pullRequests: [
        {
          id: "https://github.com/x/y/pull/7",
          title: "PR #7",
          subtitle: "ralphx/p/agent-1",
          status: "open",
          active: true,
          deepLink: { view: "agents", id: "c1", projectId: "p1" },
        },
        {
          id: "c2",
          title: "ralphx/p/agent-2",
          subtitle: "ralphx/p/agent-2",
          status: "branch",
          active: false,
          deepLink: { view: "agents", id: "c2", projectId: "p1" },
        },
        {
          id: "https://github.com/x/y/pull/9",
          title: "PR #9",
          subtitle: "ralphx/p/agent-3",
          status: "merged",
          active: false,
          deepLink: { view: "agents", id: "c3", projectId: "p1" },
        },
      ],
      checks: [],
      qa: [],
      specs: [],
      fetchedAt: null,
    };
    return render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={baseTicket}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={associations}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );
  }

  it("marks PR items with a pull-request icon and branch-only items with a branch icon", () => {
    renderWithPullRequests();

    expect(screen.getByRole("img", { name: "Open pull request" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /branch only/i })).toBeInTheDocument();
    expect(screen.getByText("PR #7")).toBeInTheDocument();
    expect(screen.getByText("ralphx/p/agent-2")).toBeInTheDocument();
  });

  it("colors an open PR icon as active and a terminal (merged/closed) PR icon as muted", () => {
    renderWithPullRequests();

    expect(screen.getByRole("img", { name: "Open pull request" })).toHaveClass(
      "text-[var(--status-success)]",
    );
    expect(screen.getByRole("img", { name: /merged or closed/i })).toHaveClass(
      "text-[var(--text-muted)]",
    );
  });
});

describe("TicketDetailSheet open in provider", () => {
  it("opens the provider URL through the app opener (reliable in WKWebView)", async () => {
    openUrlMock.mockClear();
    renderSheet({
      ticket: { ...baseTicket, url: "https://linear.app/x/issue/ABC-1" },
    });

    fireEvent.click(screen.getByRole("link", { name: /open in provider/i }));

    await waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith("https://linear.app/x/issue/ABC-1");
    });
  });
});

describe("TicketDetailSheet comment keyboard submit", () => {
  function renderComposer(onAddComment: (body: string) => Promise<void> | void) {
    return render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={baseTicket}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onAddComment={onAddComment}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );
  }

  it("submits the comment on Cmd/Ctrl+Enter", async () => {
    const onAddComment = vi.fn().mockResolvedValue(undefined);
    renderComposer(onAddComment);

    const textarea = screen.getByRole("textbox", { name: "Ticket comment" });
    fireEvent.change(textarea, { target: { value: "Looks good" } });
    fireEvent.keyDown(textarea, { key: "Enter", metaKey: true });

    await waitFor(() => {
      expect(onAddComment).toHaveBeenCalledWith("Looks good");
    });
  });

  it("does not submit on Enter without a modifier", () => {
    const onAddComment = vi.fn().mockResolvedValue(undefined);
    renderComposer(onAddComment);

    const textarea = screen.getByRole("textbox", { name: "Ticket comment" });
    fireEvent.change(textarea, { target: { value: "Looks good" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onAddComment).not.toHaveBeenCalled();
  });
});

describe("TicketDetailSheet label editing", () => {
  function renderLabelSheet(overrides: {
    ticket?: TicketSummary;
    capabilities?: TicketingCapabilities;
    onSetLabels?: (labels: string[]) => void;
    labelOptions?: { id?: string | null; name: string }[];
  }) {
    return render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={overrides.ticket ?? baseTicket}
          capabilities={overrides.capabilities ?? baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          {...(overrides.onSetLabels ? { onSetLabels: overrides.onSetLabels } : {})}
          {...(overrides.labelOptions ? { labelOptions: overrides.labelOptions } : {})}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );
  }

  it("renders read-only labels and shows a disabled reason when label write is unavailable", () => {
    const onSetLabels = vi.fn();
    renderLabelSheet({
      ticket: { ...baseTicket, labels: ["backend"] },
      capabilities: { ...baseCapabilities, labelWrite: false },
      onSetLabels,
    });

    expect(screen.getByText("backend")).toBeInTheDocument();
    // No add/remove controls when editing is unavailable.
    expect(screen.queryByLabelText("Add a label")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Remove label backend")).not.toBeInTheDocument();
    expect(onSetLabels).not.toHaveBeenCalled();
  });

  it("Jira: typing + Enter adds a label to the full set", () => {
    const onSetLabels = vi.fn();
    renderLabelSheet({
      ticket: { ...baseTicket, ref: { provider: "jira", id: "JRA-1", key: "JRA-1" }, labels: ["bug"] },
      onSetLabels,
    });

    const input = screen.getByLabelText("Add a label");
    fireEvent.change(input, { target: { value: "frontend" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onSetLabels).toHaveBeenCalledWith(["bug", "frontend"]);
  });

  it("Jira: blocks labels containing spaces with a clear message", () => {
    const onSetLabels = vi.fn();
    renderLabelSheet({
      ticket: { ...baseTicket, ref: { provider: "jira", id: "JRA-1", key: "JRA-1" }, labels: [] },
      onSetLabels,
    });

    const input = screen.getByLabelText("Add a label");
    fireEvent.change(input, { target: { value: "needs triage" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onSetLabels).not.toHaveBeenCalled();
    expect(screen.getByText("Jira labels can't contain spaces")).toBeInTheDocument();
  });

  it("removes a label via the chip × button", () => {
    const onSetLabels = vi.fn();
    renderLabelSheet({
      ticket: { ...baseTicket, ref: { provider: "jira", id: "JRA-1", key: "JRA-1" }, labels: ["bug", "frontend"] },
      onSetLabels,
    });

    fireEvent.click(screen.getByLabelText("Remove label bug"));
    expect(onSetLabels).toHaveBeenCalledWith(["frontend"]);
  });

  it("Linear: choosing an existing team label appends it; no free typing", () => {
    const onSetLabels = vi.fn();
    renderLabelSheet({
      ticket: { ...baseTicket, ref: { provider: "linear", id: "LIN-1", key: "LIN-1" }, labels: ["bug"] },
      onSetLabels,
      labelOptions: [
        { id: "label-bug", name: "bug" },
        { id: "label-feature", name: "feature" },
      ],
    });

    // No free-text input in Linear mode.
    expect(screen.queryByRole("textbox", { name: "Add a label" })).not.toBeInTheDocument();
    const select = screen.getByRole("combobox", { name: "Add a label" });
    // Already-applied "bug" is filtered out of the options.
    fireEvent.click(select);
    expect(screen.queryByRole("option", { name: "bug" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "feature" }));
    expect(onSetLabels).toHaveBeenCalledWith(["bug", "feature"]);
  });
});

describe("TicketDetailSheet project chip", () => {
  it("renders the project as a distinct labeled chip, not a plain tag", () => {
    renderSheet({
      ticket: { ...baseTicket, project: "Platform", labels: ["backend"] },
    });

    // The project carries a "Project:" title to set it apart from label tags.
    expect(screen.getByTitle("Project: Platform")).toBeInTheDocument();
    expect(screen.getByText("Platform")).toBeInTheDocument();
    expect(screen.getByText("backend")).toBeInTheDocument();
  });
});

describe("TicketDetailSheet description images", () => {
  it("renders ticket image attachments in the detail sheet", () => {
    renderSheet({
      ticket: {
        ...baseTicket,
        descriptionMarkdown: "",
        descriptionText: "",
        comments: [],
        attachments: [
          {
            id: "att-1",
            filename: "mockup.png",
            mimeType: "image/png",
            size: 2048,
            url: "https://provider.example/mockup.png",
          },
        ],
        transitions: [],
      },
    });

    expect(screen.getByText("Attachments (1)")).toBeInTheDocument();
    expect(screen.getByAltText("mockup.png")).toHaveAttribute(
      "src",
      "https://provider.example/mockup.png",
    );
    expect(screen.getByText("image/png · 2.0 KB")).toBeInTheDocument();
  });

  it("opens image attachments in an in-app preview instead of the external URL", async () => {
    openUrlMock.mockClear();
    renderSheet({
      ticket: {
        ...baseTicket,
        descriptionMarkdown: "",
        descriptionText: "",
        comments: [],
        attachments: [
          {
            id: "att-1",
            filename: "mockup.png",
            mimeType: "image/png",
            size: 2048,
            url: "https://provider.example/mockup.png",
          },
        ],
        transitions: [],
      },
    });

    await userEvent.click(screen.getByRole("button", { name: "Preview attachment mockup.png" }));

    expect(openUrlMock).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "mockup.png" })).toBeInTheDocument();
    expect(screen.getAllByAltText("mockup.png").at(-1)).toHaveAttribute(
      "src",
      "https://provider.example/mockup.png",
    );
  });

  it("renders comment image attachments below the comment body", () => {
    renderSheet({
      ticket: {
        ...baseTicket,
        descriptionMarkdown: "",
        descriptionText: "",
        comments: [
          {
            id: "comment-1",
            bodyMarkdown: "See attached",
            bodyText: "See attached",
            attachments: [
              {
                id: "comment-att-1",
                filename: "comment-shot.jpg",
                mimeType: "image/jpeg",
                size: 1024,
                url: "https://provider.example/comment-shot.jpg",
              },
            ],
            replies: [],
          },
        ],
        attachments: [],
        transitions: [],
      },
    });

    expect(screen.getByText("See attached")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Preview attachment comment-shot.jpg" }),
    ).toBeInTheDocument();
    expect(screen.getByAltText("comment-shot.jpg")).toHaveAttribute(
      "src",
      "https://provider.example/comment-shot.jpg",
    );
  });

  it("falls back to an open-in-browser button when a description image fails to load", async () => {
    openUrlMock.mockClear();
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={{
            ...baseTicket,
            descriptionMarkdown: "![diagram](https://provider.example/img.png)",
            descriptionText: "",
            comments: [],
            attachments: [],
            transitions: [],
          }}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    const img = document.querySelector(
      "img[src='https://provider.example/img.png']",
    ) as HTMLImageElement | null;
    expect(img).not.toBeNull();
    fireEvent.error(img as HTMLImageElement);

    const viewButton = screen.getByRole("button", { name: "diagram" });
    fireEvent.click(viewButton);
    await waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith("https://provider.example/img.png");
    });
  });
});

describe("TicketDetailSheet status options", () => {
  it("does not list the current status twice when a transition shares its name", () => {
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={{ ...baseTicket, state: { id: "s-current", name: "In Progress", category: "todo" } }}
          capabilities={baseCapabilities}
          transitions={[
            // Same display name as the current state but a different id (the provider
            // can return the current state in its transition list with a divergent id).
            { toStateId: "s-other", name: "In Progress", category: "todo" },
            { toStateId: "s-done", name: "Done", category: "done" },
          ]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole("combobox", { name: "Ticket status" }));
    const inProgressOptions = screen
      .getAllByRole("option")
      .filter((option) => option.textContent === "In Progress");
    expect(inProgressOptions).toHaveLength(1);
    expect(screen.getByRole("option", { name: "Done" })).toBeInTheDocument();
  });

  it("ignores a status change to a disabled transition", () => {
    const onTransitionTicket = vi.fn();
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={baseTicket}
          capabilities={baseCapabilities}
          transitions={[
            writableTransition,
            {
              toStateId: "blocked",
              name: "Blocked",
              category: "other",
              disabledReason: "Workflow blocks this transition.",
            },
          ]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onTransitionTicket={onTransitionTicket}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole("combobox", { name: "Ticket status" }));
    fireEvent.click(screen.getByRole("option", { name: /Blocked/ }));
    // The disabled transition must not fire the transition callback.
    expect(onTransitionTicket).not.toHaveBeenCalled();

    // A writable transition still routes through the callback.
    fireEvent.click(screen.getByRole("option", { name: "Done" }));
    expect(onTransitionTicket).toHaveBeenCalledWith(writableTransition);
  });
});

describe("TicketDetailSheet associations navigation", () => {
  const associations: TicketAssociations = {
    tasks: [
      {
        id: "task-1",
        title: "Linked task",
        status: "executing",
        active: true,
        deepLink: { view: "kanban", id: "task-1", projectId: "p1" },
      },
    ],
    proposals: [],
    sessions: [],
    conversations: [],
    pullRequests: [],
    checks: [],
    qa: [],
    specs: [],
    fetchedAt: null,
  };

  it("invokes onNavigate with the association deep link when a row is clicked", () => {
    const onNavigate = vi.fn();
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={baseTicket}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={associations}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onNavigate={onNavigate}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: /Linked task/ }));
    expect(onNavigate).toHaveBeenCalledWith({ view: "kanban", id: "task-1", projectId: "p1" });
  });
});

describe("TicketDetailSheet disabled-control tooltips", () => {
  it("wraps the status control in a tooltip when status write-back is unavailable", () => {
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={baseTicket}
          capabilities={{ ...baseCapabilities, statusWrite: false }}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    // The status select is rendered disabled with the explanatory reason in its
    // tooltip trigger wrapper.
    const statusSelect = screen.getByRole("combobox", { name: "Ticket status" });
    expect(statusSelect).toBeDisabled();
  });
});

describe("TicketDetailSheet comment optimistic flow", () => {
  function renderWithComments(args: {
    ticket: TicketDetail;
    onAddComment?: (body: string) => Promise<void> | void;
  }) {
    return render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={args.ticket}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          {...(args.onAddComment ? { onAddComment: args.onAddComment } : {})}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );
  }

  const detail = (comments: TicketDetail["comments"]): TicketDetail => ({
    ...baseTicket,
    descriptionMarkdown: "Detail body.",
    comments,
    attachments: [],
    transitions: [writableTransition],
  });

  it("keeps an optimistic comment visible, then dedupes it once the provider echoes it", async () => {
    const onAddComment = vi.fn().mockResolvedValue(undefined);
    const { rerender } = renderWithComments({ ticket: detail([]), onAddComment });

    const textarea = screen.getByRole("textbox", { name: "Ticket comment" });
    fireEvent.change(textarea, { target: { value: "Pushed a fix." } });
    fireEvent.click(screen.getByRole("button", { name: "Add comment" }));

    await waitFor(() => expect(onAddComment).toHaveBeenCalledWith("Pushed a fix."));
    // Optimistic local comment is shown immediately.
    expect(
      screen.getByText("Pushed a fix.", { selector: "p" }),
    ).toBeInTheDocument();

    // The provider now returns the confirmed comment with the same body; the
    // local optimistic copy must be pruned so only one row remains.
    rerender(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={detail([
            {
              id: "server-1",
              author: { name: "RalphX" },
              bodyMarkdown: "Pushed a fix.",
              bodyText: "Pushed a fix.",
              createdAt: "2026-06-19T22:00:03.000Z",
              updatedAt: "2026-06-19T22:00:03.000Z",
            },
          ])}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onAddComment={onAddComment}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    await waitFor(() => {
      expect(screen.getAllByText("Pushed a fix.", { selector: "p" })).toHaveLength(1);
    });
  });

  it("rolls back the optimistic comment when the add fails and preserves the draft", async () => {
    const onAddComment = vi.fn().mockRejectedValue(new Error("Comment rejected"));
    renderWithComments({ ticket: detail([]), onAddComment });

    const textarea = screen.getByRole("textbox", { name: "Ticket comment" });
    fireEvent.change(textarea, { target: { value: "Will fail." } });
    fireEvent.click(screen.getByRole("button", { name: "Add comment" }));

    await waitFor(() => expect(onAddComment).toHaveBeenCalled());
    // The optimistic comment is removed on failure; the draft is preserved.
    await waitFor(() => {
      expect(screen.queryByText("Will fail.", { selector: "p" })).not.toBeInTheDocument();
    });
    expect(textarea).toHaveValue("Will fail.");
  });
});

describe("TicketDetailSheet close affordance", () => {
  it("calls onClose when the overlay is dismissed via Escape", async () => {
    const onClose = vi.fn();
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={baseTicket}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onClose={onClose}
        />
      </TooltipProvider>,
    );

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });
});

describe("TicketDetailSheet markdown image edge cases", () => {
  it("renders nothing for a markdown image with an empty source", () => {
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={{
            ...baseTicket,
            // Empty src → TicketMarkdownImage returns null (no broken-image icon).
            descriptionMarkdown: "![empty]()",
            descriptionText: "",
            comments: [],
            attachments: [],
            transitions: [],
          }}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    // No <img> and no view-image fallback button is produced for the empty source.
    expect(document.querySelector("img")).toBeNull();
    expect(screen.queryByRole("button", { name: /view image/i })).not.toBeInTheDocument();
  });
});

describe("TicketDetailSheet assignment and comment controls", () => {
  it("routes Assign to me and Clear assignee through their callbacks", () => {
    const onAssignToMe = vi.fn();
    const onClearAssignee = vi.fn();

    const { rerender } = render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={baseTicket}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onAssignToMe={onAssignToMe}
          onClearAssignee={onClearAssignee}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Assign to me" }));
    expect(onAssignToMe).toHaveBeenCalled();

    // Re-render with an assignee so the Clear control is offered.
    rerender(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={{ ...baseTicket, assignee: { id: "user-1", name: "A. User" } }}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onAssignToMe={onAssignToMe}
          onClearAssignee={onClearAssignee}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Clear assignee" }));
    expect(onClearAssignee).toHaveBeenCalled();
  });

  it("does not submit an empty comment and jumps to the comments section", () => {
    const onAddComment = vi.fn();
    render(
      <TooltipProvider>
        <TicketDetailSheet
          open
          ticket={{
            ...baseTicket,
            descriptionMarkdown: "Body.",
            comments: [
              {
                id: "c-1",
                author: { name: "A. User" },
                bodyMarkdown: "Existing comment.",
                bodyText: "Existing comment.",
                createdAt: "2026-06-19T22:00:00.000Z",
                updatedAt: "2026-06-19T22:00:00.000Z",
              },
            ],
            attachments: [],
            transitions: [writableTransition],
          }}
          capabilities={baseCapabilities}
          transitions={[writableTransition]}
          associations={undefined}
          isDetailLoading={false}
          isAssociationsLoading={false}
          isTransitionPending={false}
          isAssignPending={false}
          isCommentPending={false}
          onAddComment={onAddComment}
          onClose={vi.fn()}
        />
      </TooltipProvider>,
    );

    // The Add comment button is disabled while the draft is empty, so clicking it
    // (or submitting) never reaches the provider callback.
    const addButton = screen.getByRole("button", { name: "Add comment" });
    expect(addButton).toBeDisabled();
    fireEvent.click(addButton);
    expect(onAddComment).not.toHaveBeenCalled();

    // The comments jump control scrolls the comments section into view.
    const scrollIntoView = vi.fn();
    window.HTMLElement.prototype.scrollIntoView = scrollIntoView;
    fireEvent.click(screen.getByRole("button", { name: /Comments \(1\)/ }));
    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: "smooth", block: "start" });
  });
});
