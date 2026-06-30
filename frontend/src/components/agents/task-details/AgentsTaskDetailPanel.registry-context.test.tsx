import React from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AgentsTaskDetailPanel } from "./AgentsTaskDetailPanel";
import type { PlanBranch } from "@/api/plan-branch.types";
import type { Task } from "@/types/task";
import type { TaskContext } from "@/types/task-context";

const mockTaskContextApi = vi.hoisted(() => ({
  getTaskContext: vi.fn(),
}));

const mockPlanBranchApi = vi.hoisted(() => ({
  getByProject: vi.fn(),
}));

const contextProbe = vi.hoisted(() => ({
  viewModes: [] as unknown[],
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
  const { useTaskDetailContextModel } = await import(
    "./detail-views/shared/TaskDetailContext"
  );
  const MockRegistryView = ({ task }: { task: Task }) => {
    const model = useTaskDetailContextModel();
    contextProbe.viewModes.push(model?.viewMode ?? null);

    return (
      <TwoColumnLayout description={task.description} testId="mock-agents-registry-view">
        <div>Agents registry body for {task.title}</div>
      </TwoColumnLayout>
    );
  };

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
      title: "Plan visible from agents registry detail",
      artifactType: "specification",
      currentVersion: 2,
      contentPreview: "Agents shared context rail content.",
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

function renderPanel(
  task: Task,
  props?: Partial<React.ComponentProps<typeof AgentsTaskDetailPanel>>
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <AgentsTaskDetailPanel task={task} useViewRegistry {...props} />
    </QueryClientProvider>
  );
}

describe("AgentsTaskDetailPanel registry context", () => {
  beforeEach(() => {
    mockTaskContextApi.getTaskContext.mockResolvedValue(createTaskContext());
    mockPlanBranchApi.getByProject.mockResolvedValue([createPlanBranch()]);
    contextProbe.viewModes = [];
  });

  it("wraps registry views with the common task context rail", async () => {
    renderPanel(createTask());

    expect(screen.getByText("Agents registry body for Test Task")).toBeInTheDocument();
    expect(await screen.findByText("Task")).toBeInTheDocument();
    const currentModes = contextProbe.viewModes.filter(Boolean);
    expect(currentModes.length).toBeGreaterThan(0);
    expect(currentModes[0]).toMatchObject({ kind: "current" });
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
    const historicalModes = contextProbe.viewModes.filter(Boolean);
    expect(historicalModes.length).toBeGreaterThan(0);
    expect(historicalModes[historicalModes.length - 1]).toMatchObject({
      kind: "historical",
      status: "waiting_on_pr",
      timestamp: "2026-01-28T12:20:00+00:00",
      conversationId: "conversation-123",
      agentRunId: "run-123",
    });
  });

  it("keeps registry view mode stable across identical rerenders", async () => {
    const task = createTask({
      category: "plan_merge",
      internalStatus: "merge_incomplete",
      metadata: JSON.stringify({
        error:
          "PR operation failed: Infrastructure error: failed to draft plan PR description: Database error: FOREIGN KEY constraint failed",
        source_branch: "agent-7dc9bd5e",
        target_branch: "main",
      }),
    });
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
      },
    });
    const renderUi = () => (
      <QueryClientProvider client={queryClient}>
        <AgentsTaskDetailPanel task={task} useViewRegistry />
      </QueryClientProvider>
    );
    const { rerender } = render(renderUi());

    expect(await screen.findByText("Agents registry body for Test Task")).toBeInTheDocument();
    const initialModes = contextProbe.viewModes.filter(Boolean);
    expect(initialModes.length).toBeGreaterThan(0);
    const stableMode = initialModes[0];
    expect(initialModes.every((mode) => mode === stableMode)).toBe(true);

    contextProbe.viewModes = [];
    rerender(renderUi());
    rerender(renderUi());

    expect(screen.getByText("Agents registry body for Test Task")).toBeInTheDocument();
    const rerenderedModes = contextProbe.viewModes.filter(Boolean);
    expect(rerenderedModes.length).toBeGreaterThan(0);
    expect(rerenderedModes.every((mode) => mode === stableMode)).toBe(true);
  });
});
