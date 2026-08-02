/**
 * MergingTaskDetail component tests
 *
 * Covers: progress rendering with mock merge_progress events,
 * reload-style remount recovery, fallback when events are missing/delayed,
 * validation progress display, and Stop Merge action button.
 */

import React from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { MergingTaskDetail } from "./MergingTaskDetail";
import { MergingTaskDetail as AgentsMergingTaskDetail } from "@/components/agents/task-details/detail-views/MergingTaskDetail";
import { setDetailViewEnvironment } from "@/components/agents/task-details/detail-views/agent-gate.test-utils";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { ArtifactResponse } from "@/api/artifacts";
import type { Task } from "@/types/task";
import type { MergeProgressEvent } from "@/types/events";
import type { PlanBranch } from "@/api/plan-branch.types";
import type { ReviewNoteResponse } from "@/lib/tauri";

const mockPlanBranchState = vi.hoisted((): { current: PlanBranch | null } => ({
  current: null,
}));

const mockPlanArtifactState = vi.hoisted((): { current: ArtifactResponse | null } => ({
  current: {
    id: "artifact-123",
    name: "Plan: Fix graph crash",
    artifact_type: "specification",
    content_type: "inline",
    content:
      "# Fix graph crash when no active plan selected\n\nGuard graph rendering when no plan is selected.",
    created_at: "2026-01-28T12:00:00+00:00",
    created_by: "test",
    version: 3,
    bucket_id: null,
    task_id: null,
    process_id: null,
    derived_from: [],
  },
}));

const mockGitDiffState = vi.hoisted(() => ({
  commits: [] as Array<{
    sha: string;
    shortSha: string;
    message: string;
    author: string;
    date: Date;
  }>,
}));

const mockPlanReviewState = vi.hoisted(() => ({
  tasks: [] as Task[],
  historiesByTaskId: new Map<string, ReviewNoteResponse[]>(),
}));

const mockConfirmation = {
  confirm: vi.fn(async () => true),
  confirmationDialogProps: {},
  ConfirmationDialog: () => null,
};

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: vi.fn(() => mockConfirmation),
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      stop: vi.fn(async () => ({})),
      list: vi.fn(async () => ({
        tasks: mockPlanReviewState.tasks,
        total: mockPlanReviewState.tasks.length,
        hasMore: false,
        offset: 0,
      })),
    },
    reviews: {
      getTaskStateHistory: vi.fn(async (taskId: string) =>
        mockPlanReviewState.historiesByTaskId.get(taskId) ?? []
      ),
    },
    artifacts: {
      getArtifact: vi.fn(async () => mockPlanArtifactState.current),
    },
  },
}));

vi.mock("@/hooks/usePlanBranchForTask", () => ({
  usePlanBranchForTask: vi.fn(() => ({ data: mockPlanBranchState.current })),
}));

vi.mock("@/hooks/useGitDiff", () => ({
  useGitDiff: vi.fn(() => ({
    changes: [],
    commits: mockGitDiffState.commits,
    commitFiles: [],
    isLoadingChanges: false,
    isLoadingHistory: false,
    isLoadingCommitFiles: false,
    error: null,
    fetchDiff: vi.fn(),
    fetchCommitFiles: vi.fn(),
    refresh: vi.fn(),
  })),
}));

vi.mock("@/hooks/useReviews", () => ({
  useTaskStateHistory: vi.fn(() => ({ data: [], isLoading: false })),
}));

vi.mock("@/hooks/useTaskStateTransitions", () => ({
  useTaskStateTransitions: vi.fn(() => ({ data: [] })),
}));

vi.mock("@/components/reviews/ReviewDetailModal", () => ({
  ReviewDetailModal: ({
    taskId,
    reviewMode,
  }: {
    taskId: string;
    reviewMode?: string;
  }) => (
    <div data-testid="review-detail-modal">
      Review modal for {taskId} in {reviewMode ?? "task"} mode
    </div>
  ),
}));

import { api } from "@/lib/tauri";

const mockApiTasksStop = vi.mocked(api.tasks.stop);

// Stable mock listeners for EventBus
const mockListeners = new Map<string, Set<(payload: unknown) => void>>();

const stableBus = {
  subscribe: (eventName: string, callback: (payload: unknown) => void) => {
    if (!mockListeners.has(eventName)) {
      mockListeners.set(eventName, new Set());
    }
    mockListeners.get(eventName)!.add(callback);
    return () => {
      mockListeners.get(eventName)?.delete(callback);
    };
  },
};

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => stableBus,
}));

function emitEvent(eventName: string, payload: unknown) {
  const listeners = mockListeners.get(eventName);
  if (listeners) {
    for (const listener of listeners) {
      listener(payload);
    }
  }
}

function createTestTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Test Task",
    description: "Test description",
    priority: 2,
    internalStatus: "pending_merge",
    needsReviewPoint: false,
    createdAt: "2026-01-28T12:00:00+00:00",
    updatedAt: "2026-01-28T12:00:00+00:00",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    taskBranch: "ralphx/ralphx/task-123",
    worktreePath: null,
    mergeCommitSha: null,
    metadata: null,
    ...overrides,
  };
}

function makeProgressEvent(
  overrides: Partial<MergeProgressEvent> = {}
): MergeProgressEvent {
  return {
    task_id: "task-123",
    phase: "worktree_setup",
    status: "started",
    message: "",
    timestamp: "2026-02-11T10:00:00Z",
    ...overrides,
  };
}

function createTestPlanBranch(overrides?: Partial<PlanBranch>): PlanBranch {
  return {
    id: "plan-branch-123",
    planArtifactId: "artifact-123",
    sessionId: "session-123",
    projectId: "project-456",
    branchName: "ralphx/ralphx/plan-a3612efd",
    sourceBranch: "main",
    status: "active",
    mergeTaskId: "task-123",
    createdAt: "2026-01-28T12:00:00+00:00",
    mergedAt: null,
    prNumber: 68,
    prUrl: "https://github.com/aigentive/ralphx.app/pull/68",
    prDraft: false,
    prPushStatus: "pushed",
    prStatus: "Open",
    prPollingActive: true,
    prEligible: true,
    baseBranchOverride: null,
    ...overrides,
  };
}

function createReviewNote(overrides?: Partial<ReviewNoteResponse>): ReviewNoteResponse {
  return {
    id: "review-note-1",
    task_id: "plan-task-1",
    reviewer: "ai",
    outcome: "approved",
    summary: "Plan implementation review passed",
    notes: "No follow-up work needed.",
    issues: [],
    followup_session_id: null,
    created_at: "2026-01-28T12:20:00+00:00",
    ...overrides,
  };
}

function TestWrapper({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{children}</TooltipProvider>
    </QueryClientProvider>
  );
}

function renderWithProviders(ui: React.ReactElement) {
  return render(ui, { wrapper: TestWrapper });
}

describe("MergingTaskDetail", () => {
  beforeEach(() => {
    mockListeners.clear();
    mockPlanBranchState.current = null;
    mockPlanArtifactState.current = {
      id: "artifact-123",
      name: "Plan: Fix graph crash",
      artifact_type: "specification",
      content_type: "inline",
      content:
        "# Fix graph crash when no active plan selected\n\nGuard graph rendering when no plan is selected.",
      created_at: "2026-01-28T12:00:00+00:00",
      created_by: "test",
      version: 3,
      bucket_id: null,
      task_id: null,
      process_id: null,
      derived_from: [],
    };
    mockGitDiffState.commits = [];
    const planTask = createTestTask({
      id: "plan-task-1",
      title: "Fix graph crash",
      category: "feature",
      internalStatus: "merged",
    });
    mockPlanReviewState.tasks = [planTask];
    mockPlanReviewState.historiesByTaskId = new Map([
      [planTask.id, [createReviewNote({ task_id: planTask.id })]],
    ]);
    // Mock invoke to return resolved promises for hydration calls
    vi.mocked(invoke).mockResolvedValue(undefined);
    mockConfirmation.confirm = vi.fn(async () => true);
    mockApiTasksStop.mockReset();
    mockApiTasksStop.mockResolvedValue({} as never);
  });

  describe("progress rendering", () => {
    it("shows 'Waiting for merge progress...' when no progress events received", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByTestId("merge-resuming-section")).toBeInTheDocument();
      expect(screen.getByText("Waiting for merge progress...")).toBeInTheDocument();
    });

    it("renders phase timeline when progress events arrive", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // Initially shows waiting state
      expect(screen.getByText("Waiting for merge progress...")).toBeInTheDocument();

      // Emit first progress event
      act(() => {
        emitEvent(
          "task:merge_progress",
          makeProgressEvent({ phase: "worktree_setup", status: "started", message: "Setting up..." })
        );
      });

      // Phase timeline should appear, waiting state should disappear
      expect(screen.getByTestId("merge-phase-timeline")).toBeInTheDocument();
      expect(screen.queryByText("Waiting for merge progress...")).not.toBeInTheDocument();
      expect(screen.getByText("Worktree Setup")).toBeInTheDocument();
    });

    it("renders sequence of phases in correct order with proper status indicators", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // Emit dynamic phase list first (as backend does)
      act(() => {
        emitEvent("task:merge_phases", {
          task_id: "task-123",
          phases: [
            { id: "worktree_setup", label: "Worktree Setup" },
            { id: "programmatic_merge", label: "Merge" },
            { id: "npm_run_typecheck", label: "Type Check" },
            { id: "finalize", label: "Finalize" },
          ],
        });
      });

      // Emit full sequence
      const phases: Array<{ phase: string; status: MergeProgressEvent["status"]; message: string }> = [
        { phase: "worktree_setup", status: "started", message: "Setting up worktree" },
        { phase: "worktree_setup", status: "passed", message: "" },
        { phase: "programmatic_merge", status: "started", message: "Merging branches" },
        { phase: "programmatic_merge", status: "passed", message: "" },
        { phase: "npm_run_typecheck", status: "started", message: "Running type checker..." },
      ];

      for (const { phase, status, message } of phases) {
        act(() => {
          emitEvent("task:merge_progress", makeProgressEvent({ phase, status, message }));
        });
      }

      // All phases up to typecheck should be visible
      expect(screen.getByText("Worktree Setup")).toBeInTheDocument();
      expect(screen.getByText("Merge")).toBeInTheDocument();
      expect(screen.getByText("Type Check")).toBeInTheDocument();
      expect(screen.getByText("Running type checker...")).toBeInTheDocument();
    });

    it("updates phase status from started to passed", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // Start worktree_setup
      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "worktree_setup",
          status: "started",
          message: "Setting up...",
        }));
      });

      expect(screen.getByText("Setting up...")).toBeInTheDocument();

      // Complete worktree_setup
      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "worktree_setup",
          status: "passed",
          message: "Done",
        }));
      });

      // Message for passed phases should not be shown (per MergePhaseTimeline behavior)
      expect(screen.queryByText("Done")).not.toBeInTheDocument();
      expect(screen.queryByText("Setting up...")).not.toBeInTheDocument();
    });

    it("shows failed phase message when a phase fails", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "worktree_setup",
          status: "passed",
        }));
      });

      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "programmatic_merge",
          status: "passed",
        }));
      });

      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "npm_run_typecheck",
          status: "failed",
          message: "Type errors found in 3 files",
        }));
      });

      expect(screen.getByText("Type errors found in 3 files")).toBeInTheDocument();
    });

    it("renders full seven-phase sequence through finalize", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // Emit dynamic phase list first
      act(() => {
        emitEvent("task:merge_phases", {
          task_id: "task-123",
          phases: [
            { id: "worktree_setup", label: "Worktree Setup" },
            { id: "programmatic_merge", label: "Merge" },
            { id: "npm_run_typecheck", label: "Type Check" },
            { id: "npm_run_lint", label: "Lint" },
            { id: "cargo_clippy", label: "Clippy" },
            { id: "cargo_test", label: "Test" },
            { id: "finalize", label: "Finalize" },
          ],
        });
      });

      const fullSequence: Array<{ phase: string; status: MergeProgressEvent["status"] }> = [
        { phase: "worktree_setup", status: "passed" },
        { phase: "programmatic_merge", status: "passed" },
        { phase: "npm_run_typecheck", status: "passed" },
        { phase: "npm_run_lint", status: "passed" },
        { phase: "cargo_clippy", status: "passed" },
        { phase: "cargo_test", status: "passed" },
        { phase: "finalize", status: "started" },
      ];

      for (const { phase, status } of fullSequence) {
        act(() => {
          emitEvent("task:merge_progress", makeProgressEvent({ phase, status }));
        });
      }

      expect(screen.getByText("Worktree Setup")).toBeInTheDocument();
      expect(screen.getByText("Merge")).toBeInTheDocument();
      expect(screen.getByText("Type Check")).toBeInTheDocument();
      expect(screen.getByText("Lint")).toBeInTheDocument();
      expect(screen.getByText("Clippy")).toBeInTheDocument();
      expect(screen.getByText("Test")).toBeInTheDocument();
      expect(screen.getByText("Finalize")).toBeInTheDocument();
    });
  });

  describe("reload-style remount recovery", () => {
    it("accumulates events, loses them on unmount, and remounts to fresh state", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });

      // First render
      const { unmount } = renderWithProviders(<MergingTaskDetail task={task} />);

      // Emit some progress events
      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "worktree_setup",
          status: "passed",
        }));
      });
      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "programmatic_merge",
          status: "started",
        }));
      });

      // Should show phase timeline with accumulated events
      expect(screen.getByTestId("merge-phase-timeline")).toBeInTheDocument();
      expect(screen.getByText("Worktree Setup")).toBeInTheDocument();
      expect(screen.getByText("Merge")).toBeInTheDocument();

      // Simulate reload: unmount component
      unmount();

      // Events emitted while component is unmounted won't be captured
      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "programmatic_merge",
          status: "passed",
        }));
      });
      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "typecheck",
          status: "started",
        }));
      });

      // Remount: component starts fresh (hooks re-subscribe)
      renderWithProviders(<MergingTaskDetail task={task} />);

      // After remount with no new events, should show waiting state
      expect(screen.getByText("Waiting for merge progress...")).toBeInTheDocument();
      expect(screen.queryByTestId("merge-phase-timeline")).not.toBeInTheDocument();
    });

    it("recovers and displays new events after remount", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });

      // First render
      const { unmount } = renderWithProviders(<MergingTaskDetail task={task} />);

      // Emit initial event
      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "worktree_setup",
          status: "started",
        }));
      });

      expect(screen.getByTestId("merge-phase-timeline")).toBeInTheDocument();

      // Unmount (simulate reload)
      unmount();
      cleanup();

      // Remount
      renderWithProviders(<MergingTaskDetail task={task} />);

      // Starts with waiting state
      expect(screen.getByText("Waiting for merge progress...")).toBeInTheDocument();

      // New events arrive after remount
      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "npm_run_typecheck",
          status: "started",
          message: "Checking types...",
        }));
      });

      // Component recovers and shows new events
      expect(screen.getByTestId("merge-phase-timeline")).toBeInTheDocument();
      expect(screen.getByText("Type Check")).toBeInTheDocument();
      expect(screen.getByText("Checking types...")).toBeInTheDocument();
    });
  });

  describe("fallback behavior when events are missing or delayed", () => {
    it("shows waiting state for pending_merge task with no events", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // Should show the resuming/waiting section
      expect(screen.getByTestId("merge-resuming-section")).toBeInTheDocument();
      expect(screen.getByText("Waiting for merge progress...")).toBeInTheDocument();
    });

    it("does not show phase timeline or waiting state for historical pending_merge", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} isHistorical viewStatus="pending_merge" />);

      // Historical mode should not show live progress or waiting state
      expect(screen.queryByTestId("merge-phase-timeline")).not.toBeInTheDocument();
      expect(screen.queryByTestId("merge-resuming-section")).not.toBeInTheDocument();
    });

    it("does not show phase timeline or waiting state for merging (agent phase)", () => {
      const task = createTestTask({ internalStatus: "merging" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // Agent phase doesn't show merge progress timeline
      expect(screen.queryByTestId("merge-phase-timeline")).not.toBeInTheDocument();
      expect(screen.queryByTestId("merge-resuming-section")).not.toBeInTheDocument();
    });

    it("does not show stale metadata validation log in live mode", () => {
      const metadata = JSON.stringify({
        validation_log: [
          {
            task_id: "task-123",
            phase: "validate",
            command: "npm run typecheck",
            path: ".",
            label: "Type Check",
            status: "success",
            duration_ms: 3200,
          },
        ],
      });

      const task = createTestTask({
        internalStatus: "pending_merge",
        metadata,
      });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // In live mode (not historical), stale metadata should NOT be shown
      expect(screen.queryByTestId(`validation-progress-${task.id}`)).not.toBeInTheDocument();
    });

    it("shows metadata validation log in historical mode", () => {
      const metadata = JSON.stringify({
        validation_log: [
          {
            task_id: "task-123",
            phase: "validate",
            command: "npm run typecheck",
            path: ".",
            label: "Type Check",
            status: "success",
            duration_ms: 3200,
          },
          {
            task_id: "task-123",
            phase: "validate",
            command: "npm run lint",
            path: ".",
            label: "Lint",
            status: "failed",
            exit_code: 1,
            stderr: "ESLint error",
          },
        ],
      });

      const task = createTestTask({
        internalStatus: "pending_merge",
        metadata,
      });
      renderWithProviders(<MergingTaskDetail task={task} isHistorical viewStatus="pending_merge" />);

      // In historical mode, metadata should be shown
      expect(screen.getByTestId(`validation-progress-${task.id}`)).toBeInTheDocument();
      expect(screen.getByText("Merge Validation")).toBeInTheDocument();
    });

    it("ignores events for different task IDs", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          task_id: "other-task-999",
          phase: "worktree_setup",
          status: "started",
        }));
      });

      // Should still show waiting state since event was for different task
      expect(screen.getByText("Waiting for merge progress...")).toBeInTheDocument();
      expect(screen.queryByTestId("merge-phase-timeline")).not.toBeInTheDocument();
    });

    it("ignores malformed progress event payloads", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      act(() => {
        emitEvent("task:merge_progress", { invalid: "data" });
      });

      act(() => {
        emitEvent("task:merge_progress", null);
      });

      act(() => {
        emitEvent("task:merge_progress", "not an object");
      });

      // Should still show waiting state
      expect(screen.getByText("Waiting for merge progress...")).toBeInTheDocument();
    });

    it("transitions from waiting to phase timeline when first event arrives after delay", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // Initially waiting
      expect(screen.getByText("Waiting for merge progress...")).toBeInTheDocument();

      // First event arrives (simulating delayed backend response)
      act(() => {
        emitEvent("task:merge_progress", makeProgressEvent({
          phase: "worktree_setup",
          status: "started",
          message: "Initializing...",
        }));
      });

      // Should switch to timeline
      expect(screen.queryByText("Waiting for merge progress...")).not.toBeInTheDocument();
      expect(screen.getByTestId("merge-phase-timeline")).toBeInTheDocument();
      expect(screen.getByText("Initializing...")).toBeInTheDocument();
    });
  });

  describe("pending_merge basic rendering", () => {
    it("renders merging-task-detail test id", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByTestId("merging-task-detail")).toBeInTheDocument();
    });

    it("shows 'Merging Changes...' title for active pending_merge", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByText("Merging Changes...")).toBeInTheDocument();
    });

    it("shows branch info", () => {
      const task = createTestTask({
        internalStatus: "pending_merge",
        taskBranch: "ralphx/ralphx/task-123",
      });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // BranchBadge strips "ralphx/<slug>/" prefix; full name is in title attr
      expect(screen.getByText("task-123")).toBeInTheDocument();
      expect(screen.getByTitle("ralphx/ralphx/task-123")).toBeInTheDocument();
    });
  });

  describe("merging (agent phase) rendering", () => {
    it("shows 'Resolving Merge Conflicts' for agent phase with no conflict type", () => {
      const task = createTestTask({ internalStatus: "merging" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByText("Resolving Merge Conflicts")).toBeInTheDocument();
    });

    it("does not present legacy freshness metadata as the active merge workflow", () => {
      const task = createTestTask({
        internalStatus: "merging",
        metadata: JSON.stringify({ freshness_origin_state: "executing" }),
      });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByText("Resolving Merge Conflicts")).toBeInTheDocument();
      expect(
        screen.queryByText(/Stale branches detected when starting execution/),
      ).not.toBeInTheDocument();
    });

    it("shows PR waiting copy instead of conflict-agent UI for PR-backed plan merge", async () => {
      mockPlanBranchState.current = createTestPlanBranch();
      const task = createTestTask({
        internalStatus: "merging",
        category: "plan_merge",
        taskBranch: null,
      });

      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(await screen.findByText("Fix graph crash when no active plan selected")).toBeInTheDocument();
      expect(screen.getByText("Waiting on Pull Request")).toBeInTheDocument();
      expect(
        screen.getByText(
          "Review and merge PR #68 in GitHub. RalphX will finish this plan after GitHub reports it merged."
        )
      ).toBeInTheDocument();
      expect(screen.getByTestId("pr-mode-section")).toBeInTheDocument();
      expect(screen.getAllByText("PR #68").length).toBeGreaterThanOrEqual(1);
      expect(screen.getByText("Waiting for GitHub review or merge.")).toBeInTheDocument();
      expect(screen.queryByText("Agent resolving conflicts")).not.toBeInTheDocument();
      expect(screen.queryByTestId("merge-progress-section")).not.toBeInTheDocument();
      expect(screen.queryByTestId("merging-actions-section")).not.toBeInTheDocument();
      expect(screen.queryByText("Stop Merge")).not.toBeInTheDocument();
      expect(screen.getAllByTitle("ralphx/ralphx/plan-a3612efd").length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByTitle("main").length).toBeGreaterThanOrEqual(1);
    });

    it("shows plan context for local plan merges before PR mode", async () => {
      mockPlanBranchState.current = createTestPlanBranch({
        prNumber: null,
        prUrl: null,
        prStatus: null,
        prPollingActive: false,
        prEligible: false,
      });
      const task = createTestTask({
        internalStatus: "pending_merge",
        category: "plan_merge",
        taskBranch: null,
      });

      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(await screen.findByText("Fix graph crash when no active plan selected")).toBeInTheDocument();
      expect(screen.getByText("Guard graph rendering when no plan is selected.")).toBeInTheDocument();
      expect(screen.queryByTestId("pr-mode-section")).not.toBeInTheDocument();
    });

    it("shows merger-agent copy for PR branch update conflicts instead of PR waiting copy", () => {
      mockPlanBranchState.current = createTestPlanBranch({
        prPollingActive: false,
      });
      const metadata = JSON.stringify({
        plan_update_conflict: true,
        pr_branch_update_conflict: true,
        base_branch: "origin/main",
        target_branch: "ralphx/ralphx/plan-a3612efd",
        conflict_files: ["src/app.ts"],
      });
      const task = createTestTask({
        internalStatus: "merging",
        category: "plan_merge",
        taskBranch: null,
        metadata,
      });

      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByText("Updating PR Branch")).toBeInTheDocument();
      expect(
        screen.getByText(
          "A merger agent is updating PR #68 with the latest changes from origin/main so GitHub review can continue."
        )
      ).toBeInTheDocument();
      expect(screen.getByTestId("merge-progress-section")).toBeInTheDocument();
      expect(screen.getByTestId("conflict-files-section")).toBeInTheDocument();
      expect(screen.getByTestId("merging-actions-section")).toBeInTheDocument();
      expect(screen.getByText("Stop Merge")).toBeInTheDocument();
      expect(screen.queryByText("Waiting on Pull Request")).not.toBeInTheDocument();
      expect(
        screen.queryByText(
          "Review and merge PR #68 in GitHub. RalphX will finish this plan after GitHub reports it merged."
        )
      ).not.toBeInTheDocument();
      expect(screen.getByTitle("origin/main")).toBeInTheDocument();
      expect(screen.getByTitle("ralphx/ralphx/plan-a3612efd")).toBeInTheDocument();
    });

    it("shows full commit history and Review Code for PR-backed plan merge", async () => {
      const user = userEvent.setup();
      mockPlanBranchState.current = createTestPlanBranch();
      mockGitDiffState.commits = Array.from({ length: 6 }, (_, index) => ({
        sha: `sha-${index + 1}`,
        shortSha: `abc123${index}`,
        message: `feat: plan branch commit ${index + 1}`,
        author: "RalphX",
        date: new Date("2026-01-28T12:00:00+00:00"),
      }));
      const task = createTestTask({
        internalStatus: "waiting_on_pr",
        category: "plan_merge",
        taskBranch: null,
      });

      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(await screen.findByText("Fix graph crash when no active plan selected")).toBeInTheDocument();
      expect(screen.getByTestId("commits-section")).toBeInTheDocument();
      for (const commit of mockGitDiffState.commits) {
        expect(screen.getByText(commit.shortSha)).toBeInTheDocument();
        expect(screen.getByText(commit.message)).toBeInTheDocument();
      }
      expect(screen.queryByText("+1 more commits")).not.toBeInTheDocument();
      expect(screen.getByText("Code Review")).toBeInTheDocument();
      expect(screen.getByText("Review Diff")).toBeInTheDocument();
      expect(await screen.findByText("Plan implementation review passed")).toBeInTheDocument();
      expect(screen.getByText("Fix graph crash")).toBeInTheDocument();
      expect(screen.queryByText("No review history available")).not.toBeInTheDocument();
      expect(screen.queryByText("No internal plan review records available")).not.toBeInTheDocument();

      await user.click(screen.getByTestId("review-code-button"));

      expect(screen.getByTestId("review-detail-modal")).toHaveTextContent(
        "Review modal for task-123 in plan_merge mode"
      );
    });

    it("renders historical PR wait without live polling or conflict-agent copy", async () => {
      mockPlanBranchState.current = createTestPlanBranch({
        prPollingActive: true,
      });
      const task = createTestTask({
        internalStatus: "merged",
        category: "plan_merge",
        taskBranch: null,
      });

      const { container } = renderWithProviders(
        <MergingTaskDetail task={task} isHistorical viewStatus="waiting_on_pr" />
      );

      expect(await screen.findByText("Fix graph crash when no active plan selected")).toBeInTheDocument();
      expect(screen.getByText("Merge Completed")).toBeInTheDocument();
      expect(screen.getByTestId("pr-mode-section")).toBeInTheDocument();
      expect(
        screen.getByText("At this point, RalphX was waiting for GitHub review or merge.")
      ).toBeInTheDocument();
      expect(screen.queryByText("Waiting for GitHub review or merge.")).not.toBeInTheDocument();
      expect(screen.queryByText("Agent resolving conflicts")).not.toBeInTheDocument();
      expect(screen.getByText("Waiting on pull request")).toBeInTheDocument();
      expect(container.querySelector(".animate-spin")).not.toBeInTheDocument();
    });

    it("shows conflict files when present in metadata", () => {
      const metadata = JSON.stringify({
        conflict_files: ["src/main.ts", "src/lib/utils.ts"],
      });
      const task = createTestTask({ internalStatus: "merging", metadata });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByTestId("conflict-files-section")).toBeInTheDocument();
      expect(screen.getByText("src/main.ts")).toBeInTheDocument();
      expect(screen.getByText("src/lib/utils.ts")).toBeInTheDocument();
    });

    it("shows 'Updating Plan Branch' for plan_update_conflict in merging state", () => {
      const metadata = JSON.stringify({ plan_update_conflict: true });
      const task = createTestTask({ internalStatus: "merging", metadata });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByText("Updating Plan Branch")).toBeInTheDocument();
      expect(screen.getByText("Merging latest changes from main into plan branch")).toBeInTheDocument();
    });

    it("shows 'Updating Task Branch' for source_update_conflict in merging state", () => {
      const metadata = JSON.stringify({ source_update_conflict: true });
      const task = createTestTask({ internalStatus: "merging", metadata });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByText("Updating Task Branch")).toBeInTheDocument();
      expect(screen.getByText("Merging latest changes from plan into task branch")).toBeInTheDocument();
    });
  });

  describe("conflict type in historical mode (resolving)", () => {
    it("shows 'Updating Plan Branch' in historical resolving with plan_update_conflict", () => {
      const metadata = JSON.stringify({ plan_update_conflict: true });
      const task = createTestTask({ internalStatus: "merging", metadata });
      renderWithProviders(<MergingTaskDetail task={task} isHistorical viewStatus="merging" />);

      expect(screen.getByText("Updating Plan Branch")).toBeInTheDocument();
    });

    it("shows 'Updating Task Branch' in historical resolving with source_update_conflict", () => {
      const metadata = JSON.stringify({ source_update_conflict: true });
      const task = createTestTask({ internalStatus: "merging", metadata });
      renderWithProviders(<MergingTaskDetail task={task} isHistorical viewStatus="merging" />);

      expect(screen.getByText("Updating Task Branch")).toBeInTheDocument();
    });

    it("shows 'Resolving Conflicts' in historical resolving with no conflict type", () => {
      const task = createTestTask({ internalStatus: "merging" });
      renderWithProviders(<MergingTaskDetail task={task} isHistorical viewStatus="merging" />);

      expect(screen.getByText("Resolving Conflicts")).toBeInTheDocument();
    });
  });

  describe("Stop Merge action button", () => {
    it("shows Stop Merge button during active agent merge", () => {
      const task = createTestTask({ internalStatus: "merging" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.getByTestId("merging-actions-section")).toBeInTheDocument();
      expect(screen.getByTestId("stop-merge-action")).toBeInTheDocument();
      expect(screen.getByText("Stop Merge")).toBeInTheDocument();
    });

    it("hides Stop Merge button in historical mode", () => {
      const task = createTestTask({ internalStatus: "merging" });
      renderWithProviders(<MergingTaskDetail task={task} isHistorical viewStatus="merging" />);

      expect(screen.queryByTestId("merging-actions-section")).not.toBeInTheDocument();
      expect(screen.queryByTestId("stop-merge-action")).not.toBeInTheDocument();
    });

    it("hides Stop Merge button during programmatic merge phase (pending_merge)", () => {
      const task = createTestTask({ internalStatus: "pending_merge" });
      renderWithProviders(<MergingTaskDetail task={task} />);

      expect(screen.queryByTestId("merging-actions-section")).not.toBeInTheDocument();
      expect(screen.queryByTestId("stop-merge-action")).not.toBeInTheDocument();
    });

    it("shows confirmation dialog when Stop Merge is clicked", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "merging" });
      mockConfirmation.confirm = vi.fn(async () => false);
      renderWithProviders(<MergingTaskDetail task={task} />);

      await user.click(screen.getByTestId("stop-merge-action"));

      expect(mockConfirmation.confirm).toHaveBeenCalledWith(
        expect.objectContaining({
          title: "Stop merge?",
          variant: "destructive",
        })
      );
    });

    it("calls api.tasks.stop when Stop Merge is confirmed", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "merging" });
      mockConfirmation.confirm = vi.fn(async () => true);
      renderWithProviders(<MergingTaskDetail task={task} />);

      await user.click(screen.getByTestId("stop-merge-action"));

      expect(mockApiTasksStop).toHaveBeenCalledWith("task-123");
    });

    it("does not call api when confirmation is cancelled", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "merging" });
      mockConfirmation.confirm = vi.fn(async () => false);
      renderWithProviders(<MergingTaskDetail task={task} />);

      await user.click(screen.getByTestId("stop-merge-action"));

      expect(mockApiTasksStop).not.toHaveBeenCalled();
    });
  });

  describe("Agents fork Stop Merge gate", () => {
    it.each([
      ["remote-default", true],
      ["remote-agent", false],
      ["local", false],
    ] as const)("follows taskStop gating in %s", (environment, disabled) => {
      setDetailViewEnvironment(environment);
      const task = createTestTask({ internalStatus: "merging" });
      renderWithProviders(<AgentsMergingTaskDetail task={task} />);

      const stop = screen.getByTestId("stop-merge-action");
      disabled ? expect(stop).toBeDisabled() : expect(stop).toBeEnabled();
      if (disabled) {
        expect(screen.getByTestId("agent-gate-tooltip")).toHaveAttribute("data-agent-gated", "true");
      } else {
        expect(screen.queryByTestId("agent-gate-tooltip")).not.toBeInTheDocument();
      }
    });
  });

  describe("validation recovery mode", () => {
    it("shows recovery UI when validation_recovery is true in metadata", () => {
      const metadata = JSON.stringify({
        validation_recovery: true,
        validation_failures: [
          { command: "npm run typecheck", exit_code: 1, stderr: "Type errors" },
        ],
      });
      const task = createTestTask({ internalStatus: "merging", metadata });
      renderWithProviders(<MergingTaskDetail task={task} />);

      // Recovery mode shows "Fixing Validation Errors..." title
      expect(screen.getByText("Fixing Validation Errors...")).toBeInTheDocument();
      // Validation failures are shown via ValidationProgress (no separate section)
      expect(screen.queryByTestId("validation-failures-section")).not.toBeInTheDocument();
    });
  });
});
