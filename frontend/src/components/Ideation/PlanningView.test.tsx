import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, fireEvent, render as rtlRender, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { PlanningView } from "./PlanningView";
import type { IdeationSession, TaskProposal } from "@/types/ideation";
import { useIdeationStore } from "@/stores/ideationStore";
import { useProposalStore } from "@/stores/proposalStore";
import { ideationApi } from "@/api/ideation";

const mockToastError = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => mockToastError(...args),
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/api/ideation", () => ({
  ideationApi: {
    sessions: {
      getChildren: vi.fn().mockResolvedValue([]),
      getLatestChildSessionId: vi.fn().mockResolvedValue({
        sessionId: "session-1",
        purpose: "verification",
        latestChildSessionId: null,
      }),
    },
  },
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: () => () => {},
    emit: vi.fn(),
  }),
}));

// Mock for plan store
const mockClearActivePlan = vi.fn();
const mockSetActivePlan = vi.fn().mockResolvedValue(undefined);
const mockSetIsPlanExpanded = vi.fn();
const mockActivePlanByProject: Record<string, string | null> = {};

// Configurable isPlanExpanded for testing tab-switch effects
let mockIsPlanExpanded = false;

vi.mock("@/hooks/useDependencyGraph", () => ({
  useDependencyGraph: () => ({ data: { proposals: [], edges: [], warnings: [] }, isFetching: false }),
}));

// Mock for useIdeation - will be replaced per-test for reopen tests
const mockReopenMutate = vi.fn();
const mockResetMutate = vi.fn();

vi.mock("@/hooks/useIdeation", () => ({
  useReopenSession: () => ({ mutate: mockReopenMutate, isPending: false }),
  useResetAndReaccept: () => ({ mutate: mockResetMutate, isPending: false }),
  useIdeationSessions: () => ({ data: [] }),
  ideationKeys: {
    sessions: () => ["ideation", "sessions"],
    sessionList: (projectId: string) => ["ideation", "sessions", "list", projectId],
    sessionWithData: (sessionId: string) => ["ideation", "sessions", "detail", sessionId, "with-data"],
    sessionGroupCounts: (projectId: string, search?: string) => ["ideation", "sessions", "counts", projectId, ...(search ? [search] : [])],
    sessionsByGroup: (projectId: string, group: string, search?: string) => ["ideation", "sessions", "group", projectId, group, ...(search ? [search] : [])],
    proposals: () => ["ideation", "proposals"],
  },
}));

vi.mock("@/hooks/useFileDrop", () => ({
  useFileDrop: () => ({ isDragging: false, dropProps: {}, error: null }),
}));

vi.mock("./useIdeationHandlers", () => ({
  useIdeationHandlers: (
    session: IdeationSession | null,
    _proposals: TaskProposal[],
    _onRemoveProposal: (proposalId: string) => void,
    _onReorderProposals: (proposalIds: string[]) => void,
    onArchiveSession: (sessionId: string) => void
  ) => ({
    highlightedProposalIds: new Set<string>(),
    isPlanExpanded: mockIsPlanExpanded,
    setIsPlanExpanded: mockSetIsPlanExpanded,
    importStatus: null,
    setImportStatus: vi.fn(),
    fileInputRef: { current: null },
    handleArchive: () => {
      if (session) onArchiveSession(session.id);
    },
    handleClearAll: vi.fn(),
    handleReviewSync: vi.fn(),
    handleUndoSync: vi.fn(),
    handleDismissSync: vi.fn(),
    handleImportPlan: vi.fn(),
    handleFileSelected: vi.fn(),
    handleFileDrop: vi.fn(),
  }),
}));

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({ headerContent }: { headerContent?: ReactElement }) => (
    <div data-testid="integrated-chat-panel">{headerContent}</div>
  ),
}));

vi.mock("./PlanBrowser", () => ({
  PlanBrowser: () => <div data-testid="plan-browser" />,
}));

vi.mock("./StartSessionPanel", () => ({
  StartSessionPanel: ({ onNewSession }: { onNewSession: () => void }) => (
    <div data-testid="start-session-panel">
      <button onClick={onNewSession}>Start Session</button>
    </div>
  ),
}));

vi.mock("./ProposalsToolbar", () => ({
  ProposalsToolbar: ({ onAcceptPlan }: { onAcceptPlan: (targetColumn: string) => void }) => (
    <button onClick={() => onAcceptPlan("backlog")}>Accept Plan</button>
  ),
}));

vi.mock("./TieredProposalList", () => ({
  TieredProposalList: ({ proposals }: { proposals: TaskProposal[] }) => (
    <div data-testid="tiered-proposal-list">{proposals.length}</div>
  ),
}));

vi.mock("./ProposalsEmptyState", () => ({
  ProposalsEmptyState: () => <div data-testid="proposals-empty-state" />,
}));

vi.mock("./DropZoneOverlay", () => ({
  DropZoneOverlay: () => null,
}));

vi.mock("./AcceptedSessionBanner", () => ({
  AcceptedSessionBanner: () => <div data-testid="accepted-session-banner" />,
}));

vi.mock("./AcceptModal", () => ({
  AcceptModal: ({ isOpen, onAccept, sessionId, proposals }: {
    isOpen: boolean;
    onAccept: (options: { sessionId: string; proposalIds: string[]; targetColumn: string }) => void;
    sessionId: string;
    proposals: Array<{ id: string }>;
  }) =>
    isOpen ? (
      <button
        data-testid="accept-modal-confirm"
        onClick={() =>
          onAccept({
            sessionId,
            proposalIds: proposals.map((p) => p.id),
            targetColumn: "backlog",
          })
        }
      >
        Confirm Accept
      </button>
    ) : null,
}));

vi.mock("./PlanDisplay", () => ({
  PlanDisplay: () => <div data-testid="plan-display" />,
}));

vi.mock("./ProactiveSyncNotification", () => ({
  ProactiveSyncNotificationBanner: () => null,
}));

vi.mock("./ReopenSessionDialog", () => ({
  ReopenSessionDialog: ({ open, onConfirm }: { open: boolean; onConfirm: () => void }) =>
    open ? <button onClick={onConfirm}>Confirm Reopen</button> : null,
}));

vi.mock("@/stores/planStore", () => ({
  usePlanStore: vi.fn((selector: (state: Record<string, unknown>) => unknown) => {
    const state = {
      setActivePlan: mockSetActivePlan,
      clearActivePlan: mockClearActivePlan,
      activePlanByProject: mockActivePlanByProject,
      planCandidates: [],
      isLoading: false,
      error: null,
      loadActivePlan: vi.fn(),
      loadCandidates: vi.fn(),
    };
    return selector ? selector(state) : state;
  }),
}));

vi.mock("@/stores/projectStore", () => ({
  useProjectStore: vi.fn((selector: (state: Record<string, unknown>) => unknown) => {
    const state = {
      activeProjectId: "project-1",
      projects: {},
      setProjects: vi.fn(),
      updateProject: vi.fn(),
      selectProject: vi.fn(),
      addProject: vi.fn(),
      removeProject: vi.fn(),
    };
    return selector ? selector(state) : state;
  }),
  selectActiveProject: (state: Record<string, unknown>) =>
    state.activeProjectId ? state.projects?.[state.activeProjectId as string] ?? null : null,
}));

vi.mock("@/components/ui/ResizeHandle", () => ({
  CHAT_PANEL_DEFAULT_WIDTH: 420,
  CHAT_PANEL_MIN_WIDTH: 320,
  ResizeHandle: ({
    isResizing,
    onDoubleClick,
    onMouseDown,
    testId,
  }: {
    isResizing: boolean;
    onDoubleClick?: React.MouseEventHandler<HTMLDivElement>;
    onMouseDown: React.MouseEventHandler<HTMLDivElement>;
    testId?: string;
  }) => (
    <div
      data-resizing={String(isResizing)}
      data-testid={testId ?? "resize-handle"}
      onDoubleClick={onDoubleClick}
      onMouseDown={onMouseDown}
    />
  ),
}));

if (!HTMLElement.prototype.scrollTo) {
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    value: vi.fn(),
    writable: true,
  });
}

const mockSession: IdeationSession = {
  id: "session-1",
  projectId: "project-1",
  title: "Authentication Feature",
  status: "active",
  planArtifactId: null,
  createdAt: "2026-01-24T00:00:00Z",
  updatedAt: "2026-01-24T01:00:00Z",
  archivedAt: null,
  convertedAt: null,
};

const mockProposals: TaskProposal[] = [
  {
    id: "proposal-1",
    sessionId: "session-1",
    title: "Setup database",
    description: "Initialize SQLite database",
    category: "setup",
    steps: [],
    acceptanceCriteria: [],
    suggestedPriority: "high",
    priorityScore: 75,
    priorityReason: "Foundation task",
    estimatedComplexity: "moderate",
    userPriority: null,
    userModified: false,
    status: "pending",
    createdTaskId: null,
    sortOrder: 0,
    createdAt: "2026-01-24T00:00:00Z",
    updatedAt: "2026-01-24T00:00:00Z",
  },
  {
    id: "proposal-2",
    sessionId: "session-1",
    title: "Create login form",
    description: "Build the login UI",
    category: "feature",
    steps: [],
    acceptanceCriteria: [],
    suggestedPriority: "medium",
    priorityScore: 55,
    priorityReason: "Depends on database",
    estimatedComplexity: "simple",
    userPriority: null,
    userModified: false,
    status: "pending",
    createdTaskId: null,
    sortOrder: 1,
    createdAt: "2026-01-24T00:00:00Z",
    updatedAt: "2026-01-24T00:00:00Z",
  },
];

function render(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return rtlRender(
    <QueryClientProvider client={queryClient}>
      {ui}
    </QueryClientProvider>
  );
}

describe("PlanningView", () => {
  const defaultProps = {
    session: mockSession,
    proposals: mockProposals,
    onNewSession: vi.fn(),
    onSelectSession: vi.fn(),
    onArchiveSession: vi.fn(),
    onEditProposal: vi.fn(),
    onRemoveProposal: vi.fn(),
    onReorderProposals: vi.fn(),
    onApply: vi.fn().mockResolvedValue({
      createdTaskIds: [],
      dependenciesCreated: 0,
      warnings: [],
      sessionConverted: false,
      executionPlanId: null,
    }),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockSetIsPlanExpanded.mockClear();
    mockIsPlanExpanded = false;
    localStorage.clear();
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      configurable: true,
      value: vi.fn((query: string) => ({
        matches: query === "(min-width: 1440px)" || query === "(min-width: 1280px)",
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });
    // Reset proposal store state between tests
    useProposalStore.setState({ lastProposalAddedAt: {} });
  });

  it("renders the main view and active-session layout", () => {
    render(<PlanningView {...defaultProps} />);
    expect(screen.getByTestId("ideation-view")).toBeInTheDocument();
    expect(screen.getByTestId("plan-browser")).toBeInTheDocument();
    expect(screen.getByTestId("ideation-header")).toBeInTheDocument();
    expect(screen.getByTestId("proposals-panel")).toBeInTheDocument();
    expect(screen.getByTestId("conversation-panel")).toBeInTheDocument();
  });

  it("resizes the desktop Plan Browser sidebar and resets the saved width", async () => {
    render(<PlanningView {...defaultProps} />);

    const sidebarContainer = screen.getByTestId("ideation-sidebar-container");
    vi.spyOn(sidebarContainer, "getBoundingClientRect").mockReturnValue({
      bottom: 720,
      height: 720,
      left: 0,
      right: 340,
      top: 0,
      width: 340,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect);

    const handle = screen.getByTestId("ideation-sidebar-resize-handle");
    expect(handle).toHaveAttribute("data-resizing", "false");

    fireEvent.mouseDown(handle, { clientX: 340 });
    await vi.waitFor(() => expect(handle).toHaveAttribute("data-resizing", "true"));
    expect(sidebarContainer.style.transition).toBe("none");

    fireEvent.mouseMove(document, { clientX: 420 });
    fireEvent.mouseUp(document);

    await vi.waitFor(() => {
      expect(sidebarContainer.style.width).toBe("420px");
      expect(sidebarContainer.style.minWidth).toBe("420px");
      expect(handle).toHaveAttribute("data-resizing", "false");
    });
    expect(localStorage.getItem("ralphx-plan-browser-sidebar-width")).toBe("420");

    fireEvent.doubleClick(handle);

    await vi.waitFor(() => {
      expect(sidebarContainer.style.width).toBe("340px");
      expect(sidebarContainer.style.minWidth).toBe("340px");
    });
    expect(localStorage.getItem("ralphx-plan-browser-sidebar-width")).toBeNull();
  });

  it("shows title and proposal count in the header", () => {
    render(<PlanningView {...defaultProps} />);
    expect(screen.getByText("Authentication Feature")).toBeInTheDocument();
    expect(screen.getByText("2 proposals")).toBeInTheDocument();
  });

  it("calls onArchiveSession when Archive is clicked", async () => {
    const onArchiveSession = vi.fn();
    const user = userEvent.setup();
    render(<PlanningView {...defaultProps} onArchiveSession={onArchiveSession} />);

    await user.click(screen.getByRole("button", { name: /Archive/i }));
    expect(onArchiveSession).toHaveBeenCalledWith("session-1");
  });

  it("calls onApply with all proposal IDs when accepting plan", async () => {
    const onApply = vi.fn().mockResolvedValue({
      createdTaskIds: ["task-1", "task-2"],
      dependenciesCreated: 1,
      warnings: [],
      sessionConverted: true,
      executionPlanId: "exec-plan-1",
    });
    const user = userEvent.setup();
    render(<PlanningView {...defaultProps} onApply={onApply} />);

    // Switch to the Proposals tab (ProposalsToolbar now lives there)
    await user.click(screen.getByTestId("tab-proposals"));

    // Click "Accept Plan" to open the modal
    await user.click(screen.getByRole("button", { name: "Accept Plan" }));
    // Click confirm in the modal
    await user.click(screen.getByTestId("accept-modal-confirm"));

    expect(onApply).toHaveBeenCalledWith({
      sessionId: "session-1",
      proposalIds: ["proposal-1", "proposal-2"],
      targetColumn: "backlog",
    });
  });

  it("always shows proposals tab, hides count badge when empty", () => {
    render(<PlanningView {...defaultProps} proposals={[]} />);
    // Proposals tab is always visible
    expect(screen.queryByTestId("tab-proposals")).toBeInTheDocument();
    // Count badge is not shown when there are no proposals
    expect(screen.queryByTestId("tab-proposals-count")).not.toBeInTheDocument();
  });

  it("renders start-session panel when there is no active session", async () => {
    const onNewSession = vi.fn();
    const user = userEvent.setup();

    render(
      <PlanningView
        {...defaultProps}
        session={null}
        onNewSession={onNewSession}
      />
    );

    expect(screen.getByTestId("start-session-panel")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Start Session" }));
    expect(onNewSession).toHaveBeenCalledTimes(1);
  });

  it("hides Archive action for read-only session", () => {
    render(
      <PlanningView
        {...defaultProps}
        session={{ ...mockSession, status: "accepted" }}
      />
    );

    expect(screen.queryByRole("button", { name: /Archive/i })).not.toBeInTheDocument();
  });

  it("sets active plan after accepting proposals", async () => {
    const onApply = vi.fn().mockResolvedValue({
      createdTaskIds: ["task-1"],
      dependenciesCreated: 0,
      warnings: [],
      sessionConverted: true,
      executionPlanId: "exec-plan-abc",
    });
    const user = userEvent.setup();
    render(<PlanningView {...defaultProps} onApply={onApply} />);

    // Switch to the Proposals tab (ProposalsToolbar now lives there)
    await user.click(screen.getByTestId("tab-proposals"));

    // Click "Accept Plan" to open the modal, then confirm
    await user.click(screen.getByRole("button", { name: "Accept Plan" }));
    await user.click(screen.getByTestId("accept-modal-confirm"));

    // Wait for async operations to complete
    await vi.waitFor(() => {
      expect(onApply).toHaveBeenCalledWith({
        sessionId: "session-1",
        proposalIds: ["proposal-1", "proposal-2"],
        targetColumn: "backlog",
      });
    });

    // Verify setActivePlan was called with executionPlanId for atomic update
    await vi.waitFor(() => {
      expect(mockSetActivePlan).toHaveBeenCalledWith(
        "project-1",
        "session-1",
        "ideation",
        "exec-plan-abc"
      );
    });
  });

  it("keeps modal open and shows toast.error when onApply rejects", async () => {
    const onApply = vi.fn().mockRejectedValue(new Error("Branch creation failed"));
    const user = userEvent.setup();
    render(<PlanningView {...defaultProps} onApply={onApply} />);

    await user.click(screen.getByTestId("tab-proposals"));
    await user.click(screen.getByRole("button", { name: "Accept Plan" }));
    await user.click(screen.getByTestId("accept-modal-confirm"));

    await vi.waitFor(() => {
      expect(mockToastError).toHaveBeenCalledWith("Branch creation failed");
    });
    // Modal should still be open (confirm button still rendered)
    expect(screen.getByTestId("accept-modal-confirm")).toBeInTheDocument();
  });

  it("closes modal after onApply resolves with sessionConverted=true", async () => {
    const onApply = vi.fn().mockResolvedValue({
      createdTaskIds: ["task-1"],
      dependenciesCreated: 0,
      warnings: [],
      sessionConverted: true,
      executionPlanId: "exec-plan-1",
    });
    const user = userEvent.setup();
    render(<PlanningView {...defaultProps} onApply={onApply} />);

    await user.click(screen.getByTestId("tab-proposals"));
    await user.click(screen.getByRole("button", { name: "Accept Plan" }));
    await user.click(screen.getByTestId("accept-modal-confirm"));

    await vi.waitFor(() => {
      expect(screen.queryByTestId("accept-modal-confirm")).not.toBeInTheDocument();
    });
    expect(mockToastError).not.toHaveBeenCalled();
  });

  it("does not set active plan when accept does not convert the session", async () => {
    const onApply = vi.fn().mockResolvedValue({
      createdTaskIds: [],
      dependenciesCreated: 0,
      warnings: ["Execution plan already active"],
      sessionConverted: false,
      executionPlanId: "exec-plan-stale",
    });
    const user = userEvent.setup();

    render(<PlanningView {...defaultProps} onApply={onApply} />);

    await user.click(screen.getByTestId("tab-proposals"));
    await user.click(screen.getByRole("button", { name: "Accept Plan" }));
    await user.click(screen.getByTestId("accept-modal-confirm"));

    await vi.waitFor(() => {
      expect(onApply).toHaveBeenCalledWith({
        sessionId: "session-1",
        proposalIds: ["proposal-1", "proposal-2"],
        targetColumn: "backlog",
      });
    });

    expect(mockSetActivePlan).not.toHaveBeenCalled();
  });

  it("clears active plan when reopening a session that was the active plan", async () => {
    // Setup: session-1 is the active plan
    mockActivePlanByProject["project-1"] = "session-1";
    mockClearActivePlan.mockResolvedValue(undefined);

    const acceptedSession = {
      ...mockSession,
      status: "accepted" as const,
      convertedAt: "2026-01-24T02:00:00Z",
    };

    const user = userEvent.setup();
    render(
      <PlanningView
        {...defaultProps}
        session={acceptedSession}
      />
    );

    // Click the Reopen button
    const reopenButton = screen.getByRole("button", { name: /Reopen/i });
    await user.click(reopenButton);
    await user.click(screen.getByRole("button", { name: "Confirm Reopen" }));

    // This should trigger the reopen dialog (mocked away)
    // Simulate the mutation succeeding by calling the onSuccess callback
    expect(mockReopenMutate).toHaveBeenCalledWith(
      "session-1",
      expect.objectContaining({
        onSuccess: expect.any(Function),
        onError: expect.any(Function),
      })
    );

    // Extract and call the onSuccess handler
    const onSuccessHandler = mockReopenMutate.mock.calls[0][1].onSuccess;
    await onSuccessHandler();

    // Verify clearActivePlan was called with the correct project ID
    expect(mockClearActivePlan).toHaveBeenCalledWith("project-1");
  });

  it("does not clear active plan when reopening a different session", async () => {
    // Setup: session-2 is the active plan, we're reopening session-1
    mockActivePlanByProject["project-1"] = "session-2";
    mockClearActivePlan.mockClear();

    const acceptedSession = {
      ...mockSession,
      status: "accepted" as const,
      convertedAt: "2026-01-24T02:00:00Z",
    };

    const user = userEvent.setup();
    render(
      <PlanningView
        {...defaultProps}
        session={acceptedSession}
      />
    );

    // Click the Reopen button
    const reopenButton = screen.getByRole("button", { name: /Reopen/i });
    await user.click(reopenButton);
    await user.click(screen.getByRole("button", { name: "Confirm Reopen" }));

    // Simulate the mutation succeeding
    const onSuccessHandler = mockReopenMutate.mock.calls[0][1].onSuccess;
    await onSuccessHandler();

    // Verify clearActivePlan was NOT called
    expect(mockClearActivePlan).not.toHaveBeenCalled();
  });

  it("does not clear active plan when no active plan is set", async () => {
    // Setup: no active plan
    mockActivePlanByProject["project-1"] = null;
    mockClearActivePlan.mockClear();

    const acceptedSession = {
      ...mockSession,
      status: "accepted" as const,
      convertedAt: "2026-01-24T02:00:00Z",
    };

    const user = userEvent.setup();
    render(
      <PlanningView
        {...defaultProps}
        session={acceptedSession}
      />
    );

    // Click the Reopen button
    const reopenButton = screen.getByRole("button", { name: /Reopen/i });
    await user.click(reopenButton);
    await user.click(screen.getByRole("button", { name: "Confirm Reopen" }));

    // Simulate the mutation succeeding
    const onSuccessHandler = mockReopenMutate.mock.calls[0][1].onSuccess;
    await onSuccessHandler();

    // Verify clearActivePlan was NOT called
    expect(mockClearActivePlan).not.toHaveBeenCalled();
  });

  it("renders footer when footer prop is provided with active session", () => {
    const footer = <div data-testid="test-footer-content">Footer Content</div>;
    render(<PlanningView {...defaultProps} footer={footer} />);

    // The component wraps the footer in a div with data-testid="ideation-footer"
    expect(screen.getByTestId("ideation-footer")).toBeInTheDocument();
    expect(screen.getByTestId("test-footer-content")).toBeInTheDocument();
    expect(screen.getByText("Footer Content")).toBeInTheDocument();
    // Verify proposals panel still renders alongside footer
    expect(screen.getByTestId("proposals-panel")).toBeInTheDocument();
  });

  it("renders StartSessionPanel and footer with no session when footer is provided", () => {
    // With the refactored layout, footer now renders at top level (outside session conditional)
    // so it appears in both active and no-session states
    const footer = <div data-testid="test-exec-bar">Execution Bar</div>;
    render(
      <PlanningView
        {...defaultProps}
        session={null}
        footer={footer}
      />
    );

    // StartSessionPanel should render
    expect(screen.getByTestId("start-session-panel")).toBeInTheDocument();
    // Footer should now be visible at the top level
    expect(screen.getByTestId("ideation-footer")).toBeInTheDocument();
    expect(screen.getByTestId("test-exec-bar")).toBeInTheDocument();
    expect(screen.getByText("Execution Bar")).toBeInTheDocument();
  });

  it("does not render footer when footer prop is not provided (backward compatible)", () => {
    render(<PlanningView {...defaultProps} />);

    expect(screen.queryByTestId("ideation-footer")).not.toBeInTheDocument();
    // Verify the view still renders normally
    expect(screen.getByTestId("ideation-view")).toBeInTheDocument();
    expect(screen.getByTestId("proposals-panel")).toBeInTheDocument();
  });

  it("auto-expands plan when navigating to existing session with a planArtifactId", () => {
    const sessionWithPlan = { ...mockSession, planArtifactId: "plan-artifact-123" };
    render(<PlanningView {...defaultProps} session={sessionWithPlan} proposals={mockProposals} />);
    expect(mockSetIsPlanExpanded).toHaveBeenCalledWith(true);
  });

  it("does not auto-expand plan when session has no planArtifactId", () => {
    render(<PlanningView {...defaultProps} session={mockSession} proposals={[]} />);
    expect(mockSetIsPlanExpanded).not.toHaveBeenCalledWith(true);
  });

  describe("Proposals tab auto-switch", () => {
    it("does not collapse plan when a new proposal arrives (switches tab instead)", () => {
      mockIsPlanExpanded = true;
      render(<PlanningView {...defaultProps} />);
      mockSetIsPlanExpanded.mockClear(); // clear calls from initial render
      // Simulate a new proposal arriving
      act(() => {
        useProposalStore.setState({ lastProposalAddedAt: { "session-1": Date.now() } });
      });
      // Old behavior (auto-collapse) should NOT happen
      expect(mockSetIsPlanExpanded).not.toHaveBeenCalledWith(false);
    });

    it("does not collapse plan when proposals load after auto-open", () => {
      mockIsPlanExpanded = true;
      render(<PlanningView {...defaultProps} proposals={[]} />);
      mockSetIsPlanExpanded.mockClear(); // clear calls from initial render
      // Old behavior (auto-collapse) should NOT happen when proposals arrive
      expect(mockSetIsPlanExpanded).not.toHaveBeenCalledWith(false);
    });

    it("switches to Proposals tab via store when new proposal arrives and plan is expanded", () => {
      mockIsPlanExpanded = true;
      const originalFn = useIdeationStore.getState().setActiveIdeationTab;
      const spySetActiveIdeationTab = vi.fn(originalFn);
      useIdeationStore.setState({ setActiveIdeationTab: spySetActiveIdeationTab });
      render(<PlanningView {...defaultProps} />);
      act(() => {
        useProposalStore.setState({ lastProposalAddedAt: { "session-1": Date.now() } });
      });
      expect(spySetActiveIdeationTab).toHaveBeenCalledWith("session-1", "proposals");
      // Restore original function to avoid affecting other tests
      useIdeationStore.setState({ setActiveIdeationTab: originalFn });
    });
  });

  describe("Verification tab", () => {
    it("does not show Verification tab when verificationStatus is unverified and no planArtifact", () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "unverified", planArtifactId: null }}
        />
      );
      expect(screen.queryByTestId("tab-verification")).not.toBeInTheDocument();
    });

    it("shows Verification tab when verificationStatus is reviewing", () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "reviewing" }}
        />
      );
      expect(screen.getByTestId("tab-verification")).toBeInTheDocument();
    });

    it("shows Verification tab when verificationStatus is verified", () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "verified", planArtifactId: "plan-1" }}
        />
      );
      expect(screen.getByTestId("tab-verification")).toBeInTheDocument();
    });

    it("shows Verification tab when verificationStatus is needs_revision", () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "needs_revision", planArtifactId: "plan-1" }}
        />
      );
      expect(screen.getByTestId("tab-verification")).toBeInTheDocument();
    });

    it("renders Verification tab between Plan and Proposals in the tab bar", () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "reviewing" }}
        />
      );
      const planTab = screen.getByTestId("tab-plan");
      const verificationTab = screen.getByTestId("tab-verification");
      const proposalsTab = screen.getByTestId("tab-proposals");
      // Check DOM ordering using compareDocumentPosition
      expect(planTab.compareDocumentPosition(verificationTab) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(verificationTab.compareDocumentPosition(proposalsTab) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });

    it("shows pulsing badge when verificationInProgress is true", () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "reviewing", verificationInProgress: true }}
        />
      );
      expect(screen.getByTestId("verification-badge-in-progress")).toBeInTheDocument();
    });

    it("shows checkmark badge when verificationStatus is verified", () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "verified", planArtifactId: "plan-1" }}
        />
      );
      expect(screen.getByTestId("verification-badge-verified")).toBeInTheDocument();
    });

    it("does not show a verified badge when no plan exists and verification is not active", () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{
            ...mockSession,
            verificationStatus: "verified",
            planArtifactId: null,
            inheritedPlanArtifactId: null,
            verificationInProgress: false,
          }}
        />
      );
      expect(screen.queryByTestId("verification-badge-verified")).not.toBeInTheDocument();
    });

    it("shows warning badge when verificationStatus is needs_revision", () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "needs_revision", planArtifactId: "plan-1" }}
        />
      );
      expect(screen.getByTestId("verification-badge-warning")).toBeInTheDocument();
    });

    it("badge is visible when Plan tab is active (not just Verification tab)", async () => {
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "needs_revision", planArtifactId: "plan-1" }}
        />
      );
      // Plan tab is active by default — badge should still be visible in the tab bar
      expect(screen.getByTestId("content-tab-bar")).toBeInTheDocument();
      expect(screen.getByTestId("verification-badge-warning")).toBeInTheDocument();
    });

    it("switches to Verification tab content when tab is clicked", async () => {
      const user = userEvent.setup();
      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "reviewing" }}
        />
      );
      await user.click(screen.getByTestId("tab-verification"));
      expect(screen.getByTestId("verification-tab-content")).toBeInTheDocument();
    });

    it("renders IntegratedChatPanel for verification child when lastVerificationChildId is set and activeVerificationChildId is null", () => {
      // Set lastVerificationChildId (persisted after agent terminates) with no active child
      useIdeationStore.setState({
        lastVerificationChildId: { "session-1": "child-session-99" },
        activeVerificationChildId: { "session-1": null },
        activeIdeationTab: { "session-1": "verification" },
      });

      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "verified" }}
        />
      );

      // Verification child panel mounts because lastVerificationChildId is set
      const panels = screen.getAllByTestId("integrated-chat-panel");
      // Both parent and child panels should be mounted
      expect(panels.length).toBe(2);
      // Child panel header contains "Verification" text (may appear in tab + panel header)
      expect(screen.getAllByText("Verification").length).toBeGreaterThanOrEqual(1);
    });

    it("renders IntegratedChatPanel for verification child when activeVerificationChildId is set and lastVerificationChildId is null", () => {
      useIdeationStore.setState({
        lastVerificationChildId: { "session-1": null },
        activeVerificationChildId: { "session-1": "child-session-42" },
        activeIdeationTab: { "session-1": "verification" },
      });

      render(
        <PlanningView
          {...defaultProps}
          session={{ ...mockSession, verificationStatus: "reviewing", verificationInProgress: true }}
        />
      );

      const panels = screen.getAllByTestId("integrated-chat-panel");
      expect(panels.length).toBe(2);
      expect(screen.getAllByText("Verification").length).toBeGreaterThanOrEqual(1);
    });

    it("preserves verification child routing across session switches", async () => {
      const user = userEvent.setup();
      const verificationSession = {
        ...mockSession,
        verificationStatus: "verified" as const,
        planArtifactId: "plan-1",
      };
      useIdeationStore.setState({
        lastVerificationChildId: { "session-1": "child-session-99" },
        activeVerificationChildId: { "session-1": null },
        activeIdeationTab: { "session-1": "plan" },
      });

      const { rerender } = render(
        <PlanningView {...defaultProps} session={verificationSession} />
      );

      const sessionTwo = { ...mockSession, id: "session-2" };
      rerender(
        <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
          <PlanningView {...defaultProps} session={sessionTwo} />
        </QueryClientProvider>
      );

      rerender(
        <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
          <PlanningView {...defaultProps} session={verificationSession} />
        </QueryClientProvider>
      );

      await user.click(screen.getByTestId("tab-verification"));

      const panels = screen.getAllByTestId("integrated-chat-panel");
      expect(panels.length).toBe(2);
      expect(useIdeationStore.getState().lastVerificationChildId["session-1"]).toBe("child-session-99");
    });
  });

  describe("eager verification child pre-population", () => {
    const mockGetLatestChildSessionId = ideationApi.sessions
      .getLatestChildSessionId as ReturnType<typeof vi.fn>;

    beforeEach(() => {
      mockGetLatestChildSessionId.mockResolvedValue({
        sessionId: "session-1",
        purpose: "verification",
        latestChildSessionId: null,
      });
      useIdeationStore.setState({
        lastVerificationChildId: {},
        activeVerificationChildId: {},
        activeIdeationTab: {},
      });
    });

    it("pre-populates lastVerificationChildId when eager query returns the latest child id", async () => {
      mockGetLatestChildSessionId.mockResolvedValue({
        sessionId: "session-1",
        purpose: "verification",
        latestChildSessionId: "child-abc",
      });

      const originalFn = useIdeationStore.getState().setLastVerificationChildId;
      const spy = vi.fn(originalFn);
      useIdeationStore.setState({ setLastVerificationChildId: spy });

      await act(async () => {
        render(
          <PlanningView
            {...defaultProps}
            session={{ ...mockSession, verificationStatus: "verified", planArtifactId: "plan-1" }}
          />
        );
      });

      await vi.waitFor(() => {
        expect(spy).toHaveBeenCalledWith("session-1", "child-abc");
      });

      // Restore original
      useIdeationStore.setState({ setLastVerificationChildId: originalFn });
    });

    it("no-ops when eager query returns no latest child id", async () => {
      mockGetLatestChildSessionId.mockResolvedValue({
        sessionId: "session-1",
        purpose: "verification",
        latestChildSessionId: null,
      });

      const originalFn = useIdeationStore.getState().setLastVerificationChildId;
      const spy = vi.fn();
      useIdeationStore.setState({ setLastVerificationChildId: spy });

      await act(async () => {
        render(
          <PlanningView
            {...defaultProps}
            session={{ ...mockSession, verificationStatus: "verified" }}
          />
        );
      });

      // Allow query to settle
      await new Promise((r) => setTimeout(r, 50));

      expect(spy).not.toHaveBeenCalledWith("session-1", expect.any(String));

      useIdeationStore.setState({ setLastVerificationChildId: originalFn });
    });

    it("does not overwrite lastVerificationChildId already populated by event", async () => {
      mockGetLatestChildSessionId.mockResolvedValue({
        sessionId: "session-1",
        purpose: "verification",
        latestChildSessionId: "child-from-query",
      });

      // Pre-populate store as if event-driven path already ran
      useIdeationStore.setState({
        lastVerificationChildId: { "session-1": "child-from-event" },
      });

      const originalFn = useIdeationStore.getState().setLastVerificationChildId;
      const spy = vi.fn();
      useIdeationStore.setState({ setLastVerificationChildId: spy });

      await act(async () => {
        render(
          <PlanningView
            {...defaultProps}
            session={{ ...mockSession, verificationStatus: "verified" }}
          />
        );
      });

      await new Promise((r) => setTimeout(r, 50));

      // Should not overwrite the event-driven value
      expect(spy).not.toHaveBeenCalledWith("session-1", "child-from-query");

      useIdeationStore.setState({ setLastVerificationChildId: originalFn });
    });

    it("does not call getLatestChildSessionId when sessionPurpose is verification", async () => {
      mockGetLatestChildSessionId.mockClear();

      await act(async () => {
        render(
          <PlanningView
            {...defaultProps}
            session={{ ...mockSession, sessionPurpose: "verification" }}
          />
        );
      });

      await new Promise((r) => setTimeout(r, 50));

      expect(mockGetLatestChildSessionId).not.toHaveBeenCalled();
    });
  });
});
