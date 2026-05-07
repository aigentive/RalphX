import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { useUiStore } from "@/stores/uiStore";
import { useChatStore } from "@/stores/chatStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { useProposalStore } from "@/stores/proposalStore";
import { useProjectStore } from "@/stores/projectStore";
import { useExecutionStatus } from "@/hooks/useExecutionControl";
import { useExecutionEvents } from "@/hooks/useExecutionEvents";

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

// Mock TaskBoard to avoid Tauri API calls during tests
vi.mock("@/components/tasks/TaskBoard", () => ({
  TaskBoard: () => <div data-testid="task-board-mock">Task Board</div>,
}));

// Mock TaskGraphView to avoid ReactFlow setup during app-shell tests
vi.mock("@/components/TaskGraph", () => ({
  TaskGraphView: ({ footer }: { footer?: React.ReactNode }) => (
    <div data-testid="task-graph-view-mock">
      Task Graph
      {footer && <div data-testid="graph-footer-mock">{footer}</div>}
    </div>
  ),
}));

// Mock IdeationView to avoid complex ideation state issues
vi.mock("@/components/Ideation", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/components/Ideation")>();
  return {
    ...actual,
    IdeationView: ({ footer }: { footer?: React.ReactNode }) => (
      <div data-testid="ideation-view-mock">
        Ideation View
        {footer && <div data-testid="ideation-footer-mock">{footer}</div>}
      </div>
    ),
  };
});

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

// Mock AgentsView
vi.mock("@/components/agents", () => ({
  AgentsView: () => <div data-testid="agents-view-mock">Agents View</div>,
}));

// Mock UpdateChecker to avoid delayed non-critical updater checks during shell tests.
vi.mock("@/components/UpdateChecker", () => ({
  UpdateChecker: () => null,
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
  ProjectCreationWizard: () => null,
}));

// Mock ideation hooks
vi.mock("@/hooks/useIdeation", () => ({
  useIdeationSession: vi.fn().mockReturnValue({
    data: null,
    isLoading: false,
  }),
  useIdeationSessions: vi.fn().mockReturnValue({
    data: [],
    isLoading: false,
  }),
  useCreateIdeationSession: vi.fn().mockReturnValue({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
  useArchiveIdeationSession: vi.fn().mockReturnValue({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
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
  useUiStore.setState({
    sidebarOpen: true,
    reviewsPanelOpen: false,
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
    selectedTaskId: null,
    graphSelection: null,
    viewByProject: {},
    sessionByProject: {},
    selectedTaskByProject: {},
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
}

describe("App", () => {
  beforeEach(() => {
    resetStores();

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

  it("should display the primary navigation shell", () => {
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
    expect(screen.getByTestId("nav-agents")).toHaveAttribute("aria-current", "page");
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
    expect(activeButton).toHaveStyle({
      backgroundColor: "var(--bg-hover)",
      color: "var(--nav-rail-active-color)",
      boxShadow: "var(--nav-rail-active-shadow)",
    });
    expect(activeButton.querySelector(".left-nav-rail__active-border")).toBeInTheDocument();

    expect(screen.getByTestId("nav-kanban")).toHaveStyle({
      color: "var(--nav-rail-inactive-color)",
    });
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

  it("uses the v27 panel surface for the right reviews sidebar", () => {
    useUiStore.setState({ reviewsPanelOpen: true });

    render(<App />);

    expect(screen.getByTestId("reviews-panel-shell").getAttribute("style")).toContain(
      "background-color: var(--app-sidebar-bg)"
    );
    expect(screen.getByTestId("reviews-panel-shell").getAttribute("style")).toContain(
      "border-left-color: var(--app-sidebar-border)"
    );
    expect(screen.getByTestId("reviews-panel-frame").getAttribute("style")).toContain(
      "background-color: var(--app-sidebar-bg)"
    );
    expect(screen.getByTestId("reviews-panel-frame").getAttribute("style")).toContain(
      "box-shadow: none"
    );
  });

  it("should render Agents view by default", () => {
    render(<App />);
    expect(screen.getByTestId("agents-view-mock")).toBeInTheDocument();
  });

  it("should provide QueryClient context", () => {
    // This test verifies that QueryClientProvider is working
    // If App renders successfully with QueryClientProvider, queries should work
    render(<App />);
    expect(document.body).toBeDefined();
  });

  describe("View Navigation", () => {
    it("should render all navigation buttons", () => {
      render(<App />);
      expect(screen.getByTestId("nav-agents")).toBeInTheDocument();
      expect(screen.getByTestId("nav-kanban")).toBeInTheDocument();
      expect(screen.getByTestId("nav-graph")).toBeInTheDocument();
      expect(screen.getByTestId("nav-ideation")).toBeInTheDocument();
      expect(screen.getByTestId("nav-extensibility")).toBeInTheDocument();
      expect(screen.getByTestId("nav-activity")).toBeInTheDocument();
      expect(screen.getByTestId("nav-settings")).toBeInTheDocument();
    });

    it("should have navigation buttons rendered as accessible elements", () => {
      render(<App />);
      // All nav buttons should exist and have proper accessible labels
      // (Using shadcn Tooltip which provides keyboard shortcut info on hover)
      const navButtons = [
        { testId: "nav-agents", label: /Agents/i },
        { testId: "nav-kanban", label: /Kanban/i },
        { testId: "nav-graph", label: /Graph/i },
        { testId: "nav-ideation", label: /Ideation/i },
        { testId: "nav-extensibility", label: /Extensibility/i },
        { testId: "nav-activity", label: /Activity/i },
        { testId: "nav-settings", label: /Settings/i },
      ];
      for (const { testId, label } of navButtons) {
        const btn = screen.getByTestId(testId);
        expect(btn).toBeInTheDocument();
        expect(btn).toHaveTextContent(label);
      }
    });

    it("should start with Agents view active", () => {
      render(<App />);
      expect(screen.getByTestId("nav-agents")).toHaveAttribute("aria-current", "page");
      expect(screen.getByTestId("agents-view-mock")).toBeInTheDocument();
    });

    it("does not render the old page chat toggle on Kanban or Graph", async () => {
      const user = userEvent.setup();
      render(<App />);

      await user.click(screen.getByTestId("nav-kanban"));
      expect(screen.getByTestId("task-board-mock")).toBeInTheDocument();
      expect(screen.queryByTestId("chat-toggle")).not.toBeInTheDocument();

      await user.click(screen.getByTestId("nav-graph"));

      expect(screen.getByTestId("task-graph-view-mock")).toBeInTheDocument();
      expect(screen.queryByTestId("chat-toggle")).not.toBeInTheDocument();
    });

    it("should switch to Ideation view when clicked", async () => {
      const user = userEvent.setup();
      render(<App />);

      await user.click(screen.getByTestId("nav-ideation"));

      expect(screen.getByTestId("nav-ideation")).toHaveAttribute("aria-current", "page");
      expect(screen.getByTestId("ideation-view-mock")).toBeInTheDocument();
      expect(screen.queryByTestId("task-board-mock")).not.toBeInTheDocument();
    });

    it("should pass footer with ExecutionControlBar to Ideation view", async () => {
      const user = userEvent.setup();
      render(<App />);

      await user.click(screen.getByTestId("nav-ideation"));

      // Verify footer is rendered in the ideation view mock
      expect(screen.getByTestId("ideation-footer-mock")).toBeInTheDocument();
    });

    it("should switch to Extensibility view when clicked", async () => {
      const user = userEvent.setup();
      render(<App />);

      await user.click(screen.getByTestId("nav-extensibility"));

      expect(screen.getByTestId("nav-extensibility")).toHaveAttribute("aria-current", "page");
      expect(screen.getByTestId("extensibility-view-mock")).toBeInTheDocument();
      expect(screen.queryByTestId("task-board-mock")).not.toBeInTheDocument();
    });

    it("should switch to Activity view when clicked", async () => {
      const user = userEvent.setup();
      render(<App />);

      await user.click(screen.getByTestId("nav-activity"));

      expect(screen.getByTestId("nav-activity")).toHaveAttribute("aria-current", "page");
      expect(screen.getByTestId("activity-view-mock")).toBeInTheDocument();
      expect(screen.queryByTestId("task-board-mock")).not.toBeInTheDocument();
    });

    it("should open Settings modal when clicked", async () => {
      const user = userEvent.setup();
      render(<App />);

      await user.click(screen.getByTestId("nav-settings"));

      // Settings is now a modal — activeModal is set, current view stays visible underneath
      expect(useUiStore.getState().activeModal).toBe("settings");
      expect(useUiStore.getState().modalContext).toBeUndefined();
      expect(screen.getByTestId("agents-view-mock")).toBeInTheDocument();
    });

    it("should switch views correctly multiple times", async () => {
      const user = userEvent.setup();
      render(<App />);

      // Start on Agents
      expect(screen.getByTestId("agents-view-mock")).toBeInTheDocument();

      // Go to Activity
      await user.click(screen.getByTestId("nav-activity"));
      expect(screen.getByTestId("activity-view-mock")).toBeInTheDocument();

      // Open Settings modal
      await user.click(screen.getByTestId("nav-settings"));
      expect(useUiStore.getState().activeModal).toBe("settings");

      // Go back to Kanban
      await user.click(screen.getByTestId("nav-kanban"));
      expect(screen.getByTestId("task-board-mock")).toBeInTheDocument();
    });

    it("should remove aria-current from previous nav when switching", async () => {
      const user = userEvent.setup();
      render(<App />);

      // Agents is active initially
      expect(screen.getByTestId("nav-agents")).toHaveAttribute("aria-current", "page");
      expect(screen.getByTestId("nav-activity")).not.toHaveAttribute("aria-current");

      // Switch to Activity
      await user.click(screen.getByTestId("nav-activity"));

      // Activity is now active, Kanban is not
      expect(screen.getByTestId("nav-activity")).toHaveAttribute("aria-current", "page");
      expect(screen.getByTestId("nav-agents")).not.toHaveAttribute("aria-current");
    });

    it("restores saved view and selected task for the active project on initial render", async () => {
      useUiStore.setState({
        viewByProject: { "demo-project-1": "graph" },
        selectedTaskByProject: { "demo-project-1": "task-42" },
      });

      render(<App />);

      await waitFor(() => {
        expect(useUiStore.getState().currentView).toBe("graph");
      });
      expect(useUiStore.getState().selectedTaskId).toBe("task-42");
      expect(screen.getByTestId("task-graph-view-mock")).toBeInTheDocument();
    });
  });

  describe("Keyboard Shortcuts", () => {
    it("should switch to Agents with Cmd+1", () => {
      useUiStore.setState({ viewByProject: { "demo-project-1": "activity" } });
      render(<App />);

      fireEvent.keyDown(window, { key: "1", metaKey: true });

      expect(useUiStore.getState().currentView).toBe("agents");
    });

    it("should switch to Ideation with Cmd+2", () => {
      useUiStore.setState({ viewByProject: { "demo-project-1": "activity" } });
      render(<App />);

      fireEvent.keyDown(window, { key: "2", metaKey: true });

      expect(useUiStore.getState().currentView).toBe("ideation");
    });

    it("should switch to Graph with Cmd+3", () => {
      useUiStore.setState({ viewByProject: { "demo-project-1": "ideation" } });
      render(<App />);

      fireEvent.keyDown(window, { key: "3", metaKey: true });

      expect(useUiStore.getState().currentView).toBe("graph");
    });

    it("should switch to Kanban with Cmd+4", () => {
      useUiStore.setState({ viewByProject: { "demo-project-1": "ideation" } });
      render(<App />);

      fireEvent.keyDown(window, { key: "4", metaKey: true });

      expect(useUiStore.getState().currentView).toBe("kanban");
    });

    it("should switch to Insights with Cmd+5", () => {
      useUiStore.setState({ viewByProject: { "demo-project-1": "ideation" } });
      render(<App />);

      fireEvent.keyDown(window, { key: "5", metaKey: true });

      expect(useUiStore.getState().currentView).toBe("insights");
    });

    it("should open Settings dialog with Cmd+6", () => {
      render(<App />);

      fireEvent.keyDown(window, { key: "6", metaKey: true });

      expect(useUiStore.getState().activeModal).toBe("settings");
      expect(useUiStore.getState().modalContext).toBeUndefined();
    });

    it("should work with Ctrl key (for non-Mac)", () => {
      useUiStore.setState({ viewByProject: { "demo-project-1": "ideation" } });
      render(<App />);

      fireEvent.keyDown(window, { key: "4", ctrlKey: true });

      expect(useUiStore.getState().currentView).toBe("kanban");
    });

    it("should not switch views when pressing number without modifier", () => {
      render(<App />);

      fireEvent.keyDown(window, { key: "4" });

      // Should still be on agents (default)
      expect(useUiStore.getState().currentView).toBe("agents");
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

      // View nav items are gone
      expect(screen.queryByTestId("nav-agents")).toBeNull();
      expect(screen.queryByTestId("nav-ideation")).toBeNull();
      expect(screen.queryByTestId("nav-graph")).toBeNull();
      expect(screen.queryByTestId("nav-kanban")).toBeNull();

      // v27 topbar controls stay mounted even when the project-scoped rail collapses.
      expect(screen.getByTestId("reviews-toggle")).toBeInTheDocument();

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
  });

  describe("Execution Status Query Scoping", () => {
    it("should call useExecutionStatus with undefined when no active project", () => {
      // This test verifies Phase 82 requirement: execution status queries are scoped to active project
      // When no project is set, activeProjectId is null, so currentProjectId = ""
      // and "" || undefined = undefined
      useProjectStore.setState({ activeProjectId: null, projects: {}, isInitialized: false });
      render(<App />);

      // useExecutionStatus should be called with undefined (no active project)
      expect(vi.mocked(useExecutionStatus)).toHaveBeenCalledWith(undefined);
    });

    it("should call useExecutionStatus with project ID when project is active", () => {
      // Set up an active project BEFORE rendering
      useProjectStore.setState({ activeProjectId: "test-project-123" });

      render(<App />);

      // When a project is active, useExecutionStatus should be called with that project ID
      expect(vi.mocked(useExecutionStatus)).toHaveBeenCalledWith("test-project-123");
    });
  });
});
