import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import userEvent from "@testing-library/user-event";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useChatStore } from "@/stores/chatStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import type { AgentConversationWorkspace } from "@/api/chat";
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
const { workspacesByProject, workspaceCalls } = vi.hoisted(() => ({
  workspacesByProject: new Map<string, AgentConversationWorkspace[]>(),
  workspaceCalls: [] as Array<{
    projectId: string | null;
    enabled?: boolean;
  }>,
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

vi.mock("./useProjectAgentConversationWorkspaces", () => ({
  useProjectAgentConversationWorkspaces: (
    projectId: string | null | undefined,
    options?: { enabled?: boolean }
  ) => {
    workspaceCalls.push({
      projectId: projectId ?? null,
      enabled: options?.enabled,
    });
    return {
      data: workspacesByProject.get(projectId ?? "") ?? [],
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

const workspace = (
  overrides: Partial<AgentConversationWorkspace> = {}
): AgentConversationWorkspace => ({
  conversationId: "conversation-1",
  projectId: "project-1",
  mode: "edit",
  baseRefKind: "project_default",
  baseRef: "main",
  baseDisplayName: "Project default (main)",
  baseCommit: null,
  branchName: "ralphx/demo/agent-conversation-1",
  worktreePath: "/tmp/ralphx/conversation-1",
  linkedIdeationSessionId: null,
  linkedPlanBranchId: null,
  publicationPrNumber: null,
  publicationPrUrl: null,
  publicationPrStatus: null,
  publicationPushStatus: null,
  status: "active",
  createdAt: "2026-04-22T10:00:00Z",
  updatedAt: "2026-04-22T12:00:00Z",
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
    workspacesByProject.clear();
    workspaceCalls.length = 0;
    useChatStore.setState({ activeConversationIds: {}, agentStatus: {} });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true, "project-2": false },
      showAllProjects: true,
      projectSort: "latest",
      sidebarGroupBy: "project",
      sidebarProjectFilterIds: [],
      sidebarPublicationStateFilters: [
        "active",
        "draft",
        "merged",
        "closed",
        "uncommitted",
        "unpushed",
      ],
      pinnedConversationIds: {},
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

  it("shows compact conversation time with a full timestamp title", () => {
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
    expect(row.getByText("2h")).toHaveAttribute(
      "title",
      "Apr 25, 2026, 2:33 PM",
    );
  });

  it("shows compact day labels before switching to a date-only label", () => {
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
    expect(row.getByText("2d")).toHaveAttribute(
      "title",
      "Apr 23, 2026, 12:06 PM",
    );
    expect(row.queryByText(/12:06/)).not.toBeInTheDocument();
    expect(row.queryByText(/days ago/)).not.toBeInTheDocument();
  });

  it("uses PR metadata instead of the base branch and omits implied open state", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 3, 25, 19, 0, 0));
    const activeConversation = conversation({
      createdAt: new Date(2026, 3, 25, 10, 0, 0).toISOString(),
    });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: activeConversation.id,
        publicationPrNumber: 123,
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
      }),
    ]);

    renderSidebar([project({ baseBranch: "develop" })]);

    const row = within(screen.getByTestId("agents-session-conversation-1"));
    expect(row.getByText("PR #123")).toBeInTheDocument();
    expect(row.getByText("9h")).toBeInTheDocument();
    expect(screen.getByTestId("agents-ref-icon-conversation-1")).toHaveAttribute(
      "data-ref-kind",
      "pull-request",
    );
    expect(row.queryByText("develop")).not.toBeInTheDocument();
    expect(row.queryByText("open")).not.toBeInTheDocument();
  });

  it("shows branch metadata and meaningful publication state badges", () => {
    const activeConversation = conversation({ id: "conversation-merged" });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: activeConversation.id,
        baseRef: "feature/base",
        baseDisplayName: "feature/base",
        publicationPrNumber: 77,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      }),
    ]);

    renderSidebar([project({ baseBranch: "main" })]);

    const row = within(screen.getByTestId("agents-session-conversation-merged"));
    expect(row.getByText("PR #77")).toBeInTheDocument();
    expect(row.getByText("merged")).toBeInTheDocument();
    expect(screen.getByTestId("agents-ref-icon-conversation-merged")).toHaveAttribute(
      "data-ref-kind",
      "pull-request",
    );
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
      focusedProjectId: "project-1",
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

  it("renders archived visibility inside Filters and toggles archived sessions", async () => {
    const user = userEvent.setup();
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
    await user.click(screen.getByTestId("agents-filters-trigger"));

    const archivedFilter = screen.getByTestId("agents-filter-archived");
    expect(archivedFilter).toHaveTextContent("Archived");
    expect(archivedFilter).toHaveTextContent("4");
    expect(archivedCountCalls.at(-1)).toEqual(["project-1"]);

    await user.click(within(archivedFilter).getByRole("checkbox"));
    expect(onShowArchivedChange).toHaveBeenCalledWith(true);
  });

  it("keeps selected archived filter styling neutral inside the filters popover", async () => {
    const user = userEvent.setup();
    archivedConversationCounts.set("project-1", 4);

    renderSidebar([project()], { showArchived: true });

    await user.click(screen.getByTestId("agents-filters-trigger"));
    expect(
      screen.getByTestId("agents-filter-popover").getAttribute("style")
    ).toContain("background-color: var(--bg-elevated)");
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

  it("hydrates every project row when the show-all-projects filter is enabled", () => {
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
    expect(archivedCountCalls.at(-1)).toEqual(["project-1", "project-2", "project-3"]);
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

    fireEvent.click(screen.getByTestId("agents-project-row-project-2"));

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

  it("renders Filters then Sort toolbar controls", async () => {
    const user = userEvent.setup();
    conversationsByProject.set("project-1", {
      data: [],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    expect(screen.getByTestId("agents-filter-toolbar")).toHaveClass(
      "flex",
      "mb-2",
      "px-3",
    );
    expect(screen.getByTestId("agents-filter-toolbar").getAttribute("style")).toContain(
      "background-color: var(--bg-surface)",
    );
    expect(screen.getByTestId("agents-filters-trigger")).toHaveTextContent("Filters");
    expect(screen.getByTestId("agents-sort-trigger")).toHaveTextContent("Sort");
    expect(
      screen.getByTestId("agents-filters-trigger").compareDocumentPosition(
        screen.getByTestId("agents-sort-trigger")
      )
    ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(screen.queryByTestId("agents-show-archived-pill")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-filters-trigger"));
    expect(screen.getByTestId("agents-filter-all-projects")).toHaveTextContent(
      "All projects"
    );
    expect(screen.getByTestId("agents-filter-group-by")).toHaveTextContent("Project");
    expect(screen.getByTestId("agents-filter-publication-state-active")).toHaveTextContent(
      "Active"
    );
  });

  it("uses a soft wrapper border for sidebar search focus", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await user.click(screen.getByTestId("agents-search-toggle"));
    const input = screen.getByTestId("agents-search-input");
    const searchFrame = input.parentElement as HTMLElement;

    fireEvent.focus(input);

    await waitFor(() =>
      expect(searchFrame.getAttribute("style")).toContain(
        "border-color: var(--accent-border)"
      )
    );

    fireEvent.blur(input);

    await waitFor(() =>
      expect(searchFrame.getAttribute("style")).toContain(
        "border-color: var(--overlay-weak)"
      )
    );
  });

  it("scopes project hydration until the show-all-projects filter is enabled", async () => {
    const user = userEvent.setup();
    const focused = project({ id: "project-1", name: "alpha" });
    const idle = project({ id: "project-2", name: "beta" });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true, "project-2": false },
      showAllProjects: false,
      projectSort: "latest",
    });

    renderSidebar([focused, idle]);

    expect(
      projectConversationCalls.some((call) => call.projectId === "project-2")
    ).toBe(false);

    await user.click(screen.getByTestId("agents-filters-trigger"));
    await user.click(
      within(screen.getByTestId("agents-filter-all-projects")).getByRole("checkbox")
    );

    await waitFor(() =>
      expect(useAgentSessionStore.getState().showAllProjects).toBe(true),
    );
    expect(
      projectConversationCalls.filter((call) => call.projectId === "project-2").at(-1)
        ?.options?.enabled,
    ).toBe(true);
  });

  it("hydrates individually selected project filters while all projects is off", () => {
    const focused = project({ id: "project-1", name: "alpha" });
    const selected = project({ id: "project-2", name: "beta" });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true, "project-2": true },
      showAllProjects: false,
      sidebarProjectFilterIds: ["project-2"],
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-filtered-project",
          title: "Filtered project",
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

    renderSidebar([focused, selected]);

    expect(screen.queryByTestId("agents-project-project-1")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-project-project-2")).toBeInTheDocument();
    expect(screen.getByTestId("agents-session-conversation-filtered-project"))
      .toHaveTextContent("Filtered project");
    expect(
      projectConversationCalls.filter((call) => call.projectId === "project-2").at(-1)
        ?.options?.enabled,
    ).toBe(true);
  });

  it("keeps the add project footer action alongside the restored controls", () => {
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
    expect(screen.getByTestId("agents-filters-trigger")).toBeInTheDocument();
  });

  it("preserves incoming project order for latest sort and can sort projects alphabetically", async () => {
    const user = userEvent.setup();
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

    await user.click(screen.getByTestId("agents-sort-trigger"));
    await user.click(screen.getByRole("menuitemradio", { name: "A-Z" }));

    expect(useAgentSessionStore.getState().projectSort).toBe("az");
    expect(getProjectRowOrder()).toEqual([
      "agents-project-project-1",
      "agents-project-project-2",
    ]);
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
    const count = within(screen.getByTestId("agents-project-row-project-1")).getByText("6");

    expect(count.className).toContain("group-hover/project-row:opacity-0");
    expect(actions.className).toContain("group-hover/project-row:opacity-100");
    expect(actions.className).not.toContain("group-hover/session:opacity-100");
    expect(trigger.className).toContain("hover:bg-transparent");
    expect(trigger.className).toContain("data-[state=open]:bg-transparent");

    fireEvent.pointerDown(trigger);

    expect(actions.className).toContain("opacity-100");
    expect(count.className).toContain("opacity-0");

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

  it("hides the session status dot when the row action menu is visible", async () => {
    const user = userEvent.setup();
    const conv = conversation({ id: "conversation-menu-overlap", title: "Menu overlap" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    const row = screen.getByTestId("agents-session-conversation-menu-overlap");
    const statusSlot = row.querySelector(".agents-session-status-slot");
    const dot = row.querySelector('span[aria-hidden="true"].rounded-full');
    expect(statusSlot?.className).toContain("h-4");
    expect(statusSlot?.className).toContain("w-4");
    expect(statusSlot?.className).toContain("group-hover/session:opacity-0");
    expect(dot?.className).toContain("block");
    expect(dot?.className).toContain("h-[7px]");
    expect(dot?.className).toContain("w-[7px]");

    const trigger = within(row).getByRole("button", { name: "Session actions" });
    await user.click(trigger);

    expect(trigger.className).toContain("hover:bg-transparent");
    expect(trigger.className).toContain("data-[state=open]:bg-transparent");
    expect(statusSlot?.className).toContain("opacity-0");
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

  it("toggles the whole project row via the agentSessionStore", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true },
      showAllProjects: true,
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

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(projectRow).toHaveAttribute("aria-expanded", "true");

    await user.click(projectRow);
    expect(useAgentSessionStore.getState().expandedProjectIds["project-1"]).toBe(false);
    expect(projectRow).toHaveAttribute("aria-expanded", "false");

    await user.click(projectRow);
    expect(useAgentSessionStore.getState().expandedProjectIds["project-1"]).toBe(true);
    expect(projectRow).toHaveAttribute("aria-expanded", "true");
  });

  it("focuses and expands a project row without selecting a conversation", async () => {
    const user = userEvent.setup();
    const onFocusProject = vi.fn();
    const onSelectConversation = vi.fn();
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": false },
      showAllProjects: true,
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-not-selected", title: "Do not select me" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    renderSidebar([project()], { onFocusProject, onSelectConversation });

    await user.click(screen.getByTestId("agents-project-row-project-1"));

    expect(onFocusProject).toHaveBeenCalledWith("project-1");
    expect(onSelectConversation).not.toHaveBeenCalled();
    expect(useAgentSessionStore.getState().expandedProjectIds["project-1"]).toBe(true);
  });

  it("renders a collapsed project neutrally even when it contains the selected conversation", async () => {
    const user = userEvent.setup();
    const selected = conversation({ id: "conversation-selected", title: "Selected run" });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true },
      showAllProjects: true,
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [selected],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      focusedProjectId: "project-1",
      selectedConversationId: selected.id,
    });

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(projectRow).toHaveAttribute("aria-current", "true");

    await user.click(projectRow);

    expect(projectRow).not.toHaveAttribute("aria-current");
    expect(screen.queryByTestId("agents-session-conversation-selected")).not.toBeInTheDocument();
  });

  it("clears focused project active styling when the project is collapsed", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true },
      showAllProjects: true,
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-counted", title: "Counted run" })],
      total: 46,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      focusedProjectId: "project-1",
      selectedConversationId: null,
    });

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(projectRow).toHaveAttribute("aria-current", "true");
    expect(projectRow).toHaveAttribute("aria-expanded", "true");

    await user.click(projectRow);

    expect(projectRow).toHaveAttribute("aria-expanded", "false");
    expect(projectRow).not.toHaveAttribute("aria-current");
  });

  it("renders the active runtime badge when a project is collapsed but has a running agent", () => {
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": false },
      showAllProjects: true,
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

  it("prepends a pinnedConversation that is not in the loaded conversations list", () => {
    const loaded = conversation({ id: "conversation-loaded", title: "Loaded" });
    const pinned = conversation({ id: "conversation-pinned", title: "Pinned run" });
    conversationsByProject.set("project-1", {
      data: [loaded],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      pinnedConversation: pinned,
      selectedConversationId: pinned.id,
    });

    const rows = screen.getAllByTestId(/agents-session-/);
    expect(rows.map((row) => row.getAttribute("data-testid"))).toEqual([
      "agents-session-conversation-pinned",
      "agents-session-conversation-loaded",
    ]);
  });

  it("does not duplicate a pinnedConversation already present in the loaded list", () => {
    const shared = conversation({ id: "conversation-shared", title: "Shared run" });
    conversationsByProject.set("project-1", {
      data: [shared],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      pinnedConversation: shared,
      selectedConversationId: shared.id,
    });

    const rows = screen.getAllByTestId(/agents-session-/);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveAttribute("data-testid", "agents-session-conversation-shared");
  });

  it("pins and unpins a session from the row action menu", async () => {
    const user = userEvent.setup();
    const older = conversation({
      id: "conversation-older",
      title: "Older",
      createdAt: "2026-04-22T10:00:00Z",
    });
    const newer = conversation({
      id: "conversation-newer",
      title: "Newer",
      createdAt: "2026-04-22T12:00:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [newer, older],
      total: 2,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    await user.click(
      within(screen.getByTestId("agents-session-conversation-older")).getByRole(
        "button",
        { name: "Session actions" }
      )
    );
    await user.click(screen.getByText("Pin session"));

    expect(
      useAgentSessionStore.getState().pinnedConversationIds["conversation-older"]
    ).toBe(true);
    expect(
      screen.getAllByTestId(/agents-session-/).map((row) => row.getAttribute("data-testid"))
    ).toEqual([
      "agents-session-conversation-older",
      "agents-session-conversation-newer",
    ]);
    expect(screen.getByTestId("agents-pin-icon-conversation-older")).toBeInTheDocument();

    await user.click(
      within(screen.getByTestId("agents-session-conversation-older")).getByRole(
        "button",
        { name: "Session actions" }
      )
    );
    await user.click(screen.getByText("Unpin session"));
    expect(
      useAgentSessionStore.getState().pinnedConversationIds["conversation-older"]
    ).toBeUndefined();
  });

  it("uses the pinned icon as the colored live status slot for pinned running sessions", () => {
    const conv = conversation({ id: "conversation-pinned-running", title: "Pinned live" });
    const storeKey = getAgentConversationStoreKey(conv);
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useAgentSessionStore.setState({
      pinnedConversationIds: { [conv.id]: true },
    });
    useChatStore.setState({
      activeConversationIds: { [storeKey]: conv.id },
      agentStatus: { [storeKey]: "running" },
    });

    renderSidebar();

    expect(
      screen
        .getByTestId("agents-pin-icon-conversation-pinned-running")
        .getAttribute("style")
    ).toContain("color: var(--accent-primary)");
  });

  it("groups conversations by publication state when selected in Filters", async () => {
    const user = userEvent.setup();
    const merged = conversation({ id: "conversation-merged", title: "Merged run" });
    const closed = conversation({ id: "conversation-closed", title: "Closed run" });
    conversationsByProject.set("project-1", {
      data: [merged, closed],
      total: 2,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: merged.id,
        publicationPrNumber: 11,
        publicationPrStatus: "merged",
      }),
      workspace({
        conversationId: closed.id,
        publicationPrNumber: 12,
        publicationPrStatus: "closed",
      }),
    ]);

    renderSidebar([project()]);

    await user.click(screen.getByTestId("agents-filters-trigger"));
    await user.click(screen.getByRole("radio", { name: "Publication state" }));

    expect(screen.getByTestId("agents-publication-group-merged")).toHaveTextContent(
      "Merged"
    );
    expect(screen.getByTestId("agents-publication-group-closed")).toHaveTextContent(
      "Closed"
    );
    expect(screen.queryByTestId("agents-project-row-project-1")).not.toBeInTheDocument();
  });

  it("closes the rename dialog via Escape (onOpenChange false branch)", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn();
    const conv = conversation({ id: "conversation-esc-rename", title: "old" });
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

    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByLabelText("Session title")).toBeNull());
    expect(onRenameConversation).not.toHaveBeenCalled();
  });

  it("renders the done status dot when the active runtime status is completed", () => {
    const conv = conversation({ id: "conversation-done", title: "Done run" });
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
      agentStatus: { [storeKey]: "completed" },
    });

    renderSidebar();

    // SessionRuntimeLabel only renders for "running" — "done" returns null (line 859).
    expect(screen.queryByText("running")).not.toBeInTheDocument();

    // SessionStatusDot for "done" uses --status-success (line 889 branch).
    const row = screen.getByTestId("agents-session-conversation-done");
    const dot = row.querySelector('span[aria-hidden="true"].rounded-full');
    expect(dot).not.toBeNull();
    expect(dot?.className).toContain("block");
    expect(dot?.getAttribute("style") ?? "").toContain("var(--status-success)");
  });

  it("renders no status dot for blocked failed/error/needs_approval statuses", () => {
    const conv = conversation({ id: "conversation-blocked", title: "Blocked run" });
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
      agentStatus: { [storeKey]: "failed" },
    });

    renderSidebar();

    // "blocked" state — neither running label nor success/accent dot rendered (lines 851, 859).
    expect(screen.queryByText("running")).not.toBeInTheDocument();
    const row = screen.getByTestId("agents-session-conversation-blocked");
    const dot = row.querySelector('span[aria-hidden="true"].rounded-full');
    // SessionStatusDot returns null for "blocked", so no rounded-full status dot present.
    expect(dot).toBeNull();
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
