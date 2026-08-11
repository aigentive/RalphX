import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { chatApi } from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { agentConversationKeys } from "@/components/agents/useProjectAgentConversations";
import { chatKeys } from "@/hooks/useChat";
import { getQueryClient } from "@/lib/queryClient";
import { api } from "@/lib/tauri";
import { markPostUpdatePreparing } from "@/lib/postUpdatePreparing";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import { useChatStore } from "@/stores/chatStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { useProposalStore } from "@/stores/proposalStore";
import { useProjectStore } from "@/stores/projectStore";
import { useExecutionStatus } from "@/hooks/useExecutionControl";
import { useExecutionEvents } from "@/hooks/useExecutionEvents";
import { useRunningProcesses } from "@/hooks/useRunningProcesses";
import { useMergePipeline } from "@/hooks/useMergePipeline";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { attentionKeys } from "@/hooks/useAttentionItems";
import { toast } from "sonner";
import type { Project } from "@/types/project";

const { sonnerToasterMock } = vi.hoisted(() => ({
  sonnerToasterMock: vi.fn(() => null),
}));
const { preloadAutomationsViewMock } = vi.hoisted(() => ({
  preloadAutomationsViewMock: vi.fn(),
}));
const { resolveTaskAgentWorkspaceMock } = vi.hoisted(() => ({
  resolveTaskAgentWorkspaceMock: vi.fn(),
}));

function AutomationsViewMock({
  onNewAutomation,
}: {
  onNewAutomation: () => void;
}) {
  return (
    <button data-testid="automations-new-automation" onClick={onNewAutomation} type="button">
      New automation
    </button>
  );
}

vi.mock("sonner", () => ({
  Toaster: sonnerToasterMock,
  toast: {
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/components/automations/preloadAutomationsView", () => ({
  preloadAutomationsView: preloadAutomationsViewMock,
}));

vi.mock("@/api/tasks", () => ({
  tasksApi: {
    pause: vi.fn(),
    stop: vi.fn(),
    resolveAgentWorkspace: resolveTaskAgentWorkspaceMock,
  },
  stepsApi: {
    getByTask: vi.fn(),
    create: vi.fn(),
    update: vi.fn(),
    reorder: vi.fn(),
    getProgress: vi.fn(),
    start: vi.fn(),
    complete: vi.fn(),
    skip: vi.fn(),
    fail: vi.fn(),
  },
}));

Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

// App shell tests exercise navigation/layout behavior. Keep global startup
// listeners out of this suite; EventProvider and PermissionDialog have their
// own focused coverage.
vi.mock("@/providers/EventProvider", () => ({
  EventProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  useEventBus: () => ({
    subscribe: vi.fn(() => () => {}),
    emit: vi.fn(),
  }),
}));

// Mock the useEvents hooks to prevent Tauri API calls
vi.mock("@/hooks/useEvents", () => ({
  useTaskEvents: vi.fn(),
  useProposalEvents: vi.fn(),
  useStepEvents: vi.fn(),
  useSupervisorAlerts: vi.fn(),
  useReviewEvents: vi.fn(),
  useFileChangeEvents: vi.fn(),
  useAgentEvents: vi.fn(),
  useExecutionErrorEvents: vi.fn(),
  useRecoveryPromptEvents: vi.fn(),
}));

vi.mock("@/components/PermissionDialog", () => ({
  PermissionDialog: () => null,
}));

// Mock ExtensibilityView
vi.mock("@/components/ExtensibilityView", () => ({
  ExtensibilityView: () => <div data-testid="extensibility-view-mock">Extensibility View</div>,
}));

// Mock ActivityView
vi.mock("@/components/activity", () => ({
  ActivityView: ({ showHeader }: { showHeader?: boolean }) => (
    <div data-testid="activity-view-mock">Activity View {showHeader && "(with header)"}</div>
  ),
}));

// Capture the props the App passes to the TicketingDashboardView so we can
// drive its onNavigateToAssociation callback from tests without a real backend.
const ticketingViewProps = vi.hoisted(() => ({
  current: null as null | {
    projectId: string | null;
    onNavigateToAssociation: (deepLink: {
      view: string;
      id: string;
      projectId?: string | null;
      conversationId?: string | null;
    }) => void;
  },
}));

// Mock the ticketing dashboard surface (and its data hooks) so navigating to
// the ticketing view does not require live Tauri ticketing endpoints.
vi.mock("@/components/ticketing", () => ({
  TicketingDashboardView: (props: {
    projectId: string | null;
    onNavigateToAssociation: (deepLink: {
      view: string;
      id: string;
      projectId?: string | null;
      conversationId?: string | null;
    }) => void;
  }) => {
    ticketingViewProps.current = props;
    return (
      <div
        data-testid="ticketing-dashboard-view-mock"
        data-project-id={props.projectId ?? ""}
      >
        Ticketing Dashboard View
      </div>
    );
  },
}));

vi.mock("@/hooks/useTicketingEvents", () => ({
  useTicketingCacheEvents: vi.fn(),
}));

// Mock the lazy Agents modules so App shell coverage does not hydrate its controller.
vi.mock("@/components/agents/AgentIssueReportDialog", () => ({
  AgentIssueReportDialog: () => null,
}));

vi.mock("@/components/agents/AgentsView", () => ({
  AgentsView: ({
    footer,
    onCreateProject,
  }: {
    footer?: React.ReactNode;
    onCreateProject?: () => void;
  }) => (
    <div data-testid="agents-view-mock">
      Agents View
      <button type="button" data-testid="agents-create-project" onClick={onCreateProject}>
        Add project
      </button>
      {footer && <div data-testid="agents-footer-mock">{footer}</div>}
    </div>
  ),
}));

// Mock UpdateChecker to avoid delayed non-critical updater checks during shell tests.
vi.mock("@/components/UpdateChecker", () => ({
  UpdateChecker: () => null,
}));

vi.mock("@/components/ProviderCliUpdateChecker", () => ({
  ProviderCliUpdateChecker: () => null,
}));

// Mock SettingsView (still exported from index for backward compat, but no longer used in App)
vi.mock("@/components/settings", () => ({
  SettingsView: () => <div data-testid="settings-view-mock">Settings View</div>,
}));

// Mock SettingsDialog — the new modal-based settings overlay
vi.mock("@/components/settings/SettingsDialog", () => ({
  default: () => <div data-testid="settings-dialog-mock">Settings Dialog</div>,
}));

// Mock ProjectSelector
vi.mock("@/components/projects/ProjectSelector", () => ({
  ProjectSelector: ({ onNewProject }: { onNewProject?: () => void }) => (
    <button
      data-testid="project-selector-mock"
      onClick={onNewProject}
      aria-label="Select project"
    >
      Demo Project
    </button>
  ),
}));

// Mock ProjectCreationWizard
vi.mock("@/components/projects/ProjectCreationWizard", () => ({
  ProjectCreationWizard: ({
    isOpen,
    onCreate,
  }: {
    isOpen: boolean;
    onCreate: (project: {
      name: string;
      workingDirectory: string;
      gitMode: "worktree";
      baseBranch: string;
    }) => void | Promise<void>;
  }) =>
    isOpen ? (
      <button
        type="button"
        data-testid="project-wizard-create"
        onClick={() =>
          void onCreate({
            name: "Created Project",
            workingDirectory: "/tmp/created-project",
            gitMode: "worktree",
            baseBranch: "main",
          })
        }
      >
        Create Project
      </button>
    ) : null,
}));

// Mock proposal hooks
vi.mock("@/hooks/useProposals", () => ({
  useProposalMutations: vi.fn().mockReturnValue({
    createProposal: { mutateAsync: vi.fn() },
    updateProposal: { mutateAsync: vi.fn() },
    deleteProposal: { mutate: vi.fn() },
    reorder: { mutate: vi.fn() },
  }),
}));

// Mock apply proposals hook
vi.mock("@/hooks/useApplyProposals", () => ({
  useApplyProposals: vi.fn().mockReturnValue({
    apply: {
      mutateAsync: vi.fn(),
      isPending: false,
    },
  }),
}));

// Mock execution hooks
vi.mock("@/hooks/useExecutionControl", () => ({
  useExecutionStatus: vi.fn(),
}));

vi.mock("@/hooks/useExecutionEvents", () => ({
  useExecutionEvents: vi.fn(),
}));

// Mock Tauri global-shortcut plugin (used by useAppKeyboardShortcuts)
vi.mock("@tauri-apps/plugin-global-shortcut", () => ({
  register: vi.fn().mockResolvedValue(undefined),
  unregister: vi.fn().mockResolvedValue(undefined),
}));

// Mock hooks that make Tauri API calls
vi.mock("@/hooks/useRunningProcesses", () => ({
  useRunningProcesses: vi.fn().mockReturnValue({ data: undefined }),
}));

vi.mock("@/hooks/useMergePipeline", () => ({
  useMergePipeline: vi.fn().mockReturnValue({ data: undefined }),
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: vi.fn().mockReturnValue({
    settings: {
      providers: [],
      defaultProvider: "codex",
      requiresOnboarding: false,
    },
    providers: [],
    isLoading: false,
    isPlaceholderData: false,
  }),
}));

// Mock other required hooks
vi.mock("@/hooks/useReviews", () => ({
  reviewKeys: {
    all: ["reviews"],
    pending: () => ["reviews", "pending"],
  },
  usePendingReviews: vi.fn().mockReturnValue({
    data: [],
    isLoading: false,
  }),
  useReviewsByTaskId: vi.fn().mockReturnValue({
    data: [],
    isLoading: false,
  }),
  useTaskStateHistory: vi.fn().mockReturnValue({
    data: [],
    isLoading: false,
  }),
  useTasksAwaitingReview: vi.fn().mockReturnValue({
    allTasks: [],
    aiTasks: [],
    humanTasks: [],
    aiCount: 0,
    humanCount: 0,
    totalCount: 0,
    isLoading: false,
  }),
}));

vi.mock("@/hooks/useReviewMutations", () => ({
  useReviewMutations: vi.fn().mockReturnValue({
    isApproving: false,
    isRequestingChanges: false,
  }),
}));

vi.mock("@/hooks/useProjects", () => ({
  useProjects: vi.fn().mockReturnValue({
    data: [{ id: "demo-project-1", name: "Demo Project", workingDirectory: "/tmp/demo", createdAt: "2024-01-01T00:00:00Z", updatedAt: "2024-01-01T00:00:00Z" }],
    isLoading: false,
  }),
  projectKeys: {
    all: ["projects"],
    list: () => ["projects", "list"],
  },
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: vi.fn().mockReturnValue({
    confirm: vi.fn(),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

// useAppKeyboardShortcuts is NOT mocked — let the real hook run
// so keyboard shortcut tests work. @tauri-apps/plugin-global-shortcut
// is mocked above to prevent Tauri API calls.

vi.mock("@/hooks", () => ({
  useNavCompactBreakpoint: vi.fn().mockReturnValue({
    isNavCompact: false,
  }),
}));

// Mock useAskUserQuestion to prevent Tauri API calls
vi.mock("@/hooks/useAskUserQuestion", () => ({
  useAskUserQuestion: vi.fn().mockReturnValue({
    pendingQuestion: null,
    isLoading: false,
    respond: vi.fn(),
  }),
}));

// Reset stores before each test
function resetStores() {
  localStorage.clear();

  useUiStore.setState({
    sidebarOpen: true,
    notificationsPanelOpen: false,
    currentView: "agents",
    activeModal: null,
    modalContext: undefined,
    notifications: [],
    loading: {},
    confirmation: null,
    activeQuestions: {},
    answeredQuestions: {},
    executionStatus: {
      isPaused: false,
      runningCount: 0,
      maxConcurrent: 10,
      queuedCount: 0,
      canStartTask: true,
    },
    graphSelection: null,
    viewByProject: {},
  });

  useChatStore.setState({
    messages: {},
    context: {
      view: "kanban",
      projectId: "demo-project",
    },
    isLoading: false,
  });

  useIdeationStore.setState({
    sessions: {},
    activeSessionId: null,
    isLoading: false,
    error: null,
  });

  useProposalStore.setState({
    proposals: {},
    isLoading: false,
    error: null,
    lastProposalAddedAt: {},
    lastDependencyRefreshRequestedAt: {},
    lastProposalUpdatedAt: {},
    lastUpdatedProposalId: {},
  });

  useProjectStore.setState({
    activeProjectId: "demo-project-1",
    projects: { "demo-project-1": { id: "demo-project-1", name: "Demo Project", workingDirectory: "/tmp/demo", createdAt: "2024-01-01T00:00:00Z", updatedAt: "2024-01-01T00:00:00Z" } as never },
    isInitialized: true,
  });

  useAgentSessionStore.setState(useAgentSessionStore.getInitialState(), true);
  getQueryClient().clear();
}

function buildCreatedProject(overrides: Partial<Project> = {}): Project {
  return {
    id: "created-project",
    name: "Created Project",
    workingDirectory: "/tmp/created-project",
    gitMode: "worktree",
    baseBranch: "main",
    worktreeParentDirectory: null,
    useFeatureBranches: true,
    mergeValidationMode: "block",
    detectedAnalysis: null,
    customAnalysis: null,
    analyzedAt: null,
    githubPrEnabled: false,
    createdAt: "2026-06-15T12:00:00Z",
    updatedAt: "2026-06-15T12:00:00Z",
    ...overrides,
  };
}

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    preloadAutomationsViewMock.mockResolvedValue({ default: AutomationsViewMock });
    resolveTaskAgentWorkspaceMock.mockResolvedValue(null);
    resetStores();
    vi.mocked(useHarnessProviders).mockReturnValue({
      settings: {
        providers: [],
        defaultProvider: "codex",
        requiresOnboarding: false,
      },
      providers: [],
      isLoading: false,
      isPlaceholderData: false,
    } as never);

    // Setup default mock return values for execution hooks
    vi.mocked(useExecutionStatus).mockReturnValue({
      data: {
        isPaused: false,
        runningCount: 0,
        maxConcurrent: 2,
        queuedCount: 0,
        canStartTask: true,
      },
      isPaused: false,
      runningCount: 0,
      queuedCount: 0,
      maxConcurrent: 2,
      globalMaxConcurrent: 20,
      canStartTask: true,
      isLoading: false,
    });

    vi.mocked(useExecutionEvents).mockReturnValue(undefined);
  });

  it("should render without crashing", () => {
    render(<App />);
    expect(document.body).toBeDefined();
  });

  it("keeps bottom-left toasts pinned to the left gutter when lifted above the execution footer", () => {
    render(<App />);

    const toasterProps = sonnerToasterMock.mock.calls.at(-1)?.[0];
    expect(toasterProps).toEqual(
      expect.objectContaining({
        offset: {
          bottom: "92px",
          left: "16px",
        },
        position: "bottom-left",
      }),
    );
    expect(toasterProps?.style).toEqual(
      expect.objectContaining({
        zIndex: 40,
      }),
    );
  });

  it("should display the primary navigation shell", () => {
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
    expect(screen.getByTestId("nav-agents")).toHaveAttribute("aria-current", "page");
  });

  it("switches to a new Agents conversation for a project created from Agents", async () => {
    const createdProject = buildCreatedProject();
    const createProjectSpy = vi
      .spyOn(api.projects, "create")
      .mockResolvedValue(createdProject);

    useUiStore.setState({ currentView: "agents" });
    useAgentSessionStore.setState({
      focusedProjectId: "demo-project-1",
      selectedProjectId: "demo-project-1",
      selectedConversationId: "old-conversation",
    });

    render(<App />);

    fireEvent.click(await screen.findByTestId("agents-create-project"));
    fireEvent.click(screen.getByTestId("project-wizard-create"));

    await waitFor(() => expect(createProjectSpy).toHaveBeenCalledTimes(1));

    expect(useProjectStore.getState().activeProjectId).toBe("created-project");
    expect(useUiStore.getState().currentView).toBe("agents");
    expect(useAgentSessionStore.getState()).toEqual(
      expect.objectContaining({
        focusedProjectId: "created-project",
        selectedProjectId: null,
        selectedConversationId: null,
      })
    );
  });

  it("keeps narrowed filters visible when creating a project from Agents", async () => {
    const createdProject = buildCreatedProject({
      id: "created-filter-project",
      name: "Filtered Project",
    });
    const createProjectSpy = vi
      .spyOn(api.projects, "create")
      .mockResolvedValue(createdProject);

    useUiStore.getState().setCurrentView("agents");
    useUiStore.setState({ graphSelection: { kind: "task", id: "task-1" } });
    useAgentSessionStore.setState({
      focusedProjectId: "demo-project-1",
      selectedProjectId: "demo-project-1",
      selectedConversationId: "old-conversation",
      showAllProjects: false,
      sidebarProjectFilterIds: ["demo-project-1"],
    });

    render(<App />);

    fireEvent.click(await screen.findByTestId("agents-create-project"));
    fireEvent.click(screen.getByTestId("project-wizard-create"));

    await waitFor(() => expect(createProjectSpy).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(useUiStore.getState()).toEqual(
        expect.objectContaining({
          currentView: "agents",
          graphSelection: null,
        })
      )
    );

    expect(useProjectStore.getState().activeProjectId).toBe("created-filter-project");
    expect(useUiStore.getState().viewByProject["demo-project-1"]).toBe("agents");
    expect(useAgentSessionStore.getState()).toEqual(
      expect.objectContaining({
        focusedProjectId: "created-filter-project",
        selectedProjectId: null,
        selectedConversationId: null,
        showAllProjects: false,
        sidebarProjectFilterIds: ["demo-project-1", "created-filter-project"],
      })
    );
  });

  it("does not show Atlassian awareness on a normal app load", async () => {
    render(<App />);

    await waitFor(() =>
      expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument()
    );
    expect(toast.info).not.toHaveBeenCalled();
  });

  it("does not derive app-shell readiness from the post-update marker", () => {
    markPostUpdatePreparing("0.12.3");

    render(<App />);

    expect(screen.queryByTestId("startup-screen")).not.toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
  });

  it("renders the v27 mini rail logo and flat active highlight", () => {
    render(<App />);

    const brandSvg = screen.getByTestId("left-nav-brand").querySelector("svg");
    expect(brandSvg).toHaveClass("h-[44px]", "w-[44px]");

    const activeButton = screen.getByTestId("nav-agents");
    expect(activeButton.className).toContain("h-[44px] w-[44px]");
    expect(activeButton.className).not.toContain("focus-visible:ring");
    expect(activeButton.className).toContain(
      "focus-visible:[outline:2px_solid_var(--border-focus)]"
    );
    expect(activeButton.className).toContain("bg-[var(--bg-hover)]");
    expect(activeButton.className).toContain("text-[var(--nav-rail-active-color)]");
    expect(activeButton).toHaveStyle({
      boxShadow: "var(--nav-rail-active-shadow)",
    });
    expect(activeButton.querySelector(".left-nav-rail__active-border")).toBeInTheDocument();

    expect(screen.getByTestId("nav-automations").className).toContain(
      "text-[var(--nav-rail-inactive-color)]"
    );
  });

  it("should display theme selector", () => {
    render(<App />);
    expect(screen.getByTestId("theme-selector")).toBeInTheDocument();
  });

  it("keeps only one topbar dropdown open at a time", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByTestId("theme-selector-trigger"));
    expect(screen.getByTestId("theme-option-dark")).toBeInTheDocument();

    await user.click(screen.getByTestId("font-scale-selector-trigger"));
    expect(screen.queryByTestId("theme-option-dark")).not.toBeInTheDocument();
    expect(screen.getByTestId("font-scale-option-default")).toBeInTheDocument();

    await user.click(screen.getByTestId("theme-selector-trigger"));
    expect(screen.queryByTestId("font-scale-option-default")).not.toBeInTheDocument();
    expect(screen.getByTestId("theme-option-dark")).toBeInTheDocument();
  });

  it("should have main element with flex layout", () => {
    render(<App />);
    const mainElement = screen.getByRole("main");
    // h-screen for fixed header layout (header is fixed, needs explicit height)
    expect(mainElement).toHaveClass("h-screen", "flex", "flex-col");
  });

  it("selecting a font-scale option closes the dropdown and updates the trigger label", async () => {
    const user = userEvent.setup();
    const { useThemeStore } = await import("@/stores/themeStore");
    const initial = useThemeStore.getState().fontScale;
    try {
      render(<App />);

      await user.click(screen.getByTestId("font-scale-selector-trigger"));
      await user.click(screen.getByTestId("font-scale-option-lg"));
      // Dropdown closed.
      expect(screen.queryByTestId("font-scale-option-lg")).not.toBeInTheDocument();
      // Trigger label reflects the new option (110%).
      expect(screen.getByTestId("font-scale-selector").textContent).toMatch(/110%/);
    } finally {
      // Reset persisted fontScale so order-dependent v27 chrome tests still see 100%.
      useThemeStore.getState().setFontScale(initial);
    }
  });

  it("should render the v27 top navigation chrome", () => {
    render(<App />);
    const header = screen.getByRole("banner");
    expect(header).toBeInTheDocument();
    expect(header).toHaveAttribute("data-testid", "app-header");
    expect(header.getAttribute("style")).toContain(
      "background-color: var(--app-navbar-bg)"
    );
    expect(header.getAttribute("style")).toContain("border-bottom-color: var(--app-navbar-border)");
    expect(screen.getByTestId("left-nav-rail").getAttribute("style")).toContain(
      "background-color: var(--app-rail-bg)"
    );
    expect(screen.getByTestId("left-nav-rail").getAttribute("style")).toContain(
      "border-right-color: var(--app-rail-border)"
    );
    expect(screen.queryByTestId("window-traffic-lights")).not.toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent(
      "Workspace/Agents/New run"
    );
    const commandSearch = screen.getByTestId("topbar-command-search");
    expect(commandSearch).toBeInTheDocument();
    expect(commandSearch).toHaveClass("h-8", "w-[320px]", "rounded-[6px]");
    expect(commandSearch.getAttribute("style")).toContain("background-color: var(--bg-elevated)");
    expect(commandSearch.getAttribute("style")).toContain("border-color: var(--border-default)");
    expect(screen.getByTestId("font-scale-selector")).toHaveTextContent("100%");
    expect(screen.queryByTestId("project-selector-mock")).not.toBeInTheDocument();
  });

  it.skip.each(["agents", "activity", "automations"] as const)(
    "shows the project selector in the %s topbar",
    (view) => {
      useUiStore.getState().setCurrentView(view);

      render(<App />);

      expect(screen.getByTestId("project-selector-mock")).toBeInTheDocument();
    }
  );

  it("shows the selected agent conversation in the Agents breadcrumb", () => {
    const queryClient = getQueryClient();
    const getMessagesPage = vi.spyOn(chatApi, "getConversationMessagesPage");
    useAgentSessionStore.setState({
      selectedConversationId: "conversation-breadcrumb",
    });
    queryClient.setQueryData(chatKeys.conversationHistory("conversation-breadcrumb"), {
      pages: [
        {
          conversation: {
            id: "conversation-breadcrumb",
            contextType: "project",
            contextId: "demo-project-1",
            providerSessionId: null,
            providerHarness: null,
            title: "List worktree directory contents",
            messageCount: 0,
            lastMessageAt: null,
            createdAt: "2026-05-07T00:00:00Z",
            updatedAt: "2026-05-07T00:00:00Z",
            archivedAt: null,
          },
          messages: [],
          limit: 1,
          offset: 0,
          totalMessageCount: 0,
          hasOlder: false,
        },
      ],
      pageParams: [0],
    });

    render(<App />);

    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent(
      "Workspace/Agents/List worktree directory contents"
    );
    expect(getMessagesPage).not.toHaveBeenCalled();
  });

  it("uses the project agent conversation list cache for the Agents breadcrumb", () => {
    const queryClient = getQueryClient();
    const getMessagesPage = vi.spyOn(chatApi, "getConversationMessagesPage");
    const conversation = {
      id: "conversation-project-list-cache",
      contextType: "project",
      contextId: "demo-project-1",
      providerSessionId: null,
      providerHarness: null,
      title: "Hydrated from project list",
      messageCount: 0,
      lastMessageAt: null,
      createdAt: "2026-05-07T00:00:00Z",
      updatedAt: "2026-05-07T00:00:00Z",
      archivedAt: null,
    } as const;

    useAgentSessionStore.setState({
      selectedConversationId: conversation.id,
    });
    queryClient.setQueryData(agentConversationKeys.projectList("demo-project-1", false, ""), {
      pages: [
        {
          conversations: [conversation],
          total: 1,
          offset: 0,
          hasMore: false,
        },
      ],
      pageParams: [0],
    });

    render(<App />);

    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent(
      "Workspace/Agents/Hydrated from project list"
    );
    expect(getMessagesPage).not.toHaveBeenCalled();
  });

  it("falls back to a cheap conversation summary lookup when agent caches miss", async () => {
    const queryClient = getQueryClient();
    const getMessagesPage = vi.spyOn(chatApi, "getConversationMessagesPage");
    const getConversationSummary = vi.spyOn(chatApi, "getConversationSummary").mockResolvedValue({
      id: "conversation-cache-miss",
      contextType: "project",
      contextId: "demo-project-1",
      providerSessionId: null,
      providerHarness: null,
      title: "Fetched after cache miss",
      messageCount: 42,
      lastMessageAt: "2026-05-07T00:02:00Z",
      createdAt: "2026-05-07T00:00:00Z",
      updatedAt: "2026-05-07T00:02:00Z",
      archivedAt: null,
    });

    useAgentSessionStore.setState({
      selectedConversationId: "conversation-cache-miss",
    });
    queryClient.setQueryData(agentConversationKeys.projectList("demo-project-1", false, ""), {
      pages: [
        {
          conversations: [
            {
              id: "other-conversation",
              contextType: "project",
              contextId: "demo-project-1",
              providerSessionId: null,
              providerHarness: null,
              title: "Other conversation",
              messageCount: 0,
              lastMessageAt: null,
              createdAt: "2026-05-07T00:00:00Z",
              updatedAt: "2026-05-07T00:00:00Z",
              archivedAt: null,
            },
          ],
          total: 1,
          offset: 0,
          hasMore: false,
        },
      ],
      pageParams: [0],
    });

    render(<App />);

    await waitFor(() =>
      expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent(
        "Workspace/Agents/Fetched after cache miss"
      )
    );
    expect(getConversationSummary).toHaveBeenCalledWith("conversation-cache-miss");
    expect(getMessagesPage).not.toHaveBeenCalled();
    expect(
      queryClient.getQueryData(chatKeys.conversationHistory("conversation-cache-miss"))
    ).toBeUndefined();
    expect(
      queryClient.getQueryData(chatKeys.conversationSummary("conversation-cache-miss"))
    ).toMatchObject({
      title: "Fetched after cache miss",
    });
  });

  it("renames the selected agent conversation from the Agents breadcrumb", async () => {
    const user = userEvent.setup();
    const queryClient = getQueryClient();
    const conversation = {
      id: "conversation-breadcrumb-rename",
      contextType: "project",
      contextId: "demo-project-1",
      providerSessionId: null,
      providerHarness: null,
      title: "List worktree directory contents",
      messageCount: 0,
      lastMessageAt: null,
      createdAt: "2026-05-07T00:00:00Z",
      updatedAt: "2026-05-07T00:00:00Z",
      archivedAt: null,
    } as const;
    const updateTitle = vi.spyOn(chatApi, "updateConversationTitle").mockResolvedValue({
      ...conversation,
      title: "Renamed agent run",
      updatedAt: "2026-05-07T00:01:00Z",
    });

    useAgentSessionStore.setState({
      selectedConversationId: conversation.id,
    });
    queryClient.setQueryData(chatKeys.conversationSummary(conversation.id), conversation);
    queryClient.setQueryData(chatKeys.conversationHistory(conversation.id), {
      pages: [
        {
          conversation,
          messages: [],
          limit: 1,
          offset: 0,
          totalMessageCount: 0,
          hasOlder: false,
        },
      ],
      pageParams: [0],
    });

    render(<App />);

    expect(screen.getByTestId("agent-breadcrumb-title")).toHaveAttribute(
      "data-theme-button-skip",
      "true"
    );
    await user.click(screen.getByRole("button", { name: "Rename agent conversation" }));
    const input = screen.getByRole("textbox", { name: "Rename agent conversation" });
    expect(input).toHaveClass("border-transparent");
    await user.clear(input);
    await user.type(input, "Renamed agent run{enter}");

    await waitFor(() =>
      expect(updateTitle).toHaveBeenCalledWith(conversation.id, "Renamed agent run")
    );
    expect(updateTitle).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent(
      "Workspace/Agents/Renamed agent run"
    );
    expect(queryClient.getQueryData(chatKeys.conversationSummary(conversation.id))).toMatchObject({
      title: "Renamed agent run",
    });
  });

  it("renames ideation-backed agent conversations from the Agents breadcrumb", async () => {
    const user = userEvent.setup();
    const queryClient = getQueryClient();
    const conversation = {
      id: "conversation-breadcrumb-ideation-rename",
      contextType: "ideation",
      contextId: "ideation-session-1",
      providerSessionId: null,
      providerHarness: null,
      title: "Review proposal set",
      messageCount: 0,
      lastMessageAt: null,
      createdAt: "2026-05-07T00:00:00Z",
      updatedAt: "2026-05-07T00:00:00Z",
      archivedAt: null,
    } as const;
    const updateConversationTitle = vi
      .spyOn(chatApi, "updateConversationTitle")
      .mockResolvedValue({
        ...conversation,
        title: "Renamed ideation run",
        updatedAt: "2026-05-07T00:01:00Z",
      });
    const updateSessionTitle = vi
      .spyOn(ideationApi.sessions, "updateTitle")
      .mockResolvedValue({ id: "ideation-session-1", title: "Renamed ideation run" } as never);

    useAgentSessionStore.setState({
      selectedConversationId: conversation.id,
    });
    queryClient.setQueryData(chatKeys.conversationHistory(conversation.id), {
      pages: [
        {
          conversation,
          messages: [],
          limit: 1,
          offset: 0,
          totalMessageCount: 0,
          hasOlder: false,
        },
      ],
      pageParams: [0],
    });

    render(<App />);

    await user.click(screen.getByRole("button", { name: "Rename agent conversation" }));
    const input = screen.getByRole("textbox", { name: "Rename agent conversation" });
    await user.clear(input);
    await user.type(input, "Renamed ideation run{enter}");

    await waitFor(() =>
      expect(updateConversationTitle).toHaveBeenCalledWith(
        conversation.id,
        "Renamed ideation run"
      )
    );
    expect(updateSessionTitle).toHaveBeenCalledWith(
      "ideation-session-1",
      "Renamed ideation run"
    );
  });

  it("restores the breadcrumb title and shows an error when breadcrumb rename fails", async () => {
    const user = userEvent.setup();
    const queryClient = getQueryClient();
    const conversation = {
      id: "conversation-breadcrumb-rename-failure",
      contextType: "project",
      contextId: "demo-project-1",
      providerSessionId: null,
      providerHarness: null,
      title: "Original title",
      messageCount: 0,
      lastMessageAt: null,
      createdAt: "2026-05-07T00:00:00Z",
      updatedAt: "2026-05-07T00:00:00Z",
      archivedAt: null,
    } as const;
    vi.spyOn(chatApi, "updateConversationTitle").mockRejectedValue(
      new Error("Rename failed")
    );

    useAgentSessionStore.setState({
      selectedConversationId: conversation.id,
    });
    queryClient.setQueryData(chatKeys.conversation(conversation.id), {
      conversation,
      messages: [],
    });
    queryClient.setQueryData(chatKeys.conversationHistory(conversation.id), {
      pages: [
        {
          conversation,
          messages: [],
          limit: 1,
          offset: 0,
          totalMessageCount: 0,
          hasOlder: false,
        },
      ],
      pageParams: [0],
    });
    queryClient.setQueryData(
      chatKeys.conversationList(conversation.contextType, conversation.contextId),
      [conversation]
    );

    render(<App />);

    await user.click(screen.getByRole("button", { name: "Rename agent conversation" }));
    const input = screen.getByRole("textbox", { name: "Rename agent conversation" });
    await user.clear(input);
    await user.type(input, "Broken title{enter}");

    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Rename failed")
    );
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent(
      "Workspace/Agents/Original title"
    );
  });

  it("cancels breadcrumb rename on Escape without saving", async () => {
    const user = userEvent.setup();
    const queryClient = getQueryClient();
    const conversation = {
      id: "conversation-breadcrumb-rename-cancel",
      contextType: "project",
      contextId: "demo-project-1",
      providerSessionId: null,
      providerHarness: null,
      title: "Original breadcrumb title",
      messageCount: 0,
      lastMessageAt: null,
      createdAt: "2026-05-07T00:00:00Z",
      updatedAt: "2026-05-07T00:00:00Z",
      archivedAt: null,
    } as const;
    const updateTitle = vi.spyOn(chatApi, "updateConversationTitle");

    useAgentSessionStore.setState({
      selectedConversationId: conversation.id,
    });
    queryClient.setQueryData(chatKeys.conversationHistory(conversation.id), {
      pages: [
        {
          conversation,
          messages: [],
          limit: 1,
          offset: 0,
          totalMessageCount: 0,
          hasOlder: false,
        },
      ],
      pageParams: [0],
    });

    render(<App />);

    await user.click(screen.getByRole("button", { name: "Rename agent conversation" }));
    const input = screen.getByRole("textbox", { name: "Rename agent conversation" });
    await user.clear(input);
    await user.type(input, "Should not save{escape}");

    expect(updateTitle).not.toHaveBeenCalled();
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent(
      "Workspace/Agents/Original breadcrumb title"
    );
  });

  it("uses the v27 panel surface for the notification center", () => {
    useUiStore.setState({ notificationsPanelOpen: true });

    render(<App />);

    const shell = screen.getByTestId("notifications-panel-shell");
    expect(shell.getAttribute("style")).toContain("width: 100vw");
    expect(shell.getAttribute("style")).toContain("max-width: 400px");
    expect(shell.getAttribute("style")).toContain("bottom: 0px");
    expect(shell.className).not.toContain("w-[400px]");
    expect(shell.className).not.toContain("invisible");
    expect(shell.getAttribute("style")).toContain(
      "background-color: var(--bg-surface)"
    );
    expect(shell.getAttribute("style")).toContain(
      "border-left-color: var(--border-subtle)"
    );
    expect(screen.getByTestId("notifications-panel-frame").getAttribute("style")).toContain(
      "background-color: var(--bg-surface)"
    );
    expect(screen.getByTestId("notifications-panel-frame").getAttribute("style")).toContain(
      "box-shadow: none"
    );
  });

  it("opens and closes the notification center from the top-bar trigger", async () => {
    const user = userEvent.setup();
    render(<App />);

    const toggle = screen.getByTestId("reviews-toggle");
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByTestId("notifications-panel-shell")).not.toBeInTheDocument();

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    const shell = screen.getByTestId("notifications-panel-shell");
    expect(shell).toHaveAttribute(
      "aria-hidden",
      "false",
    );
    expect(shell.className).not.toContain("invisible");

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    expect(shell).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    expect(shell.className).toContain("invisible");
  });

  it("closes the notification center from the outside dismissal layer", async () => {
    const user = userEvent.setup();
    useUiStore.setState({ notificationsPanelOpen: true });

    render(<App />);

    const toggle = screen.getByTestId("reviews-toggle");
    const shell = screen.getByTestId("notifications-panel-shell");
    await user.click(screen.getByTestId("notifications-panel"));
    expect(shell).toHaveAttribute("aria-hidden", "false");

    await user.click(screen.getByTestId("notifications-panel-backdrop"));

    await waitFor(() => expect(shell).toHaveAttribute("aria-hidden", "true"));
    expect(screen.queryByTestId("notifications-panel-backdrop")).not.toBeInTheDocument();
    await waitFor(() => expect(toggle).toHaveFocus());
  });

  it("returns focus to the notification trigger after close button and Escape closes", async () => {
    const user = userEvent.setup();
    render(<App />);

    const toggle = screen.getByTestId("reviews-toggle");
    await user.click(toggle);
    await user.click(screen.getByTestId("notifications-panel-close"));
    await waitFor(() => expect(toggle).toHaveFocus());
    expect(toggle).toHaveAttribute("aria-pressed", "false");

    await user.click(toggle);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(toggle).toHaveFocus());
    expect(toggle).toHaveAttribute("aria-pressed", "false");
  });

  it("keeps the notification badge global when a different project is active", () => {
    getQueryClient().setQueryData(attentionKeys.list(), [{
      id: "task:project-2:failed",
      category: "task_failed",
      title: "Task failed in another project",
      projectId: "project-2",
      target: { kind: "task", taskId: "task-2" },
    }]);

    render(<App />);

    expect(screen.getByTestId("reviews-badge")).toHaveTextContent("1");
  });

  it("should render Agents view by default", async () => {
    render(<App />);
    expect(await screen.findByTestId("agents-view-mock")).toBeInTheDocument();
  });

  it("should provide QueryClient context", () => {
    // This test verifies that QueryClientProvider is working
    // If App renders successfully with QueryClientProvider, queries should work
    render(<App />);
    expect(document.body).toBeDefined();
  });

  describe("Ticketing view", () => {
    beforeEach(() => {
      ticketingViewProps.current = null;
      getQueryClient().clear();
    });

    it("renders the ticketing dashboard without a manual feature override", async () => {
      render(<App />);

      useUiStore.getState().setCurrentView("ticketing");

      expect(
        await screen.findByTestId("ticketing-dashboard-view-mock"),
      ).toBeInTheDocument();
      expect(
        screen.getByTestId("ticketing-dashboard-view-mock"),
      ).toHaveAttribute("data-project-id", "demo-project-1");
    });

    it("does not show a disabled placeholder for the old dashboard flag", async () => {
      render(<App />);

      useUiStore.getState().setCurrentView("ticketing");

      expect(
        await screen.findByTestId("ticketing-dashboard-view-mock"),
      ).toBeInTheDocument();
      expect(screen.queryByTestId("feature-disabled-ticketing")).not.toBeInTheDocument();
    });

    it("shows Agents without stale task selection when a legacy task has no owner", async () => {
      render(<App />);

      useUiStore.getState().setCurrentView("ticketing");
      await screen.findByTestId("ticketing-dashboard-view-mock");

      expect(ticketingViewProps.current).not.toBeNull();
      ticketingViewProps.current?.onNavigateToAssociation({
        view: "kanban",
        id: "task-99",
      });

      await waitFor(() => {
        expect(useUiStore.getState().currentView).toBe("agents");
      });
      expect(useAgentSessionStore.getState().selectedConversationId).toBeNull();
      expect(useAgentSessionStore.getState().focusedProjectId).toBeNull();
      expect(useAgentSessionStore.getState().artifactByConversationId).toEqual({});
      expect(useAgentSessionStore.getState().taskArtifactFocusRequestByConversationId).toEqual({});
      expect(useChatStore.getState().activeConversationIds).toEqual({});
    });

    it("opens the linked Agent Tasks graph and selects the legacy task", async () => {
      resolveTaskAgentWorkspaceMock.mockResolvedValue({
        conversationId: "conversation-99",
        projectId: "demo-project-1",
        title: "Task owner",
      });
      render(<App />);

      useUiStore.getState().setCurrentView("ticketing");
      await screen.findByTestId("ticketing-dashboard-view-mock");

      ticketingViewProps.current?.onNavigateToAssociation({
        view: "graph",
        id: "task-99",
        projectId: "demo-project-1",
        conversationId: "conversation-99",
      });

      await waitFor(() => {
        expect(resolveTaskAgentWorkspaceMock).toHaveBeenCalledWith("task-99");
        expect(useUiStore.getState().currentView).toBe("agents");
        expect(
          useAgentSessionStore.getState().artifactByConversationId["conversation-99"],
        ).toMatchObject({ activeTab: "tasks", isOpen: true, taskMode: "graph" });
        expect(
          useAgentSessionStore.getState().taskArtifactFocusRequestByConversationId[
            "conversation-99"
          ],
        ).toMatchObject({ taskId: "task-99" });
      });
    });

    it("resolves a historical task link to its Agent Tasks graph", async () => {
      resolveTaskAgentWorkspaceMock.mockResolvedValue({
        conversationId: "conversation-99",
        projectId: "demo-project-1",
        title: "Task owner",
      });
      render(<App />);

      useUiStore.getState().setCurrentView("ticketing");
      await screen.findByTestId("ticketing-dashboard-view-mock");

      ticketingViewProps.current?.onNavigateToAssociation({
        view: "graph",
        id: "task-99",
      });

      await waitFor(() => {
        expect(resolveTaskAgentWorkspaceMock).toHaveBeenCalledWith("task-99");
      });
      expect(
        useAgentSessionStore.getState().artifactByConversationId["conversation-99"],
      ).toMatchObject({ activeTab: "tasks", isOpen: true, taskMode: "graph" });
      expect(
        useAgentSessionStore.getState().taskArtifactFocusRequestByConversationId[
          "conversation-99"
        ],
      ).toMatchObject({ taskId: "task-99" });
    });

    it("routes an agents deep link by focusing the project and selecting the conversation", async () => {
      render(<App />);

      useUiStore.getState().setCurrentView("ticketing");
      await screen.findByTestId("ticketing-dashboard-view-mock");

      ticketingViewProps.current?.onNavigateToAssociation({
        view: "agents",
        id: "conversation-77",
        projectId: "project-x",
      });

      await waitFor(() => {
        expect(useUiStore.getState().currentView).toBe("agents");
      });
      expect(useAgentSessionStore.getState().focusedProjectId).toBe("project-x");
      expect(
        useChatStore.getState().activeConversationIds["project:project-x"],
      ).toBe("conversation-77");
    });

    it("shows Agents when a legacy ideation link has no owner", async () => {
      render(<App />);

      useUiStore.getState().setCurrentView("ticketing");
      await screen.findByTestId("ticketing-dashboard-view-mock");

      ticketingViewProps.current?.onNavigateToAssociation({
        view: "ideation",
        id: "irrelevant",
      });

      await waitFor(() => {
        expect(useUiStore.getState().currentView).toBe("agents");
      });
    });
  });

  describe("Welcome screen navbar collapse", () => {
    it("hides view tabs, project selector, and reviews toggle when no projects exist", async () => {
      const { useProjects } = await import("@/hooks/useProjects");
      vi.mocked(useProjects).mockReturnValueOnce({
        data: [],
        isLoading: false,
      } as never);
      useProjectStore.setState({
        activeProjectId: null,
        projects: {},
        isInitialized: true,
      });

      render(<App />);

      // Welcome screen renders
      expect(screen.getByTestId("welcome-screen")).toBeInTheDocument();

      // The project-scoped Agents tab is gone.
      expect(screen.queryByTestId("nav-agents")).toBeNull();

      // v27 topbar controls stay mounted even when the project-scoped rail collapses.
      expect(screen.getByTestId("reviews-toggle")).toBeInTheDocument();
      expect(screen.queryByTestId("project-selector-mock")).not.toBeInTheDocument();

      // Settings button still rendered (only thing left in Navigation)
      expect(screen.getByTestId("nav-settings")).toBeInTheDocument();
    });

    it("renders the full navbar when projects exist", () => {
      // Default beforeEach already sets up demo-project-1, no welcome state
      render(<App />);

      expect(screen.queryByTestId("welcome-screen")).toBeNull();
      expect(screen.getByTestId("nav-agents")).toBeInTheDocument();
      expect(screen.getByTestId("nav-settings")).toBeInTheDocument();
      expect(screen.getByTestId("reviews-toggle")).toBeInTheDocument();
    });

    it("routes existing users without a default provider to provider setup", async () => {
      const user = userEvent.setup();
      vi.mocked(useHarnessProviders).mockReturnValue({
        settings: {
          providers: [],
          defaultProvider: null,
          requiresOnboarding: true,
        },
        providers: [],
        isLoading: false,
        isPlaceholderData: false,
      } as never);

      render(<App />);

      expect(screen.getByTestId("welcome-screen")).toBeInTheDocument();
      expect(screen.getByTestId("welcome-provider-step")).toHaveAttribute(
        "data-current",
        "true",
      );
      expect(screen.getByTestId("welcome-project-step")).toHaveAttribute(
        "data-status",
        "complete",
      );
      expect(screen.queryByTestId("nav-agents")).toBeNull();

      await user.click(screen.getByRole("button", { name: /set up provider/i }));

      expect(useUiStore.getState().activeModal).toBe("settings");
      expect(useUiStore.getState().modalContext).toEqual({ section: "providers" });
    });

    it("does not show provider onboarding while provider settings are placeholder data", () => {
      vi.mocked(useHarnessProviders).mockReturnValue({
        settings: {
          providers: [],
          defaultProvider: null,
          requiresOnboarding: true,
        },
        providers: [],
        isLoading: false,
        isPlaceholderData: true,
      } as never);

      render(<App />);

      expect(screen.queryByTestId("welcome-screen")).toBeNull();
      expect(screen.getByTestId("nav-agents")).toBeInTheDocument();
    });
  });

  describe("Execution Status Query Scoping", () => {
    it("should call useExecutionStatus with undefined when no active project", () => {
      // This test verifies Phase 82 requirement: execution status queries are scoped to active project
      // When no project is set, activeProjectId is null, so currentProjectId = ""
      // and "" || undefined = undefined
      useProjectStore.setState({ activeProjectId: null, projects: {}, isInitialized: false });
      render(<App />);

      // useExecutionStatus should be called with undefined (no active project)
      expect(vi.mocked(useExecutionStatus)).toHaveBeenCalledWith(
        undefined,
        expect.objectContaining({ enabled: false, refetchInterval: false })
      );
    });

    it("should call useExecutionStatus with project ID when project is active", () => {
      // Set up an active project BEFORE rendering
      useProjectStore.setState({ activeProjectId: "test-project-123" });

      render(<App />);

      // When a project is active, useExecutionStatus should be called with that project ID
      expect(vi.mocked(useExecutionStatus)).toHaveBeenCalledWith(
        "test-project-123",
        expect.objectContaining({
          enabled: true,
          refetchInterval: 30000,
          refetchOnWindowFocus: true,
          staleTime: 30_000,
        })
      );
    });

    it("hydrates footer-only execution data on the Agents view", () => {
      useProjectStore.setState({ activeProjectId: "test-project-123" });

      render(<App />);

      expect(vi.mocked(useRunningProcesses)).toHaveBeenCalledWith(
        "test-project-123",
        expect.objectContaining({ enabled: true })
      );
      expect(vi.mocked(useMergePipeline)).toHaveBeenCalledWith(
        "test-project-123",
        expect.objectContaining({ enabled: true })
      );
    });

    it("scopes Agents footer execution data to the selected agent conversation project", () => {
      useProjectStore.setState({ activeProjectId: "active-project" });
      useAgentSessionStore.setState({
        selectedConversationId: "conversation-2",
        selectedProjectId: "agent-project-2",
        focusedProjectId: "agent-project-2",
      });

      render(<App />);

      expect(vi.mocked(useExecutionEvents)).toHaveBeenCalledWith("agent-project-2");
      expect(vi.mocked(useExecutionStatus)).toHaveBeenCalledWith(
        "agent-project-2",
        expect.objectContaining({ enabled: true })
      );
      expect(vi.mocked(useRunningProcesses)).toHaveBeenCalledWith(
        "agent-project-2",
        expect.objectContaining({ enabled: true })
      );
      expect(vi.mocked(useMergePipeline)).toHaveBeenCalledWith(
        "agent-project-2",
        expect.objectContaining({ enabled: true })
      );
    });

    it("hydrates footer-only execution data when the Agents footer is visible", () => {
      useProjectStore.setState({ activeProjectId: "test-project-123" });
      useUiStore.setState({ currentView: "agents" });

      render(<App />);

      expect(vi.mocked(useRunningProcesses)).toHaveBeenCalledWith(
        "test-project-123",
        expect.objectContaining({ enabled: true })
      );
      expect(vi.mocked(useMergePipeline)).toHaveBeenCalledWith(
        "test-project-123",
        expect.objectContaining({ enabled: true })
      );
      expect(vi.mocked(useExecutionStatus)).toHaveBeenCalledWith(
        "test-project-123",
        expect.objectContaining({
          enabled: true,
          refetchInterval: 30000,
          refetchOnWindowFocus: true,
        })
      );
    });

  });
});
