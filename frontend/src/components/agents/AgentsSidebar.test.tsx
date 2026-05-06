import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import userEvent from "@testing-library/user-event";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useChatStore } from "@/stores/chatStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import type { Project } from "@/types/project";
import type { AgentConversation } from "./agentConversations";
import {
  formatAgentConversationCreatedAt,
  getAgentConversationStoreKey,
} from "./agentConversations";
import { AgentsSidebar } from "./AgentsSidebar";

type ConversationsResult = {
  data: AgentConversation[];
  isLoading: boolean;
  total?: number;
  hasNextPage?: boolean;
  isFetchingNextPage?: boolean;
  fetchNextPage?: () => Promise<unknown>;
};

const { conversationsByProject } = vi.hoisted(() => ({
  conversationsByProject: new Map<string, ConversationsResult>(),
}));
const { projectConversationCalls } = vi.hoisted(() => ({
  projectConversationCalls: [] as Array<{
    projectId: string | null;
    includeArchived: boolean;
    options?: { search?: string; enabled?: boolean };
  }>,
}));
const { archivedConversationCounts, archivedCountCalls } = vi.hoisted(() => ({
  archivedConversationCounts: new Map<string, number>(),
  archivedCountCalls: [] as string[][],
}));

vi.mock("./useProjectAgentConversations", () => ({
  useProjectAgentConversations: (
    projectId: string | null | undefined,
    includeArchived = false,
    options?: { search?: string; enabled?: boolean }
  ) =>
    (() => {
      projectConversationCalls.push({
        projectId: projectId ?? null,
        includeArchived,
        options,
      });
      const result = conversationsByProject.get(projectId ?? "");
      if (result) {
        return {
          ...result,
          total: result.total ?? result.data.length,
        };
      }
      return {
        data: [],
        isLoading: false,
        total: 0,
        hasNextPage: false,
        isFetchingNextPage: false,
        fetchNextPage: vi.fn(),
      };
    })(),
}));

vi.mock("./useArchivedConversationCounts", () => ({
  useArchivedConversationCounts: (projectIds: string[]) => {
    archivedCountCalls.push(projectIds);
    const byProjectId = Object.fromEntries(
      projectIds.map((projectId) => [projectId, archivedConversationCounts.get(projectId) ?? 0])
    );
    const totalArchivedCount = Object.values(byProjectId).reduce(
      (sum, count) => sum + count,
      0
    );

    return {
      byProjectId,
      totalArchivedCount,
      isLoading: false,
    };
  },
}));

const project = (overrides: Partial<Project> = {}): Project => ({
  id: "project-1",
  name: "ralphx",
  workingDirectory: "/tmp/ralphx",
  gitMode: "worktree",
  baseBranch: null,
  worktreeParentDirectory: null,
  useFeatureBranches: true,
  mergeValidationMode: "block",
  detectedAnalysis: null,
  customAnalysis: null,
  analyzedAt: null,
  githubPrEnabled: false,
  createdAt: "2026-04-22T09:00:00Z",
  updatedAt: "2026-04-22T09:00:00Z",
  ...overrides,
});

const conversation = (
  overrides: Partial<AgentConversation> = {}
): AgentConversation => ({
  id: "conversation-1",
  contextType: "project",
  contextId: "project-1",
  claudeSessionId: null,
  providerSessionId: "thread-1",
  providerHarness: "codex",
  upstreamProvider: null,
  providerProfile: null,
  title: "Fix font scaling",
  messageCount: 1,
  lastMessageAt: "2026-04-22T12:00:00Z",
  createdAt: "2026-04-22T10:00:00Z",
  updatedAt: "2026-04-22T12:00:00Z",
  archivedAt: null,
  projectId: "project-1",
  ideationSessionId: null,
  ...overrides,
});

function renderSidebar(
  projects: Project[] = [project()],
  props?: Partial<ComponentProps<typeof AgentsSidebar>>
) {
  return render(
    <TooltipProvider delayDuration={0}>
      <AgentsSidebar
        projects={projects}
        focusedProjectId="project-1"
        selectedConversationId={null}
        onFocusProject={vi.fn()}
        onSelectConversation={vi.fn()}
        onCreateAgent={vi.fn()}
        onCreateProject={vi.fn()}
        onArchiveProject={vi.fn()}
        onRenameConversation={vi.fn()}
        onArchiveConversation={vi.fn()}
        onRestoreConversation={vi.fn()}
        showArchived={false}
        onShowArchivedChange={vi.fn()}
        {...props}
      />
    </TooltipProvider>
  );
}

function getProjectRowOrder() {
  return screen
    .getAllByTestId((testId) => testId.startsWith("agents-project-project-"))
    .map((row) => row.getAttribute("data-testid"));
}

describe("AgentsSidebar", () => {
  beforeEach(() => {
    conversationsByProject.clear();
    projectConversationCalls.length = 0;
    archivedConversationCounts.clear();
    archivedCountCalls.length = 0;
    useChatStore.setState({ activeConversationIds: {}, agentStatus: {} });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true, "project-2": false },
      showAllProjects: false,
      projectSort: "latest",
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("uses flat v27 panel chrome without light-theme blur or glow", () => {
    renderSidebar();

    const sidebar = screen.getByTestId("agents-sidebar");
    const inlineStyle = sidebar.getAttribute("style") ?? "";
    expect(inlineStyle).toContain("background-color: var(--app-sidebar-bg)");
    expect(inlineStyle).toContain("border-right-color: var(--app-sidebar-border)");
    expect(inlineStyle).toContain("box-shadow: none");
    expect(inlineStyle).not.toContain("backdrop");

    expect(screen.getByTestId("agents-new-agent")).toHaveTextContent("New");
    expect(screen.getByTestId("agents-new-agent").className).toContain("h-7");
    expect(screen.getByTestId("agents-add-project").className).toContain("rounded-[6px]");
  });

  it("orders sessions by created time and shows created time instead of provider", () => {
    const older = conversation({
      id: "older",
      title: "Older agent",
      createdAt: "2026-04-22T10:00:00Z",
      lastMessageAt: "2026-04-22T12:00:00Z",
    });
    const newer = conversation({
      id: "newer",
      title: "Newer agent",
      createdAt: "2026-04-22T11:00:00Z",
      lastMessageAt: "2026-04-22T11:01:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [newer, older],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const rows = screen.getAllByTestId(/agents-session-/);
    expect(rows.map((row) => row.getAttribute("data-testid"))).toEqual([
      "agents-session-newer",
      "agents-session-older",
    ]);

    const firstRow = within(rows[0]);
    expect(firstRow.getByText("Newer agent")).toBeInTheDocument();
    expect(
      firstRow.getByText(formatAgentConversationCreatedAt(newer.createdAt))
    ).toBeInTheDocument();
    expect(firstRow.queryByText("codex")).not.toBeInTheDocument();
  });

  it("shows human-diff conversation time with a full timestamp title", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 3, 25, 16, 33, 0));
    const activeConversation = conversation({
      createdAt: new Date(2026, 3, 25, 14, 33, 0).toISOString(),
    });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const row = within(screen.getByTestId("agents-session-conversation-1"));
    expect(row.getByText("2 hours ago")).toHaveAttribute(
      "title",
      "Apr 25, 2026, 2:33 PM",
    );
  });

  it("shows a date without time for conversations older than one day", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 3, 25, 16, 33, 0));
    const activeConversation = conversation({
      createdAt: new Date(2026, 3, 23, 12, 6, 0).toISOString(),
    });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const row = within(screen.getByTestId("agents-session-conversation-1"));
    expect(row.getByText("Apr 23")).toHaveAttribute(
      "title",
      "Apr 23, 2026, 12:06 PM",
    );
    expect(row.queryByText(/12:06/)).not.toBeInTheDocument();
    expect(row.queryByText(/days ago/)).not.toBeInTheDocument();
  });

  it("only shows a runtime label for running conversations", () => {
    const idleConversation = conversation({ id: "conversation-idle" });
    const runningConversation = conversation({
      id: "conversation-running",
      title: "Running agent",
    });
    const runningStoreKey = getAgentConversationStoreKey(runningConversation);
    conversationsByProject.set("project-1", {
      data: [runningConversation, idleConversation],
      total: 2,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [runningStoreKey]: runningConversation.id },
      agentStatus: { [runningStoreKey]: "running" },
    });

    renderSidebar();

    expect(screen.getByTestId("agents-session-conversation-running")).toHaveTextContent(
      "running"
    );
    expect(screen.queryByText("queued")).not.toBeInTheDocument();
    expect(screen.queryByText("done")).not.toBeInTheDocument();
    expect(screen.queryByText("blocked")).not.toBeInTheDocument();
  });

  it("shows load more per project and calls the paginated fetch when pressed", () => {
    const fetchNextPage = vi.fn().mockResolvedValue(undefined);
    conversationsByProject.set("project-1", {
      data: [conversation()],
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage,
    });

    renderSidebar();

    fireEvent.click(screen.getByTestId("agents-load-more-project-1"));
    expect(fetchNextPage).toHaveBeenCalledTimes(1);
  });

  it("shows the backend total session count rather than the loaded page size", () => {
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1" }), conversation({ id: "conversation-2" })],
      total: 11,
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    expect(screen.getByText("11")).toBeInTheDocument();
  });

  it("uses design-system-owned active project and session highlight state", () => {
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-active", title: "Selected run" })],
      total: 4,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      focusedProjectId: null,
      selectedConversationId: "conversation-active",
    });

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(projectRow).toHaveClass("agents-project-row");
    expect(projectRow).toHaveAttribute("aria-current", "true");
    expect(projectRow.getAttribute("style") ?? "").not.toContain("rgba(255");
    expect(within(projectRow).getByText("4")).toHaveClass("agents-project-count");

    const sessionRow = within(screen.getByTestId("agents-session-conversation-active"))
      .getByRole("button", { name: /Selected run/ });
    expect(sessionRow).toHaveClass("agents-session-row");
    expect(sessionRow).toHaveAttribute("aria-current", "true");
    expect(sessionRow.getAttribute("style") ?? "").not.toContain("rgba(255");
    expect(within(sessionRow).getByText("master").closest(".agents-session-meta")).toBeTruthy();
  });

  it("omits the legacy archived filter pill in the v27 sidebar", () => {
    const onShowArchivedChange = vi.fn();
    archivedConversationCounts.set("project-1", 4);
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onShowArchivedChange });

    expect(screen.queryByTestId("agents-show-archived-pill")).not.toBeInTheDocument();
    expect(onShowArchivedChange).not.toHaveBeenCalled();
    expect(archivedCountCalls).toHaveLength(0);
  });

  it("renders the static v27 Recent block above the add-project action", () => {
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const recent = screen.getByTestId("agents-static-recent");
    // Static recent block is rendered but hidden ("Coming soon") via aria-hidden + display:none
    expect(recent).toHaveAttribute("aria-hidden", "true");
    expect(within(recent).getByText("Recent", { selector: "span" })).toBeInTheDocument();
    expect(
      within(recent).getByRole("button", { name: "View all", hidden: true }),
    ).toBeInTheDocument();
    expect(within(recent).getByText("Add ranking to reefbot homepage")).toBeInTheDocument();
    expect(within(recent).getByText("Tighten kanban drag handles")).toBeInTheDocument();
    expect(screen.getByTestId("agents-add-project")).toBeInTheDocument();
  });

  it("shows empty projects by default in the v27 tree", () => {
    conversationsByProject.set("project-1", {
      data: [],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    expect(screen.getByTestId("agents-project-project-1")).toBeInTheDocument();
    expect(screen.queryByText("No chats yet.")).not.toBeInTheDocument();
    expect(screen.queryByText("Start")).not.toBeInTheDocument();
  });

  it("hydrates every project row in the v27 sidebar tree", () => {
    const focused = project({ id: "project-1", name: "alpha" });
    const idle = project({ id: "project-2", name: "beta" });
    const anotherIdle = project({ id: "project-3", name: "gamma" });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [conversation({ id: "conversation-2", projectId: "project-2", contextId: "project-2" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([focused, idle, anotherIdle]);

    expect(projectConversationCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          projectId: "project-1",
          options: expect.objectContaining({ enabled: true }),
        }),
        expect.objectContaining({
          projectId: "project-2",
          options: expect.objectContaining({ enabled: true }),
        }),
        expect.objectContaining({
          projectId: "project-3",
          options: expect.objectContaining({ enabled: true }),
        }),
      ])
    );
    expect(archivedCountCalls).toHaveLength(0);
  });

  it("collapses the previously expanded project when another project opens", () => {
    const first = project({ id: "project-1", name: "alpha" });
    const second = project({ id: "project-2", name: "beta" });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1", title: "First run" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-2",
          title: "Second run",
          projectId: "project-2",
          contextId: "project-2",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([first, second]);

    expect(screen.getByTestId("agents-session-conversation-1")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-session-conversation-2")).not.toBeInTheDocument();

    fireEvent.click(
      within(screen.getByTestId("agents-project-project-2")).getByRole("button", {
        name: "Expand project",
      })
    );

    expect(screen.queryByTestId("agents-session-conversation-1")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-session-conversation-2")).toBeInTheDocument();
    expect(useAgentSessionStore.getState().expandedProjectIds).toMatchObject({
      "project-1": false,
      "project-2": true,
    });
  });

  it("searches conversations on the backend across projects without matching project names", async () => {
    const focused = project({ id: "project-1", name: "alpha" });
    const idle = project({ id: "project-2", name: "beta" });
    conversationsByProject.set("project-1", {
      data: [],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-search",
          title: "Fix sidebar search",
          projectId: "project-2",
          contextId: "project-2",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([focused, idle]);

    fireEvent.click(screen.getByTestId("agents-search-toggle"));
    fireEvent.change(screen.getByTestId("agents-search-input"), {
      target: { value: "sidebar" },
    });

    await waitFor(() =>
      expect(projectConversationCalls).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            projectId: "project-2",
            options: expect.objectContaining({
              enabled: true,
              search: "sidebar",
            }),
          }),
        ])
      )
    );
    expect(screen.getByTestId("agents-session-conversation-search")).toHaveTextContent(
      "Fix sidebar search"
    );
  });

  it("does not render the removed v27 filter and sort pills", () => {
    conversationsByProject.set("project-1", {
      data: [],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    expect(screen.queryByTestId("agents-show-all-projects-pill")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-project-sort-pill")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-show-archived-pill")).not.toBeInTheDocument();
  });

  it("shows an add project footer action instead of the archived switch", () => {
    const onCreateProject = vi.fn();
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onCreateProject });

    fireEvent.click(screen.getByTestId("agents-add-project"));

    expect(onCreateProject).toHaveBeenCalledTimes(1);
    expect(screen.queryByLabelText("Show archived sessions")).not.toBeInTheDocument();
  });

  it("preserves incoming project order without a sort pill", () => {
    const alpha = project({ id: "project-1", name: "alpha" });
    const beta = project({ id: "project-2", name: "beta" });

    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1", projectId: "project-1", contextId: "project-1" })],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [conversation({ id: "conversation-2", projectId: "project-2", contextId: "project-2" })],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([beta, alpha]);

    expect(getProjectRowOrder()).toEqual([
      "agents-project-project-2",
      "agents-project-project-1",
    ]);
    expect(screen.queryByTestId("agents-project-sort-pill")).not.toBeInTheDocument();
  });

  it("keeps project actions visible while open and confirms before archiving", () => {
    const onArchiveProject = vi.fn();
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onArchiveProject });

    const actions = screen.getByTestId("agents-project-actions-project-1");
    const trigger = within(actions).getByRole("button", { name: "Project actions" });

    fireEvent.pointerDown(trigger);

    expect(actions.className).toContain("opacity-100");

    fireEvent.click(screen.getByText("Archive project"));

    expect(screen.getByText("Archive project?")).toBeInTheDocument();
    expect(onArchiveProject).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Archive project" }));

    expect(onArchiveProject).toHaveBeenCalledWith("project-1");
  });

  it("does not show a tooltip for project actions", async () => {
    const user = userEvent.setup();
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    const actions = screen.getByTestId("agents-project-actions-project-1");
    const trigger = within(actions).getByRole("button", { name: "Project actions" });

    await user.hover(trigger);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("opens a rename dialog from session actions and saves the new title", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    const activeConversation = conversation({ id: "conversation-rename", title: "Untitled agent" });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onRenameConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));

    const input = screen.getByLabelText("Session title");
    await user.clear(input);
    await user.type(input, "Review follow-up");
    await user.click(screen.getByRole("button", { name: "Rename session" }));

    await waitFor(() =>
      expect(onRenameConversation).toHaveBeenCalledWith("conversation-rename", "Review follow-up")
    );
    expect(screen.queryByText("Rename session")).not.toBeInTheDocument();
  });

  it("confirms before archiving a session", async () => {
    const user = userEvent.setup();
    const onArchiveConversation = vi.fn();
    const activeConversation = conversation({ id: "conversation-archive", title: "Untitled agent" });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onArchiveConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Archive session"));

    expect(screen.getByText("Archive session?")).toBeInTheDocument();
    expect(onArchiveConversation).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Archive session" }));

    expect(onArchiveConversation).toHaveBeenCalledWith(activeConversation);
  });

  it("toggles the sidebar search input and clears the query via the X button", async () => {
    const user = userEvent.setup();
    renderSidebar();

    expect(screen.queryByTestId("agents-search-input")).toBeNull();
    await user.click(screen.getByTestId("agents-search-toggle"));
    const input = screen.getByTestId("agents-search-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "alpha" } });
    expect(input.value).toBe("alpha");

    await user.click(screen.getByLabelText("Clear search"));
    expect(input.value).toBe("");

    // Toggling search closed clears the query and removes the input row.
    fireEvent.change(input, { target: { value: "beta" } });
    await user.click(screen.getByTestId("agents-search-toggle"));
    expect(screen.queryByTestId("agents-search-input")).toBeNull();
  });

  it("toggles the project chevron via the agentSessionStore", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true },
      showAllProjects: false,
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const chevron = screen.getByLabelText("Collapse project");
    await user.click(chevron);
    expect(useAgentSessionStore.getState().expandedProjectIds["project-1"]).toBe(false);

    await user.click(screen.getByLabelText("Expand project"));
    expect(useAgentSessionStore.getState().expandedProjectIds["project-1"]).toBe(true);
  });

  it("focuses the project when the row title is clicked", async () => {
    const user = userEvent.setup();
    const onFocusProject = vi.fn();
    conversationsByProject.set("project-1", {
      data: [],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    renderSidebar([project()], { onFocusProject });

    await user.click(screen.getByText("ralphx"));
    expect(onFocusProject).toHaveBeenCalledWith("project-1");
  });

  it("renders the active runtime badge when a project is collapsed but has a running agent", () => {
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": false },
      showAllProjects: false,
      projectSort: "latest",
    });
    const conv = conversation({ id: "conversation-running", title: "Running" });
    const storeKey = getAgentConversationStoreKey(conv);
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [storeKey]: conv.id },
      agentStatus: { [storeKey]: "generating" },
    });

    renderSidebar([project()], { focusedProjectId: null });

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(within(projectRow).getByText("1")).toBeInTheDocument();
  });

  it("selects a conversation row when clicked", async () => {
    const user = userEvent.setup();
    const onSelectConversation = vi.fn();
    const conv = conversation({ id: "conversation-pick", title: "Pick me" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onSelectConversation });

    await user.click(screen.getByText("Pick me"));
    expect(onSelectConversation).toHaveBeenCalledWith("project-1", conv);
  });

  it("submits a rename via the Enter key inside the dialog", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    const conv = conversation({ id: "conversation-rename-2", title: "old" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onRenameConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    const titleInput = screen.getByLabelText("Session title");
    await user.clear(titleInput);
    await user.type(titleInput, "renamed-via-enter{Enter}");
    await waitFor(() =>
      expect(onRenameConversation).toHaveBeenCalledWith(
        "conversation-rename-2",
        "renamed-via-enter",
      ),
    );
  });

  it("cancel button in rename dialog closes without invoking onRenameConversation", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn();
    const conv = conversation({ id: "conversation-cancel-rename", title: "old" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onRenameConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    expect(screen.getByLabelText("Session title")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByLabelText("Session title")).toBeNull());
    expect(onRenameConversation).not.toHaveBeenCalled();
  });

  it("restores an archived conversation via the row dropdown", async () => {
    const user = userEvent.setup();
    const onRestoreConversation = vi.fn();
    const archived = conversation({
      id: "conversation-archived",
      title: "Old run",
      archivedAt: "2026-04-22T13:00:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [archived],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      showArchived: true,
      onRestoreConversation,
    });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Restore session"));
    expect(onRestoreConversation).toHaveBeenCalledWith(archived);
  });

  it("renders the running runtime label and accent status dot for a generating conversation", () => {
    const conv = conversation({ id: "conversation-run", title: "Live run" });
    const storeKey = getAgentConversationStoreKey(conv);
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [storeKey]: conv.id },
      agentStatus: { [storeKey]: "generating" },
    });

    renderSidebar();

    expect(screen.getByText("running")).toBeInTheDocument();
  });

  it("rename Submit no-ops when the dialog is closed before submitting", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    const conv = conversation({ id: "conversation-rename-3", title: "" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onRenameConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    const titleInput = screen.getByLabelText("Session title");
    // Clear and submit Enter without any new value — trimmed length === 0 path.
    await user.clear(titleInput);
    await user.keyboard("{Enter}");
    expect(onRenameConversation).not.toHaveBeenCalled();
  });
});
