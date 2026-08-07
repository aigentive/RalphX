import { resetTransportEnvironmentId } from "@/lib/remote/active-environment";
import { resetQueryClient } from "@/lib/queryClient";
import { createElement, type ReactElement } from "react";
/**
 * TaskContextMenuItems.test.tsx - Tests for shared TaskContextMenuItems component
 *
 * Verifies correct actions render for different statuses and surfaces,
 * and that handlers are invoked properly.
 */

import { afterEach, describe, it, expect, vi, beforeEach } from "vitest";
import {
  render as rtlRender,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  TaskContextMenuItems,
  TaskContextMenuDialogs,
  TaskContextMenuProvider,
  useTaskContextMenu,
  type TaskContextMenuHandlers,
} from "./TaskContextMenuItems";
import type { Task } from "@/types/task";

// Gate tests park the store on a remote environment; without this the next file in
// the same worker inherits it and resolves a different keyed QueryClient. That is
// what broke EnvironmentScopedProviders under CI sharding.
afterEach(() => {
  resetQueryClient();
  resetTransportEnvironmentId();
  useEnvironmentStore.setState({ activeEnvironmentId: LOCAL_ENVIRONMENT_ID });
});

import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";

// Gated icon-only controls carry the app tooltip; `Tooltip` throws without a
// provider, which production gets from App.tsx.
function render(ui: ReactElement): ReturnType<typeof rtlRender> {
  return rtlRender(createElement(TooltipProvider, null, ui));
}

const REMOTE_ID = "remote-gate-test";

function setGateEnvironment(granted: boolean): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: REMOTE_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: REMOTE_ID, name: "Remote Mac", kind: "remote" },
    ],
    effectiveScopes: {
      [REMOTE_ID]: granted
        ? ["ui:read", "ui:operate", "ui:agent"]
        : ["ui:read", "ui:operate"],
    },
    connectionPresentations: {
      [REMOTE_ID]: {
        presentation: "connected",
        blockedFailure: null,
        blockedMessage: null,
      },
    },
  });
}

// ============================================================================
// Helpers
// ============================================================================

function createMockTask(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    category: "feature",
    title: "Test Task",
    description: "Test description",
    priority: 3,
    internalStatus: "backlog",
    needsReviewPoint: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    taskBranch: null,
    worktreePath: null,
    mergeCommitSha: null,
    metadata: null,
    ...overrides,
  };
}

function createMockHandlers(): TaskContextMenuHandlers {
  return {
    onViewDetails: vi.fn(),
    onEdit: vi.fn(),
    onArchive: vi.fn(),
    onRestore: vi.fn(),
    onPermanentDelete: vi.fn(),
    onStatusChange: vi.fn(),
    onBlockWithReason: vi.fn(),
    onUnblock: vi.fn(),
    onStartExecution: vi.fn(),
    onPause: vi.fn(),
    onResume: vi.fn(),
    onApprove: vi.fn(),
    onReject: vi.fn(),
    onRequestChanges: vi.fn(),
    onMarkResolved: vi.fn(),
    onStartIdeation: vi.fn(),
    onViewAgentChat: vi.fn(),
  };
}

/** Test wrapper that provides the required Provider + Dialogs */
function TestWrapper({
  task,
  handlers,
  context,
}: {
  task: Task;
  handlers: TaskContextMenuHandlers;
  context?: "kanban" | "graph";
}) {
  const state = useTaskContextMenu();
  return (
    <TaskContextMenuProvider state={state}>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div data-testid="trigger">Trigger</div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <TaskContextMenuItems
            task={task}
            handlers={handlers}
            context={context}
          />
        </ContextMenuContent>
        <TaskContextMenuDialogs task={task} handlers={handlers} />
      </ContextMenu>
    </TaskContextMenuProvider>
  );
}

function renderWithContextMenu(
  task: Task,
  handlers: TaskContextMenuHandlers,
  context?: "kanban" | "graph",
) {
  const result = render(
    <TestWrapper task={task} handlers={handlers} context={context} />,
  );

  // Open the context menu
  fireEvent.contextMenu(screen.getByTestId("trigger"));

  return result;
}

// ============================================================================
// Tests
// ============================================================================

describe("TaskContextMenuItems", () => {
  let handlers: TaskContextMenuHandlers;

  beforeEach(() => {
    handlers = createMockHandlers();
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [
        { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      ],
      effectiveScopes: {},
      connectionPresentations: {},
    });
  });

  // --------------------------------------------------------------------------
  // Common items (both surfaces)
  // --------------------------------------------------------------------------

  describe("common items", () => {
    it("always shows View Details", () => {
      renderWithContextMenu(createMockTask(), handlers);
      expect(screen.getByText("View Details")).toBeInTheDocument();
    });

    it("calls onViewDetails when clicked", () => {
      renderWithContextMenu(createMockTask(), handlers);
      fireEvent.click(screen.getByText("View Details"));
      expect(handlers.onViewDetails).toHaveBeenCalledTimes(1);
    });

    it("shows Edit for non-archived, non-system-controlled tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "backlog" }),
        handlers,
      );
      expect(screen.getByText("Edit")).toBeInTheDocument();
    });

    it("hides Edit for system-controlled tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "executing" }),
        handlers,
      );
      expect(screen.queryByText("Edit")).not.toBeInTheDocument();
    });

    it("hides Edit for archived tasks", () => {
      renderWithContextMenu(
        createMockTask({ archivedAt: new Date().toISOString() }),
        handlers,
      );
      expect(screen.queryByText("Edit")).not.toBeInTheDocument();
    });

    it("hides Edit when onEdit handler not provided", () => {
      handlers.onEdit = undefined;
      renderWithContextMenu(
        createMockTask({ internalStatus: "backlog" }),
        handlers,
      );
      expect(screen.queryByText("Edit")).not.toBeInTheDocument();
    });

    it("shows Start Ideation for backlog tasks when handler provided", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "backlog" }),
        handlers,
      );
      expect(screen.getByText("Start Ideation")).toBeInTheDocument();
    });

    it("hides Start Ideation for non-backlog tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "ready" }),
        handlers,
      );
      expect(screen.queryByText("Start Ideation")).not.toBeInTheDocument();
    });

    it("hides Start Ideation when handler not provided", () => {
      handlers.onStartIdeation = undefined;
      renderWithContextMenu(
        createMockTask({ internalStatus: "backlog" }),
        handlers,
      );
      expect(screen.queryByText("Start Ideation")).not.toBeInTheDocument();
    });
  });

  // --------------------------------------------------------------------------
  // Archive/Restore/Delete
  // --------------------------------------------------------------------------

  describe("archive/restore/delete", () => {
    it("disables archive and shows the gate reason without dispatching", async () => {
      setGateEnvironment(false);
      renderWithContextMenu(createMockTask(), handlers);
      const action = screen.getByTestId("archive-action");
      expect(action).toHaveAttribute("data-agent-gated", "true");
      // The reason renders in a Radix TooltipContent that mounts on focus.
      action.focus();
      await waitFor(() => {
        expect(
          screen.getByTestId("archive-gate-explanation"),
        ).toHaveTextContent(/agent control/i);
      });
      fireEvent.click(action);
      expect(handlers.onArchive).not.toHaveBeenCalled();
    });

    it("keeps archive live and dispatches when the gate is granted", async () => {
      setGateEnvironment(true);
      renderWithContextMenu(createMockTask(), handlers);
      fireEvent.click(screen.getByTestId("archive-action"));
      fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
      await waitFor(() => expect(handlers.onArchive).toHaveBeenCalledTimes(1));
    });

    it("disables restore and shows the gate reason without dispatching", async () => {
      setGateEnvironment(false);
      renderWithContextMenu(
        createMockTask({ archivedAt: new Date().toISOString() }),
        handlers,
      );
      const action = screen.getByTestId("restore-action");
      expect(action).toHaveAttribute("data-agent-gated", "true");
      // The reason renders in a Radix TooltipContent that mounts on focus.
      action.focus();
      await waitFor(() => {
        expect(
          screen.getByTestId("restore-gate-explanation"),
        ).toHaveTextContent(/agent control/i);
      });
      fireEvent.click(action);
      expect(handlers.onRestore).not.toHaveBeenCalled();
    });
    it("shows Archive for non-archived tasks", () => {
      renderWithContextMenu(createMockTask(), handlers);
      expect(screen.getByText("Archive")).toBeInTheDocument();
    });

    it("hides Archive for archived tasks", () => {
      renderWithContextMenu(
        createMockTask({ archivedAt: new Date().toISOString() }),
        handlers,
      );
      expect(screen.queryByText("Archive")).not.toBeInTheDocument();
    });

    it("shows Restore for archived tasks", () => {
      renderWithContextMenu(
        createMockTask({ archivedAt: new Date().toISOString() }),
        handlers,
      );
      expect(screen.getByText("Restore")).toBeInTheDocument();
      expect(screen.queryByText("Delete Permanently")).not.toBeInTheDocument();
    });
  });

  // --------------------------------------------------------------------------
  // Kanban surface — status-specific actions
  // --------------------------------------------------------------------------

  describe("kanban surface", () => {
    it.each([
      ["ready", "block", "onBlockWithReason"],
      ["executing", "pause", "onPause"],
    ] as const)(
      "gates %s task %s and does not dispatch",
      async (status, actionId, handlerKey) => {
        setGateEnvironment(false);
        renderWithContextMenu(
          createMockTask({ internalStatus: status }),
          handlers,
          "kanban",
        );
        const action = screen.getByTestId(`${actionId}-action`);
        expect(action).toHaveAttribute("data-agent-gated", "true");
        // The reason renders in a Radix TooltipContent that mounts on focus.
        action.focus();
        await waitFor(() => {
          expect(
            screen.getByTestId(`${actionId}-gate-explanation`),
          ).toHaveTextContent(/agent control/i);
        });
        fireEvent.click(action);
        expect(handlers[handlerKey]).not.toHaveBeenCalled();
      },
    );
    it("shows Cancel for backlog tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "backlog" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Cancel")).toBeInTheDocument();
    });

    it("shows Block and Cancel for ready tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "ready" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Block")).toBeInTheDocument();
      expect(screen.getByText("Cancel")).toBeInTheDocument();
    });

    it("shows Unblock and Cancel for blocked tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "blocked" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Unblock")).toBeInTheDocument();
      expect(screen.getByText("Cancel")).toBeInTheDocument();
    });

    it("shows Re-open for approved tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "approved" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Re-open")).toBeInTheDocument();
    });

    it("shows Retry for failed tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "failed" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Retry")).toBeInTheDocument();
    });

    it("moves failed task retry to ready", async () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "failed" }),
        handlers,
        "kanban",
      );
      fireEvent.click(screen.getByText("Retry"));
      await waitFor(() => {
        expect(screen.getByText("Retry this task?")).toBeInTheDocument();
      });
      fireEvent.click(screen.getByRole("button", { name: "Retry" }));
      await waitFor(() => {
        expect(handlers.onStatusChange).toHaveBeenCalledWith("ready");
      });
    });

    it("shows Re-open for cancelled tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "cancelled" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Re-open")).toBeInTheDocument();
    });

    it("shows Pause and Cancel for executing tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "executing" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Pause")).toBeInTheDocument();
      expect(screen.getByText("Cancel")).toBeInTheDocument();
      expect(screen.queryByText("Block")).not.toBeInTheDocument();
    });

    it("shows Pause and Cancel for re_executing tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "re_executing" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Pause")).toBeInTheDocument();
      expect(screen.getByText("Cancel")).toBeInTheDocument();
    });

    it("shows Resume and Cancel for paused tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "paused" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Resume")).toBeInTheDocument();
      expect(screen.getByText("Cancel")).toBeInTheDocument();
    });

    it("shows Start Execution for ready tasks in kanban", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "ready" }),
        handlers,
        "kanban",
      );
      expect(screen.getByText("Start Execution")).toBeInTheDocument();
    });
  });

  // --------------------------------------------------------------------------
  // Graph surface — status-specific actions
  // --------------------------------------------------------------------------

  describe("graph surface", () => {
    it("shows Start Execution and Block for ready tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "ready" }),
        handlers,
        "graph",
      );
      expect(screen.getByText("Start Execution")).toBeInTheDocument();
      expect(screen.getByText("Block")).toBeInTheDocument();
    });

    it("shows Unblock and View Blockers for blocked tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "blocked" }),
        handlers,
        "graph",
      );
      expect(screen.getByText("Unblock")).toBeInTheDocument();
      expect(screen.getByText("View Blockers")).toBeInTheDocument();
    });

    it("shows View Agent Chat for executing tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "executing" }),
        handlers,
        "graph",
      );
      expect(screen.getByText("View Agent Chat")).toBeInTheDocument();
    });

    it("shows View Work Summary for pending_review tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "pending_review" }),
        handlers,
        "graph",
      );
      expect(screen.getByText("View Work Summary")).toBeInTheDocument();
    });

    it("shows Approve and Request Changes for review_passed tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "review_passed" }),
        handlers,
        "graph",
      );
      expect(screen.getByText("Approve")).toBeInTheDocument();
      expect(screen.getByText("Request Changes")).toBeInTheDocument();
    });

    it("shows Approve, Reject, and Request Changes for escalated tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "escalated" }),
        handlers,
        "graph",
      );
      expect(screen.getByText("Approve")).toBeInTheDocument();
      expect(screen.getByText("Reject")).toBeInTheDocument();
      expect(screen.getByText("Request Changes")).toBeInTheDocument();
    });

    it("shows View Feedback for revision_needed tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "revision_needed" }),
        handlers,
        "graph",
      );
      expect(screen.getByText("View Feedback")).toBeInTheDocument();
    });

    it("shows View Conflicts and Mark Resolved for merge_conflict tasks", () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "merge_conflict" }),
        handlers,
        "graph",
      );
      expect(screen.getByText("View Conflicts")).toBeInTheDocument();
      expect(screen.getByText("Mark Resolved")).toBeInTheDocument();
    });
  });

  // --------------------------------------------------------------------------
  // Block dialog
  // --------------------------------------------------------------------------

  describe("block dialog", () => {
    it("opens BlockReasonDialog when Block action clicked", async () => {
      renderWithContextMenu(
        createMockTask({ internalStatus: "ready" }),
        handlers,
        "kanban",
      );
      fireEvent.click(screen.getByText("Block"));
      // Dialog is rendered by TaskContextMenuDialogs (outside ContextMenuContent)
      await waitFor(() => {
        expect(screen.getByTestId("block-reason-dialog")).toBeInTheDocument();
      });
    });
  });
});
