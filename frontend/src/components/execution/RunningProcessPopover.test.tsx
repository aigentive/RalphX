import { createElement, type ReactElement } from "react";
/**
 * RunningProcessPopover component tests
 */

import { beforeEach, describe, it, expect, vi } from "vitest";
import { render as rtlRender, screen, fireEvent } from "@testing-library/react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { RunningProcessPopover } from "./RunningProcessPopover";
import type {
  ExecutionCapacitySummary,
  ExecutionLaneUsage,
  RunningProcess,
  RunningIdeationSession,
  RunningWorkspaceSession,
} from "@/api/running-processes";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";

// Gated icon-only controls carry the app tooltip; `Tooltip` throws without a
// provider, which production gets from App.tsx.
function render(ui: ReactElement): ReturnType<typeof rtlRender> {
  return rtlRender(createElement(TooltipProvider, null, ui));
}

beforeEach(() => {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
    ],
    effectiveScopes: {},
    connectionPresentations: {},
  });
});

// Mock process data helper
function createMockProcess(
  overrides?: Partial<RunningProcess>,
): RunningProcess {
  return {
    taskId: "task-123",
    title: "Test Task",
    internalStatus: "executing",
    stepProgress: {
      taskId: "task-123",
      total: 7,
      completed: 2,
      inProgress: 1,
      pending: 4,
      skipped: 0,
      failed: 0,
      currentStep: {
        id: "step-3",
        taskId: "task-123",
        title: "Step 3",
        description: null,
        status: "in_progress",
        sortOrder: 2,
        dependsOn: null,
        createdBy: "user",
        completionNote: null,
        createdAt: "2026-02-11T12:00:00Z",
        updatedAt: "2026-02-11T12:00:00Z",
        startedAt: "2026-02-11T12:00:00Z",
        completedAt: null,
      },
      nextStep: null,
      percentComplete: 28.57,
    },
    elapsedSeconds: 134,
    triggerOrigin: "scheduler",
    taskBranch: "ralphx/app/task-123",
    ...overrides,
  };
}

// Mock process data helper
function createMockIdeationSession(
  overrides?: Partial<RunningIdeationSession>,
): RunningIdeationSession {
  return {
    sessionId: "session-1",
    title: "Test Ideation Session",
    elapsedSeconds: 60,
    isGenerating: true,
    ...overrides,
  };
}

function createMockWorkspaceSession(
  overrides?: Partial<RunningWorkspaceSession>,
): RunningWorkspaceSession {
  return {
    conversationId: "workspace-1",
    projectId: "project-1",
    automationId: null,
    automationRunId: null,
    title: "Workspace Agent",
    elapsedSeconds: 90,
    model: "gpt-5.5",
    ...overrides,
  };
}

const mockLanes: ExecutionLaneUsage[] = [
  {
    lane: "workspaces",
    active: 1,
    idle: 0,
    waiting: 0,
    max: 10,
    borrowed: 0,
    priorityRank: 1,
  },
  {
    lane: "tasks",
    active: 2,
    idle: 0,
    waiting: 1,
    max: 8,
    borrowed: 0,
    priorityRank: 2,
  },
  {
    lane: "ideation",
    active: 1,
    idle: 1,
    waiting: 2,
    max: 5,
    borrowed: 0,
    priorityRank: 3,
  },
];

const mockCapacity: ExecutionCapacitySummary = {
  totalActive: 4,
  globalMaxConcurrent: 20,
  borrowingEnabled: true,
  priority: ["workspaces", "tasks", "ideation"],
};

describe("RunningProcessPopover", () => {
  describe("basic rendering", () => {
    it("renders trigger element", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={false}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(screen.getByText("Trigger")).toBeInTheDocument();
    });

    it("renders popover content when open", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(screen.getByTestId("running-process-popover")).toBeInTheDocument();
    });

    it("does not render popover content when closed", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={false}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(
        screen.queryByTestId("running-process-popover"),
      ).not.toBeInTheDocument();
    });
  });

  describe("header", () => {
    it("displays correct title with process count", () => {
      const processes = [
        createMockProcess({ taskId: "task-1" }),
        createMockProcess({ taskId: "task-2" }),
      ];
      render(
        <RunningProcessPopover
          processes={processes}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(screen.getByText("Execution (2/3)")).toBeInTheDocument();
    });

    it("displays max concurrency in settings button", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={5}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(screen.getByText(/Max: 5/)).toBeInTheDocument();
    });

    it("calls onOpenSettings when settings button clicked", () => {
      const onOpenSettings = vi.fn();
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={onOpenSettings}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      fireEvent.click(screen.getByTestId("open-settings-button"));
      expect(onOpenSettings).toHaveBeenCalledOnce();
    });
  });

  describe("process list", () => {
    it("displays empty state when no processes", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(
        screen.getByText("No active execution processes"),
      ).toBeInTheDocument();
    });

    it("renders all processes as ProcessCard components", () => {
      const processes = [
        createMockProcess({ taskId: "task-1", title: "Task 1" }),
        createMockProcess({ taskId: "task-2", title: "Task 2" }),
        createMockProcess({ taskId: "task-3", title: "Task 3" }),
      ];
      render(
        <RunningProcessPopover
          processes={processes}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      expect(screen.getByTestId("process-card-task-1")).toBeInTheDocument();
      expect(screen.getByTestId("process-card-task-2")).toBeInTheDocument();
      expect(screen.getByTestId("process-card-task-3")).toBeInTheDocument();
    });

    it("passes onPauseProcess callback to ProcessCard", () => {
      const onPauseProcess = vi.fn();
      const processes = [createMockProcess({ taskId: "task-1" })];
      render(
        <RunningProcessPopover
          processes={processes}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={onPauseProcess}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      fireEvent.click(screen.getByTestId("pause-button-task-1"));
      expect(onPauseProcess).toHaveBeenCalledWith("task-1");
    });

    it("disables pause with its reason and never dispatches when gated", async () => {
      const onPauseProcess = vi.fn();
      useEnvironmentStore.setState({
        activeEnvironmentId: "remote",
        environments: [{ id: "remote", name: "Remote", kind: "remote" }],
        effectiveScopes: { remote: ["ui:read", "ui:operate"] },
        connectionPresentations: {
          remote: {
            presentation: "connected",
            blockedFailure: null,
            blockedMessage: null,
          },
        },
      });
      render(
        <RunningProcessPopover
          processes={[createMockProcess({ taskId: "task-1" })]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={onPauseProcess}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      const button = screen.getByTestId("pause-button-task-1");
      // Soft-disabled so the reason stays reachable; a real `disabled` would hide it.
      expect(button).toHaveAttribute("aria-disabled", "true");
      expect(button).not.toBeDisabled();
      button.focus();
      // Radix renders tooltip copy twice (visible + the live-region announcement).
      expect(
        (await screen.findAllByText(/agent control/i)).length,
      ).toBeGreaterThan(0);
      fireEvent.click(button);
      expect(onPauseProcess).not.toHaveBeenCalled();
    });

    it("passes onStopProcess callback to ProcessCard", () => {
      const onStopProcess = vi.fn();
      const processes = [createMockProcess({ taskId: "task-1" })];
      render(
        <RunningProcessPopover
          processes={processes}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={onStopProcess}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      fireEvent.click(screen.getByTestId("stop-button-task-1"));
      expect(onStopProcess).toHaveBeenCalledWith("task-1");
    });
  });

  describe("footer", () => {
    it("displays info text with max concurrent count", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={5}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(
        screen.getByText(/runs up to 5 tasks in parallel/),
      ).toBeInTheDocument();
    });

    it("calls onOpenSettings when footer link clicked", () => {
      const onOpenSettings = vi.fn();
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={onOpenSettings}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      // Find the footer button by text content "Open Settings"
      const footerButton = screen.getByText("Open Settings");
      fireEvent.click(footerButton);
      expect(onOpenSettings).toHaveBeenCalled();
    });
  });

  describe("open/close behavior", () => {
    it("calls onOpenChange when popover state changes", () => {
      const onOpenChange = vi.fn();
      const { rerender } = render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={false}
          onOpenChange={onOpenChange}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      // Simulate opening
      rerender(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={true}
          onOpenChange={onOpenChange}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      expect(screen.getByTestId("running-process-popover")).toBeInTheDocument();
    });
  });

  describe("tab switching", () => {
    it("renders both Execution and Ideation tab pills when showIdeation=true", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          ideationSessions={[]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          showIdeation={true}
          ideationMax={2}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(
        screen.getByRole("tab", { name: /Execution/ }),
      ).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: /Ideation/ })).toBeInTheDocument();
    });

    it("does not render tab bar when showIdeation=false", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          showIdeation={false}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    });

    it("renders running, workspace, tasks, and ideation tabs when lane usage is present", () => {
      render(
        <RunningProcessPopover
          processes={[
            createMockProcess({ taskId: "task-1" }),
            createMockProcess({ taskId: "task-2" }),
          ]}
          ideationSessions={[createMockIdeationSession()]}
          workspaceSessions={[createMockWorkspaceSession()]}
          lanes={mockLanes}
          capacity={mockCapacity}
          maxConcurrent={8}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          showIdeation={true}
          ideationMax={5}
          initialTab="running"
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      expect(screen.getByRole("tab", { name: /Running/ })).toBeInTheDocument();
      expect(
        screen.getByRole("tab", { name: /Workspaces/ }),
      ).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: /Tasks/ })).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: /Ideation/ })).toBeInTheDocument();
      expect(screen.getByTestId("capacity-lane-workspaces")).toHaveTextContent(
        "1/10",
      );
    });

    it("clicking Workspaces tab shows workspace sessions", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          workspaceSessions={[
            createMockWorkspaceSession({ title: "Main workspace" }),
          ]}
          lanes={mockLanes}
          capacity={mockCapacity}
          maxConcurrent={8}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          initialTab="running"
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      fireEvent.click(screen.getByRole("tab", { name: /Workspaces/ }));
      expect(screen.getByText("Main workspace")).toBeInTheDocument();
      expect(screen.getByText("gpt-5.5")).toBeInTheDocument();
    });

    it("clicking lane summaries in Running switches to the corresponding tab", () => {
      render(
        <RunningProcessPopover
          processes={[
            createMockProcess({ taskId: "task-lane", title: "Lane Task" }),
          ]}
          ideationSessions={[
            createMockIdeationSession({ title: "Lane Ideation" }),
          ]}
          workspaceSessions={[
            createMockWorkspaceSession({ title: "Lane Workspace" }),
          ]}
          lanes={mockLanes}
          capacity={mockCapacity}
          maxConcurrent={8}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          showIdeation={true}
          ideationMax={5}
          initialTab="running"
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      fireEvent.click(screen.getByTestId("capacity-lane-workspaces"));
      expect(screen.getByText("Lane Workspace")).toBeInTheDocument();

      fireEvent.click(screen.getByRole("tab", { name: /Running/ }));
      fireEvent.click(screen.getByTestId("capacity-lane-tasks"));
      expect(screen.getByTestId("process-card-task-lane")).toBeInTheDocument();

      fireEvent.click(screen.getByRole("tab", { name: /Running/ }));
      fireEvent.click(screen.getByTestId("capacity-lane-ideation"));
      expect(screen.getByText("Lane Ideation")).toBeInTheDocument();
    });

    it("clicking Ideation tab shows ideation content", () => {
      const session = createMockIdeationSession({
        title: "My Ideation Session",
      });
      render(
        <RunningProcessPopover
          processes={[]}
          ideationSessions={[session]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          showIdeation={true}
          ideationMax={2}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      fireEvent.click(screen.getByRole("tab", { name: /Ideation/ }));
      expect(screen.getByText("My Ideation Session")).toBeInTheDocument();
    });

    it("clicking Execution tab shows execution content", () => {
      const process = createMockProcess({
        taskId: "task-exec",
        title: "Exec Task",
      });
      const session = createMockIdeationSession({ title: "Hidden Ideation" });
      render(
        <RunningProcessPopover
          processes={[process]}
          ideationSessions={[session]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          showIdeation={true}
          ideationMax={2}
          initialTab="ideation"
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      // Currently on ideation tab — switch back to execution
      fireEvent.click(screen.getByRole("tab", { name: /Execution/ }));
      expect(screen.getByTestId("process-card-task-exec")).toBeInTheDocument();
    });

    it("initialTab='ideation' starts on ideation tab", () => {
      const session = createMockIdeationSession({
        title: "Initial Ideation Session",
      });
      render(
        <RunningProcessPopover
          processes={[]}
          ideationSessions={[session]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          showIdeation={true}
          ideationMax={2}
          initialTab="ideation"
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(screen.getByText("Initial Ideation Session")).toBeInTheDocument();
    });
  });

  describe("navigation callbacks", () => {
    it("clicking a process card closes the popover and emits an Agent task target", () => {
      const onOpenChange = vi.fn();
      const onNavigateToTask = vi.fn();
      const agentWorkspace = {
        conversationId: "conversation-1",
        projectId: "project-1",
        title: "Workspace Conversation",
      };
      const processes = [
        createMockProcess({ taskId: "task-nav-1", agentWorkspace }),
      ];
      render(
        <RunningProcessPopover
          processes={processes}
          maxConcurrent={3}
          open={true}
          onOpenChange={onOpenChange}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          onNavigateToTask={onNavigateToTask}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      fireEvent.click(screen.getByTestId("process-card-task-nav-1"));

      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(onNavigateToTask).toHaveBeenCalledWith({
        taskId: "task-nav-1",
        source: "running",
        projectId: "project-1",
        agentWorkspace,
      });
    });

    it("clicking a workspace row closes the popover and navigates to the agent conversation", () => {
      const onOpenChange = vi.fn();
      const onNavigateToWorkspace = vi.fn();
      const workspace = createMockWorkspaceSession({
        conversationId: "conversation-123",
        projectId: "project-456",
        title: "Clickable Workspace",
      });
      render(
        <RunningProcessPopover
          processes={[]}
          workspaceSessions={[workspace]}
          lanes={mockLanes}
          capacity={mockCapacity}
          maxConcurrent={8}
          open={true}
          onOpenChange={onOpenChange}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          onNavigateToWorkspace={onNavigateToWorkspace}
          initialTab="workspaces"
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      fireEvent.click(screen.getByTestId("workspace-card-conversation-123"));

      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(onNavigateToWorkspace).toHaveBeenCalledWith(
        "project-456",
        "conversation-123",
        workspace,
      );
    });

    it("passes automation run metadata when a running workspace row belongs to a run", () => {
      const onOpenChange = vi.fn();
      const onNavigateToWorkspace = vi.fn();
      const workspace = createMockWorkspaceSession({
        conversationId: "run-conversation-123",
        projectId: "project-456",
        automationId: "automation-789",
        automationRunId: "run-101",
        title: "Automation run",
      });
      render(
        <RunningProcessPopover
          processes={[]}
          workspaceSessions={[workspace]}
          lanes={mockLanes}
          capacity={mockCapacity}
          maxConcurrent={8}
          open={true}
          onOpenChange={onOpenChange}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          onNavigateToWorkspace={onNavigateToWorkspace}
          initialTab="workspaces"
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );

      fireEvent.click(
        screen.getByTestId("workspace-card-run-conversation-123"),
      );

      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(onNavigateToWorkspace).toHaveBeenCalledWith(
        "project-456",
        "run-conversation-123",
        workspace,
      );
    });
  });

  describe("empty states per tab", () => {
    it("shows 'No active execution processes' on execution tab when empty", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          showIdeation={true}
          ideationMax={2}
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(
        screen.getByText("No active execution processes"),
      ).toBeInTheDocument();
    });

    it("shows 'No active ideation sessions' on ideation tab when empty", () => {
      render(
        <RunningProcessPopover
          processes={[]}
          ideationSessions={[]}
          maxConcurrent={3}
          open={true}
          onOpenChange={vi.fn()}
          onPauseProcess={vi.fn()}
          onStopProcess={vi.fn()}
          onOpenSettings={vi.fn()}
          showIdeation={true}
          ideationMax={2}
          initialTab="ideation"
        >
          <button>Trigger</button>
        </RunningProcessPopover>,
      );
      expect(
        screen.getByText("No active ideation sessions"),
      ).toBeInTheDocument();
    });
  });
});
