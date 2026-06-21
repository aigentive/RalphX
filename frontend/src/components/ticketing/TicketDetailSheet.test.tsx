import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { openUrlMock } = vi.hoisted(() => ({ openUrlMock: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));

import type {
  TicketAssociations,
  TicketDetail,
  TicketingCapabilities,
  TicketSummary,
  TicketTransitionOption,
} from "@/api/ticketing";
import { TooltipProvider } from "@/components/ui/tooltip";

import { TicketDetailSheet } from "./TicketDetailSheet";

const baseCapabilities: TicketingCapabilities = {
  supportsBoards: true,
  supportsKanban: true,
  kanbanWrite: true,
  statusWrite: true,
  assignmentWrite: true,
  commentWrite: true,
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
};

const writableTransition: TicketTransitionOption = {
  toStateId: "done",
  name: "Done",
  category: "done",
};

function renderSheet(
  overrides: {
    ticket?: TicketSummary;
    capabilities?: TicketingCapabilities;
  } = {},
) {
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
        onClose={vi.fn()}
      />
    </TooltipProvider>,
  );
}

describe("TicketDetailSheet assignee control", () => {
  it("offers 'Assign to me' only when the ticket is unassigned", () => {
    renderSheet();

    expect(screen.getByRole("button", { name: /assign to me/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /clear assignee/i })).not.toBeInTheDocument();
  });

  it("shows the assignee and hides 'Assign to me' when assigned", () => {
    renderSheet({
      ticket: { ...baseTicket, assignee: { name: "Adrian Demian" } },
    });

    expect(screen.getByText("Assignee")).toBeInTheDocument();
    expect(screen.getByText("Adrian Demian")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /assign to me/i })).not.toBeInTheDocument();
    // Clear assignee stays available because assignment write-back is enabled.
    expect(screen.getByRole("button", { name: /clear assignee/i })).toBeInTheDocument();
  });

  it("shows the assignee read-only when assignment write-back is unavailable", () => {
    renderSheet({
      ticket: { ...baseTicket, assignee: { name: "Adrian Demian" } },
      capabilities: { ...baseCapabilities, assignmentWrite: false },
    });

    expect(screen.getByText("Adrian Demian")).toBeInTheDocument();
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
    expect((status as HTMLSelectElement).style.backgroundColor).toBe("var(--bg-elevated)");
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

    expect(screen.getByRole("img", { name: "Pull request" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /branch only/i })).toBeInTheDocument();
    expect(screen.getByText("PR #7")).toBeInTheDocument();
    expect(screen.getByText("ralphx/p/agent-2")).toBeInTheDocument();
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
