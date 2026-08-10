/**
 * ExecutionControlBar component tests
 */

import { beforeEach, describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import type { ComponentProps, ReactNode } from "react";
import { ExecutionControlBar } from "./ExecutionControlBar";
import { useAgentTerminalStore } from "@/components/agents/agentTerminalStore";
import type { MergePipelineTask } from "@/api/merge-pipeline";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

vi.mock("./RunningProcessPopover", () => ({
  RunningProcessPopover: ({
    children,
    initialTab,
    open,
    showIdeation,
    ideationMax,
  }: {
    children: ReactNode;
    initialTab?: string;
    open?: boolean;
    showIdeation?: boolean;
    ideationMax?: number;
    [key: string]: unknown;
  }) => (
    <div
      data-testid="mock-running-popover"
      data-initial-tab={initialTab}
      data-open={String(open ?? false)}
      data-show-ideation={String(showIdeation ?? false)}
      data-ideation-max={ideationMax}
    >
      {children}
    </div>
  ),
}));

vi.mock("./QueuedTasksPopover", () => ({
  QueuedTasksPopover: ({
    children,
    open,
  }: {
    children: ReactNode;
    open?: boolean;
    [key: string]: unknown;
  }) => (
    <div data-testid="mock-queued-popover" data-open={String(open ?? false)}>
      {children}
    </div>
  ),
}));

vi.mock("./MergePipelinePopover", () => ({
  MergePipelinePopover: ({
    children,
    open,
  }: {
    children: ReactNode;
    open?: boolean;
    [key: string]: unknown;
  }) => (
    <div data-testid="mock-merge-popover" data-open={String(open ?? false)}>
      {children}
    </div>
  ),
}));

// Helper: renders ExecutionControlBar with all required props, accepting overrides
function renderBar(
  overrides: Partial<ComponentProps<typeof ExecutionControlBar>> = {}
) {
  return render(
    <ExecutionControlBar
      projectId="proj-1"
      runningCount={0}
      maxConcurrent={10}
      queuedCount={0}
      mergingCount={0}
      hasAttentionMerges={false}
      mergePipelineData={null}
      isPaused={false}
      onPauseToggle={vi.fn()}
      onStop={vi.fn()}
      {...overrides}
    />
  );
}

const makeMergeTask = (
  overrides: Partial<MergePipelineTask> = {}
): MergePipelineTask => ({
  taskId: "merge-task-1",
  title: "Merge task",
  internalStatus: "merging",
  sourceBranch: "ralphx/app/task-1",
  targetBranch: "main",
  isDeferred: false,
  isMainMergeDeferred: false,
  blockingBranch: null,
  conflictFiles: null,
  errorContext: null,
  ...overrides,
});

describe("ExecutionControlBar", () => {
  beforeEach(() => {
    useUiStore.setState({
      executionBarOpenPopover: null,
      executionBarRunningTab: "execution",
    });
    useAgentTerminalStore.setState({
      openByConversationId: {},
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      statusByConversationId: {},
      metadataByConversationId: {},
      placement: "auto",
      draggingConversationId: null,
      dragOverDock: null,
    });
    useProjectStore.setState({
      projects: {},
      activeProjectId: null,
    });
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    });
  });

  it("renders indeterminate counts instead of healthy zeroes when status is unknown", () => {
    renderBar({ statusKnown: false });

    expect(screen.getByTestId("running-count")).toHaveTextContent("—");
    expect(screen.getByTestId("running-count")).not.toHaveTextContent("0/10");
    expect(screen.getByTestId("execution-control-bar")).toHaveAttribute(
      "aria-label",
      "Execution controls",
    );
  });

  describe("basic rendering", () => {
    it("renders with data-testid", () => {
      renderBar({ runningCount: 1, queuedCount: 3 });
      expect(screen.getByTestId("execution-control-bar")).toBeInTheDocument();
    });

    it("displays running tasks count", () => {
      renderBar({ runningCount: 1, queuedCount: 3 });
      expect(screen.getByTestId("running-count")).toHaveTextContent(/(Running|R): 1\/10/);
    });

    it("displays queued tasks count", () => {
      renderBar({ queuedCount: 5 });
      expect(screen.getByTestId("queued-count")).toHaveTextContent(/(Queued|Q): 5/);
    });

    it("shows escalated merge attention instead of labeling it as merging", () => {
      renderBar({
        mergingCount: 0,
        mergeAttentionCount: 1,
        hasAttentionMerges: true,
        mergePipelineData: {
          active: [],
          waiting: [],
          needsAttention: [
            makeMergeTask({
              internalStatus: "merge_incomplete",
              errorContext: "Repository hook environment failed",
            }),
          ],
        },
      });

      expect(screen.getByTestId("merging-count")).not.toHaveTextContent(/Merging:\s*1/);
      expect(screen.getByTestId("merge-attention-count")).toHaveTextContent(/(Escalated|E):\s*1/);
    });

    it("separates active merge work from escalated merge attention", () => {
      renderBar({
        mergingCount: 2,
        mergeAttentionCount: 1,
        hasAttentionMerges: true,
        mergePipelineData: {
          active: [makeMergeTask({ taskId: "active-1", internalStatus: "merging" })],
          waiting: [makeMergeTask({ taskId: "waiting-1", internalStatus: "pending_merge" })],
          needsAttention: [makeMergeTask({ taskId: "attention-1", internalStatus: "merge_incomplete" })],
        },
      });

      expect(screen.getByTestId("merging-count")).toHaveTextContent(/(Merge|M):\s*2/);
      expect(screen.getByTestId("merge-attention-count")).toHaveTextContent(/(Escalated|E):\s*1/);
    });

    it("includes queued agent messages in the status region label", () => {
      renderBar({ runningCount: 2, queuedCount: 5, queuedMessageCount: 3 });
      expect(screen.getByLabelText(/3 queued messages/)).toBeInTheDocument();
    });

    it("shows an inline queued-message warning badge when pressure exists", () => {
      renderBar({ runningCount: 1, queuedCount: 2, queuedMessageCount: 4 });
      expect(screen.getByTestId("queued-message-count")).toHaveTextContent(/Msg[s]?:\s*4/);
    });

    it("hides the queued-message warning badge when no messages are held", () => {
      renderBar({ runningCount: 1, queuedCount: 2, queuedMessageCount: 0 });
      expect(screen.queryByTestId("queued-message-count")).not.toBeInTheDocument();
    });
  });

  describe("pause button", () => {
    it("renders pause button when not paused", () => {
      renderBar({ runningCount: 1, queuedCount: 3 });
      expect(screen.getByTestId("pause-toggle-button")).toHaveTextContent("Pause");
    });

    it("renders resume button when paused", () => {
      renderBar({ queuedCount: 3, isPaused: true, haltMode: "paused" });
      expect(screen.getByTestId("pause-toggle-button")).toHaveTextContent("Resume");
    });

    it("renders start button after stop", () => {
      renderBar({ isPaused: true, haltMode: "stopped" });
      const pauseBtn = screen.getByTestId("pause-toggle-button");
      expect(pauseBtn).toHaveTextContent("Start");
      expect(pauseBtn).not.toBeDisabled();
    });

    it("calls onPauseToggle when clicked", () => {
      const onPauseToggle = vi.fn();
      renderBar({ runningCount: 1, queuedCount: 3, onPauseToggle });
      fireEvent.click(screen.getByTestId("pause-toggle-button"));
      expect(onPauseToggle).toHaveBeenCalledOnce();
    });

    it("disables pause button when isLoading", () => {
      renderBar({ runningCount: 1, queuedCount: 3, isLoading: true });
      expect(screen.getByTestId("pause-toggle-button")).toBeDisabled();
    });

    it("keeps remote Start/Resume reachable and omits host-only copy", () => {
      const onPauseToggle = vi.fn();
      useEnvironmentStore.setState({
        activeEnvironmentId: "remote-1",
        environments: [{ id: "remote-1", name: "Studio", kind: "remote" }],
        effectiveScopes: { "remote-1": ["ui:read", "ui:operate", "ui:agent"] },
        connectionPresentations: {
          "remote-1": {
            presentation: "connected",
            blockedFailure: null,
            blockedMessage: null,
          },
        },
      });
      const { rerender } = renderBar({ isPaused: true, haltMode: "paused", onPauseToggle });
      expect(screen.getByTestId("pause-toggle-button")).toBeEnabled();
      fireEvent.click(screen.getByTestId("pause-toggle-button"));
      expect(onPauseToggle).toHaveBeenCalledOnce();
      expect(screen.queryByText(/runs only on the host/i)).not.toBeInTheDocument();

      rerender(<ExecutionControlBar projectId="proj-1" runningCount={1} maxConcurrent={10} queuedCount={0} mergingCount={0} hasAttentionMerges={false} mergePipelineData={null} isPaused={false} onPauseToggle={onPauseToggle} onStop={vi.fn()} />);
      fireEvent.click(screen.getByTestId("pause-toggle-button"));
      expect(onPauseToggle).toHaveBeenCalledTimes(2);
    });
  });

  describe("stop button", () => {
    it("renders stop button", () => {
      renderBar({ runningCount: 1, queuedCount: 3 });
      expect(screen.getByTestId("stop-button")).toHaveTextContent("Stop");
    });

    it("calls onStop when clicked", () => {
      const onStop = vi.fn();
      renderBar({ runningCount: 1, queuedCount: 3, onStop });
      fireEvent.click(screen.getByTestId("stop-button"));
      expect(onStop).toHaveBeenCalledOnce();
    });

    it("disables stop button when no running tasks", () => {
      renderBar();
      expect(screen.getByTestId("stop-button")).toBeDisabled();
    });

    it("uses stopped aria label after a global stop", () => {
      renderBar({ isPaused: true, haltMode: "stopped" });
      expect(screen.getByTestId("stop-button")).toHaveAttribute(
        "aria-label",
        "Execution already stopped"
      );
    });

    it("enables stop button when there are running tasks", () => {
      renderBar({ runningCount: 1 });
      expect(screen.getByTestId("stop-button")).not.toBeDisabled();
    });

    it("disables stop button when isLoading", () => {
      renderBar({ runningCount: 1, queuedCount: 3, isLoading: true });
      expect(screen.getByTestId("stop-button")).toBeDisabled();
    });
  });

  describe("data attributes", () => {
    it("sets data-paused attribute", () => {
      renderBar({ isPaused: true });
      expect(screen.getByTestId("execution-control-bar")).toHaveAttribute("data-paused", "true");
    });

    it("sets data-running attribute", () => {
      renderBar({ runningCount: 2 });
      expect(screen.getByTestId("execution-control-bar")).toHaveAttribute("data-running", "2");
    });

    it("sets data-loading attribute when loading", () => {
      renderBar({ isLoading: true });
      expect(screen.getByTestId("execution-control-bar")).toHaveAttribute("data-loading", "true");
    });

    it("sets data-status attribute", () => {
      const { rerender } = renderBar({ runningCount: 1 });
      expect(screen.getByTestId("execution-control-bar")).toHaveAttribute("data-status", "running");

      rerender(
        <ExecutionControlBar
          projectId="proj-1"
          runningCount={0}
          maxConcurrent={10}
          queuedCount={0}
          mergingCount={0}
          hasAttentionMerges={false}
          mergePipelineData={null}
          isPaused={true}
          onPauseToggle={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByTestId("execution-control-bar")).toHaveAttribute("data-status", "paused");

      rerender(
        <ExecutionControlBar
          projectId="proj-1"
          runningCount={0}
          maxConcurrent={10}
          queuedCount={0}
          mergingCount={0}
          hasAttentionMerges={false}
          mergePipelineData={null}
          isPaused={false}
          onPauseToggle={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByTestId("execution-control-bar")).toHaveAttribute("data-status", "idle");
    });
  });

  describe("styling", () => {
    it("applies flat v29a status bar background style", () => {
      renderBar();
      const bar = screen.getByTestId("execution-control-bar");
      expect(bar.getAttribute("style")).toContain("background-color: transparent");
    });

    it("keeps the outer status bar border on the v29a kanban line token", () => {
      renderBar();
      const shell = screen.getByTestId("execution-control-shell");
      expect(shell).toHaveStyle({
        borderTopColor: "var(--kanban-toolbar-border, #2E2E36)",
        borderTopStyle: "solid",
        borderTopWidth: "1px",
      });
    });

    it("removes inner floating card border styling", () => {
      renderBar();
      const bar = screen.getByTestId("execution-control-bar");
      expect(bar.style.borderStyle).toBe("none");
    });

    it("does not apply elevation shadow", () => {
      renderBar();
      const bar = screen.getByTestId("execution-control-bar");
      expect(bar.style.boxShadow).toBe("none");
    });
  });

  describe("status indicator colors", () => {
    it("shows success color when running tasks exist", () => {
      renderBar({ runningCount: 1 });
      expect(screen.getByTestId("status-indicator")).toHaveStyle({ backgroundColor: "var(--accent-primary)" });
    });

    it("shows warning color when paused", () => {
      renderBar({ queuedCount: 3, isPaused: true });
      expect(screen.getByTestId("status-indicator")).toHaveStyle({ backgroundColor: "var(--status-warning)" });
    });

    it("shows stopped color when execution is globally stopped", () => {
      renderBar({ isPaused: true, haltMode: "stopped" });
      expect(screen.getByTestId("status-indicator")).toHaveStyle({ backgroundColor: "var(--status-error)" });
    });

    it("shows muted color when idle with no queued", () => {
      renderBar();
      expect(screen.getByTestId("status-indicator")).toHaveStyle({ backgroundColor: "var(--text-muted)" });
    });

    it("has pulsing animation class when running", () => {
      renderBar({ runningCount: 1 });
      expect(screen.getByTestId("status-indicator")).toHaveClass("status-indicator-running");
    });

    it("does not have pulsing animation when paused", () => {
      renderBar({ isPaused: true });
      expect(screen.getByTestId("status-indicator")).not.toHaveClass("status-indicator-running");
    });
  });

  describe("pause/resume button icons", () => {
    it("shows Pause icon when not paused", () => {
      renderBar({ runningCount: 1 });
      const btn = screen.getByTestId("pause-toggle-button");
      expect(btn.querySelector("svg")).toBeInTheDocument();
    });

    it("shows Play icon when paused", () => {
      renderBar({ isPaused: true });
      const btn = screen.getByTestId("pause-toggle-button");
      expect(btn.querySelector("svg")).toBeInTheDocument();
    });

    it("shows Loader2 spinner when loading", () => {
      renderBar({ runningCount: 1, isLoading: true });
      const btn = screen.getByTestId("pause-toggle-button");
      const svg = btn.querySelector("svg");
      expect(svg).toBeInTheDocument();
      expect(svg).toHaveClass("animate-spin");
    });
  });

  describe("stop button styling", () => {
    it("has error styling when can stop", () => {
      renderBar({ runningCount: 1 });
      const stopBtn = screen.getByTestId("stop-button");
      expect(stopBtn).toHaveStyle({ backgroundColor: "var(--bg-elevated)" });
      expect(stopBtn.getAttribute("style")).toContain("border-color: var(--border-default)");
      expect(stopBtn).toHaveStyle({ color: "var(--status-error)" });
      expect(stopBtn).toHaveStyle({ opacity: "1" });
    });

    it("has muted styling when disabled", () => {
      renderBar();
      const stopBtn = screen.getByTestId("stop-button");
      expect(stopBtn).toHaveStyle({ backgroundColor: "var(--bg-elevated)" });
      expect(stopBtn).toHaveStyle({ color: "var(--text-muted)" });
      expect(stopBtn).toHaveStyle({ opacity: "0.55" });
    });

    it("has Square icon", () => {
      renderBar({ runningCount: 1 });
      const stopBtn = screen.getByTestId("stop-button");
      const svg = stopBtn.querySelector("svg");
      expect(svg).toBeInTheDocument();
      expect(svg).toHaveClass("fill-current");
    });
  });

  describe("pause button styling", () => {
    it("has accent styling when paused", () => {
      renderBar({ queuedCount: 3, isPaused: true });
      const pauseBtn = screen.getByTestId("pause-toggle-button");
      expect(pauseBtn).toHaveStyle({ backgroundColor: "var(--bg-elevated)" });
      expect(pauseBtn.getAttribute("style")).toContain("border-color: var(--border-default)");
      expect(pauseBtn).toHaveStyle({ color: "var(--status-warning)" });
    });

    it("has default styling when not paused", () => {
      renderBar({ runningCount: 1 });
      expect(screen.getByTestId("pause-toggle-button")).toHaveStyle({ color: "var(--text-primary)" });
    });
  });

  describe("current task display", () => {
    it("shows current task name when running", () => {
      renderBar({ runningCount: 1, currentTaskName: "Implementing auth feature" });
      expect(screen.getByTestId("current-task")).toBeInTheDocument();
      expect(screen.getByTestId("current-task")).toHaveTextContent("Implementing auth feature");
    });

    it("does not show current task when paused", () => {
      renderBar({ queuedCount: 3, isPaused: true, currentTaskName: "Implementing auth feature" });
      expect(screen.queryByTestId("current-task")).not.toBeInTheDocument();
    });

    it("does not show current task when no tasks running", () => {
      renderBar({ currentTaskName: "Implementing auth feature" });
      expect(screen.queryByTestId("current-task")).not.toBeInTheDocument();
    });

    it("does not show current task when no task name provided", () => {
      renderBar({ runningCount: 1 });
      expect(screen.queryByTestId("current-task")).not.toBeInTheDocument();
    });

    it("has spinner icon with current task", () => {
      renderBar({ runningCount: 1, currentTaskName: "Building components" });
      const taskDisplay = screen.getByTestId("current-task");
      const svg = taskDisplay.querySelector("svg");
      expect(svg).toBeInTheDocument();
      expect(svg).toHaveClass("animate-spin");
    });

    it("has slide-in animation class", () => {
      renderBar({ runningCount: 1, currentTaskName: "Building components" });
      expect(screen.getByTestId("current-task")).toHaveClass("task-name-enter");
    });
  });

  describe("accessibility", () => {
    it("has role region", () => {
      renderBar();
      expect(screen.getByTestId("execution-control-bar")).toHaveAttribute("role", "region");
    });

    it("has aria-live for status updates", () => {
      renderBar();
      expect(screen.getByTestId("execution-control-bar")).toHaveAttribute("aria-live", "polite");
    });

    it("pause button has aria-label", () => {
      renderBar({ runningCount: 1 });
      expect(screen.getByTestId("pause-toggle-button")).toHaveAttribute("aria-label", "Pause execution");
    });

    it("pause button has aria-pressed when paused", () => {
      renderBar({ isPaused: true });
      expect(screen.getByTestId("pause-toggle-button")).toHaveAttribute("aria-pressed", "true");
    });

    it("stop button has aria-label", () => {
      renderBar({ runningCount: 1 });
      expect(screen.getByTestId("stop-button")).toHaveAttribute("aria-label", "Stop all running tasks");
    });
  });

  describe("running lane shortcuts", () => {
    it("keeps lane details inside the running popover instead of rendering bar shortcuts", () => {
      renderBar({
        runningCount: 2,
        maxConcurrent: 8,
        workspaceSessions: [
          {
            conversationId: "conversation-1",
            projectId: "project-1",
            automationId: null,
            automationRunId: null,
            title: "Workspace",
            elapsedSeconds: 30,
            model: "gpt-5.5",
          },
        ],
        lanes: [
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
            waiting: 0,
            max: 8,
            borrowed: 0,
            priorityRank: 2,
          },
        ],
        capacity: {
          totalActive: 3,
          globalMaxConcurrent: 20,
          borrowingEnabled: false,
          priority: ["workspaces", "tasks", "ideation"],
        },
      });

      expect(screen.getByTestId("running-count")).toHaveTextContent(/3\/20/);
      expect(screen.queryByTestId("workspace-count")).not.toBeInTheDocument();
      expect(screen.queryByTestId("task-lane-count")).not.toBeInTheDocument();
      expect(screen.getByTestId("mock-running-popover")).toHaveAttribute(
        "data-show-ideation",
        "false"
      );
    });

    it("does not render the ideation shortcut while still enabling the popover ideation tab", () => {
      renderBar({ ideationActive: 1, ideationMax: 2, ideationWaiting: 3 });

      expect(screen.queryByTestId("ideation-count")).not.toBeInTheDocument();
      expect(screen.queryByTestId("ideation-waiting-badge")).not.toBeInTheDocument();
      expect(screen.getByTestId("mock-running-popover")).toHaveAttribute(
        "data-show-ideation",
        "true"
      );
    });
  });

  describe("agent terminals indicator", () => {
    it("hides terminal count when no agent terminals are open", () => {
      renderBar();

      expect(screen.queryByTestId("terminals-count")).not.toBeInTheDocument();
    });

    it("hides explicitly closed terminal sessions even if the drawer state is expanded", () => {
      useAgentTerminalStore.setState({
        openByConversationId: {
          "conversation-1": true,
        },
        statusByConversationId: {
          "conversation-1": "closed",
        },
        metadataByConversationId: {
          "conversation-1": {
            conversationId: "conversation-1",
            projectId: "project-1",
            title: "Closed terminal",
            branchName: "ralphx/app/closed",
            worktreePath: "/tmp/closed",
            updatedAt: "2026-06-25T00:00:00Z",
          },
        },
      });

      renderBar();

      expect(screen.queryByTestId("terminals-count")).not.toBeInTheDocument();
    });

    it("keeps collapsed running terminal sessions visible", () => {
      useProjectStore.setState({
        projects: {
          "project-1": {
            id: "project-1",
            name: "Alpha",
            workingDirectory: "/tmp/alpha",
            gitMode: "worktree",
            baseBranch: null,
            worktreeParentDirectory: null,
            useFeatureBranches: true,
            mergeValidationMode: "block",
            detectedAnalysis: null,
            customAnalysis: null,
            analyzedAt: null,
            githubPrEnabled: false,
            createdAt: "2026-06-25T00:00:00Z",
            updatedAt: "2026-06-25T00:00:00Z",
          },
        },
      });
      useAgentTerminalStore.setState({
        openByConversationId: {
          "conversation-1": false,
          "conversation-2": false,
        },
        statusByConversationId: {
          "conversation-1": "running",
        },
        metadataByConversationId: {
          "conversation-1": {
            conversationId: "conversation-1",
            projectId: "project-1",
            title: "Implement terminal UX",
            branchName: "ralphx/app/terminal-ux",
            worktreePath: "/Users/example/ralphx-worktrees/terminal-ux",
            updatedAt: "2026-06-25T00:00:00Z",
          },
          "conversation-2": {
            conversationId: "conversation-2",
            projectId: "project-1",
            title: "Closed terminal",
            branchName: "ralphx/app/closed",
            worktreePath: "/tmp/closed",
            updatedAt: "2026-06-25T00:00:00Z",
          },
        },
      });

      renderBar();

      expect(screen.getByTestId("terminals-count")).toHaveTextContent(/1/);
      fireEvent.click(screen.getByTestId("terminals-count"));
      expect(screen.getByTestId("terminals-popover")).toHaveTextContent("Terminals (1)");
      expect(screen.getByTestId("terminal-session-conversation-1")).toHaveTextContent(
        "Implement terminal UX"
      );
      expect(screen.getByTestId("terminal-session-conversation-1")).toHaveTextContent(
        "Alpha"
      );
    });

    it("navigates to the owning agent conversation when a terminal session is clicked", () => {
      const onNavigateToWorkspace = vi.fn();
      useProjectStore.setState({
        projects: {
          "project-1": {
            id: "project-1",
            name: "Alpha",
            workingDirectory: "/tmp/alpha",
            gitMode: "worktree",
            baseBranch: null,
            worktreeParentDirectory: null,
            useFeatureBranches: true,
            mergeValidationMode: "block",
            detectedAnalysis: null,
            customAnalysis: null,
            analyzedAt: null,
            githubPrEnabled: false,
            createdAt: "2026-06-25T00:00:00Z",
            updatedAt: "2026-06-25T00:00:00Z",
          },
        },
      });
      useAgentTerminalStore.setState({
        openByConversationId: {
          "conversation-1": true,
        },
        statusByConversationId: {
          "conversation-1": "running",
        },
        metadataByConversationId: {
          "conversation-1": {
            conversationId: "conversation-1",
            projectId: "project-1",
            title: "Implement terminal UX",
            branchName: "ralphx/app/terminal-ux",
            worktreePath: "/tmp/alpha/terminal-ux",
            updatedAt: "2026-06-25T00:00:00Z",
          },
        },
      });

      renderBar({ onNavigateToWorkspace });

      fireEvent.click(screen.getByTestId("terminals-count"));
      fireEvent.click(screen.getByTestId("terminal-session-conversation-1"));

      expect(onNavigateToWorkspace).toHaveBeenCalledWith(
        "project-1",
        "conversation-1"
      );
      expect(useUiStore.getState().executionBarOpenPopover).toBeNull();
    });

    it("places terminal count after escalated merge status", () => {
      useProjectStore.setState({
        projects: {
          "project-1": {
            id: "project-1",
            name: "Alpha",
            workingDirectory: "/tmp/alpha",
            gitMode: "worktree",
            baseBranch: null,
            worktreeParentDirectory: null,
            useFeatureBranches: true,
            mergeValidationMode: "block",
            detectedAnalysis: null,
            customAnalysis: null,
            analyzedAt: null,
            githubPrEnabled: false,
            createdAt: "2026-06-25T00:00:00Z",
            updatedAt: "2026-06-25T00:00:00Z",
          },
        },
      });
      useAgentTerminalStore.setState({
        openByConversationId: {
          "conversation-1": true,
        },
        statusByConversationId: {
          "conversation-1": "running",
        },
        metadataByConversationId: {
          "conversation-1": {
            conversationId: "conversation-1",
            projectId: "project-1",
            title: "Implement terminal UX",
            branchName: "ralphx/app/terminal-ux",
            worktreePath: "/tmp/alpha/terminal-ux",
            updatedAt: "2026-06-25T00:00:00Z",
          },
        },
      });

      renderBar({
        mergingCount: 0,
        mergeAttentionCount: 1,
        hasAttentionMerges: true,
        mergePipelineData: {
          active: [],
          waiting: [],
          needsAttention: [
            makeMergeTask({
              internalStatus: "merge_incomplete",
              errorContext: "Repository hook environment failed",
            }),
          ],
        },
      });

      const attention = screen.getByTestId("merge-attention-count");
      const terminals = screen.getByTestId("terminals-count");
      expect(
        attention.compareDocumentPosition(terminals) &
          Node.DOCUMENT_POSITION_FOLLOWING
      ).toBeTruthy();
    });
  });

  describe("tab selection", () => {
    it("clicking running-count button passes initialTab='execution' to RunningProcessPopover", () => {
      renderBar({ runningCount: 2, ideationMax: 2 });
      fireEvent.click(screen.getByTestId("running-count"));
      const popover = screen.getByTestId("mock-running-popover");
      expect(popover).toHaveAttribute("data-initial-tab", "execution");
      expect(popover).toHaveAttribute("data-open", "true");
    });

    it("clicking running-count button opens the lane overview when lane data exists", () => {
      renderBar({
        lanes: [
          {
            lane: "workspaces",
            active: 1,
            idle: 0,
            waiting: 0,
            max: 10,
            borrowed: 0,
            priorityRank: 1,
          },
        ],
        capacity: {
          totalActive: 1,
          globalMaxConcurrent: 20,
          borrowingEnabled: false,
          priority: ["workspaces", "tasks", "ideation"],
        },
      });
      fireEvent.click(screen.getByTestId("running-count"));
      const popover = screen.getByTestId("mock-running-popover");
      expect(popover).toHaveAttribute("data-initial-tab", "running");
      expect(popover).toHaveAttribute("data-open", "true");
    });

    it("preserves the running popover open state across footer remounts", () => {
      const lanes = [
        {
          lane: "workspaces" as const,
          active: 1,
          idle: 0,
          waiting: 0,
          max: 10,
          borrowed: 0,
          priorityRank: 1,
        },
        {
          lane: "tasks" as const,
          active: 2,
          idle: 0,
          waiting: 0,
          max: 8,
          borrowed: 0,
          priorityRank: 2,
        },
      ];
      const capacity = {
        totalActive: 3,
        globalMaxConcurrent: 20,
        borrowingEnabled: false,
        priority: ["workspaces", "tasks", "ideation"] as const,
      };
      const firstRender = renderBar({ runningCount: 2, lanes, capacity });

      fireEvent.click(screen.getByTestId("running-count"));
      expect(screen.getByTestId("mock-running-popover")).toHaveAttribute("data-open", "true");
      expect(screen.getByTestId("mock-running-popover")).toHaveAttribute("data-initial-tab", "running");

      firstRender.unmount();
      renderBar({ runningCount: 2, lanes, capacity });

      expect(screen.getByTestId("mock-running-popover")).toHaveAttribute("data-open", "true");
      expect(screen.getByTestId("mock-running-popover")).toHaveAttribute("data-initial-tab", "running");
    });

    it("passes preserved queued popover state into the queued popover", () => {
      useUiStore.getState().setExecutionBarOpenPopover("queued");

      renderBar({ queuedCount: 3 });

      expect(screen.getByTestId("mock-queued-popover")).toHaveAttribute("data-open", "true");
    });

    it("RunningProcessPopover receives showIdeation=true when ideationMax > 0", () => {
      renderBar({ ideationMax: 3 });
      expect(screen.getByTestId("mock-running-popover")).toHaveAttribute("data-show-ideation", "true");
    });

    it("RunningProcessPopover receives showIdeation=false when ideationMax is 0", () => {
      renderBar();
      expect(screen.getByTestId("mock-running-popover")).toHaveAttribute("data-show-ideation", "false");
    });
  });

  describe("responsive breakpoints", () => {
    it("renders the wide layout when window width > 1200px", () => {
      const original = window.innerWidth;
      Object.defineProperty(window, "innerWidth", { configurable: true, value: 1400 });
      try {
        renderBar({ runningCount: 2 });
        expect(screen.getByTestId("execution-control-bar")).toBeInTheDocument();
      } finally {
        Object.defineProperty(window, "innerWidth", { configurable: true, value: original });
      }
    });

    it("renders the narrow layout when window width < 800px", () => {
      const original = window.innerWidth;
      Object.defineProperty(window, "innerWidth", { configurable: true, value: 600 });
      try {
        renderBar({ runningCount: 1 });
        expect(screen.getByTestId("execution-control-bar")).toBeInTheDocument();
      } finally {
        Object.defineProperty(window, "innerWidth", { configurable: true, value: original });
      }
    });
  });
});
