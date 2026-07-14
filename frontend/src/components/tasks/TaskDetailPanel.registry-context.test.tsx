import React from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TaskDetailPanel } from "./TaskDetailPanel";
import type { PlanBranch } from "@/api/plan-branch.types";
import type { Task } from "@/types/task";
import type { TaskContext } from "@/types/task-context";

const mockTaskContextApi = vi.hoisted(() => ({
  getTaskContext: vi.fn(),
}));

const mockPlanBranchApi = vi.hoisted(() => ({
  getByProject: vi.fn(),
}));

vi.mock("@/api/task-context", () => ({
  taskContextApi: mockTaskContextApi,
}));

vi.mock("@/api/plan-branch", () => ({
  planBranchApi: mockPlanBranchApi,
}));

vi.mock("@/hooks/useReviews", () => ({
  useReviewsByTaskId: vi.fn(() => ({ data: [], isLoading: false })),
}));

vi.mock("@/hooks/useTaskSteps", () => ({
  useTaskSteps: vi.fn(() => ({ data: [], isLoading: false })),
}));

vi.mock("./detail-views", async () => {
  const { TwoColumnLayout } = await import("./detail-views/shared");
  const MockRegistryView = ({ task }: { task: Task }) => (
    <TwoColumnLayout description={task.description} testId="mock-registry-view">
      <div>Registry body for {task.title}</div>
    </TwoColumnLayout>
  );

  return {
    BasicTaskDetail: MockRegistryView,
    RevisionTaskDetail: MockRegistryView,
    ExecutionTaskDetail: MockRegistryView,
    ReviewingTaskDetail: MockRegistryView,
    HumanReviewTaskDetail: MockRegistryView,
    EscalatedTaskDetail: MockRegistryView,
    WaitingTaskDetail: MockRegistryView,
    CompletedTaskDetail: MockRegistryView,
    MergingTaskDetail: MockRegistryView,
    MergeConflictTaskDetail: MockRegistryView,
    MergeIncompleteTaskDetail: MockRegistryView,
    MergedTaskDetail: MockRegistryView,
    BranchUpdateTaskDetail: MockRegistryView,
  };
});

function createTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Test Task",
    description: "Task description",
    priority: 2,
    internalStatus: "ready",
    needsReviewPoint: false,
    ideationSessionId: "session-123",
    createdAt: "2026-01-28T12:00:00+00:00",
    updatedAt: "2026-01-28T12:15:00+00:00",
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

function createTaskContext(): TaskContext {
  return {
    task: {
      id: "task-123",
      project_id: "project-456",
      category: "feature",
      title: "Test Task",
      description: "Task description",
      priority: 2,
      internal_status: "ready",
      needs_review_point: false,
      ideation_session_id: "session-123",
      created_at: "2026-01-28T12:00:00+00:00",
      updated_at: "2026-01-28T12:15:00+00:00",
      started_at: null,
      completed_at: null,
      archived_at: null,
      blocked_reason: null,
      task_branch: "ralphx/ralphx/task-123",
      worktree_path: null,
      merge_commit_sha: null,
      metadata: null,
    },
    sourceProposal: null,
    planArtifact: {
      id: "artifact-123",
      title: "Plan visible from registry detail",
      artifactType: "specification",
      currentVersion: 2,
      contentPreview: "Shared context rail content.",
    },
    relatedArtifacts: [],
    contextHints: [],
  };
}

function createPlanBranch(): PlanBranch {
  return {
    id: "plan-branch-123",
    planArtifactId: "artifact-123",
    sessionId: "session-123",
    projectId: "project-456",
    branchName: "ralphx/ralphx/plan-a3612efd",
    sourceBranch: "main",
    status: "active",
    mergeTaskId: "merge-task-123",
    createdAt: "2026-01-28T12:00:00+00:00",
    mergedAt: null,
    prNumber: 68,
    prUrl: "https://github.com/aigentive/ralphx.app/pull/68",
    prDraft: false,
    prPushStatus: "pushed",
    prStatus: "Open",
    prPollingActive: true,
    prEligible: true,
    mergeCommitSha: null,
    baseBranchOverride: null,
  };
}

function renderPanel(task: Task, props?: Partial<React.ComponentProps<typeof TaskDetailPanel>>) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <TaskDetailPanel task={task} useViewRegistry {...props} />
    </QueryClientProvider>
  );
}

describe("TaskDetailPanel registry context", () => {
  beforeEach(() => {
    mockTaskContextApi.getTaskContext.mockResolvedValue(createTaskContext());
    mockPlanBranchApi.getByProject.mockResolvedValue([createPlanBranch()]);
  });

  it("wraps registry views with the common task context rail", async () => {
    renderPanel(createTask());

    expect(screen.getByText("Registry body for Test Task")).toBeInTheDocument();
    expect(await screen.findByText("Plan visible from registry detail")).toBeInTheDocument();
    expect(screen.queryByText("Shared context rail content.")).not.toBeInTheDocument();
    expect(screen.getByText("PR #68")).toBeInTheDocument();
    expect(screen.getByText("task-123")).toBeInTheDocument();
    expect(screen.queryByText("Merge")).not.toBeInTheDocument();
  });

  it("passes historical state metadata into the common rail", async () => {
    renderPanel(createTask({ internalStatus: "merged" }), {
      viewAsStatus: "waiting_on_pr",
      viewTimestamp: "2026-01-28T12:20:00+00:00",
      viewConversationId: "conversation-123",
      viewAgentRunId: "run-123",
    });

    expect(await screen.findByText("Historical State")).toBeInTheDocument();
    expect(screen.getByText("Waiting on PR")).toBeInTheDocument();
  });

  it.each([
    "updating_plan_branch",
    "updating_task_branch",
    "branch_update_blocked",
  ] as const)("wraps %s with the common task context rail", async (internalStatus) => {
    renderPanel(createTask({ internalStatus }));

    expect(screen.getByText("Registry body for Test Task")).toBeInTheDocument();
    expect(await screen.findByText("Plan visible from registry detail")).toBeInTheDocument();
  });
});
