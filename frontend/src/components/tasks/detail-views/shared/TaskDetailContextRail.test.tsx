import React from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TaskDetailContextProvider } from "./TaskDetailContextProvider";
import { TwoColumnLayout } from "./TwoColumnLayout";
import type { PlanBranch } from "@/api/plan-branch.types";
import type { Artifact } from "@/types/artifact";
import type { TaskProposal } from "@/types/ideation";
import type { Task } from "@/types/task";
import type { TaskContext } from "@/types/task-context";

const mockTaskContextApi = vi.hoisted(() => ({
  getTaskContext: vi.fn(),
}));

const mockPlanBranchApi = vi.hoisted(() => ({
  getByProject: vi.fn(),
}));

const mockArtifactApi = vi.hoisted(() => ({
  get: vi.fn(),
}));

const mockIdeationApi = vi.hoisted(() => ({
  proposals: {
    get: vi.fn(),
  },
}));

const mockToTaskProposal = vi.hoisted(() => vi.fn((proposal) => proposal));

vi.mock("@/api/task-context", () => ({
  taskContextApi: mockTaskContextApi,
}));

vi.mock("@/api/plan-branch", () => ({
  planBranchApi: mockPlanBranchApi,
}));

vi.mock("@/api/artifact", () => ({
  artifactApi: mockArtifactApi,
}));

vi.mock("@/api/ideation", () => ({
  ideationApi: mockIdeationApi,
  toTaskProposal: mockToTaskProposal,
}));

vi.mock("@/components/Ideation/PlanDisplay", () => ({
  PlanDisplay: ({
    plan,
    showOverflowActions,
  }: {
    plan: Artifact;
    showOverflowActions?: boolean;
  }) => (
    <div>
      <div>Plan dialog: {plan.name}</div>
      <div>Overflow actions: {String(showOverflowActions ?? true)}</div>
    </div>
  ),
}));

function createTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Test Task",
    description: "Implement the selected plan item.",
    priority: 2,
    internalStatus: "merged",
    needsReviewPoint: false,
    ideationSessionId: "session-123",
    createdAt: "2026-01-28T12:00:00+00:00",
    updatedAt: "2026-01-28T12:15:00+00:00",
    startedAt: "2026-01-28T12:05:00+00:00",
    completedAt: "2026-01-28T12:30:00+00:00",
    archivedAt: null,
    blockedReason: null,
    taskBranch: "ralphx/ralphx/task-123",
    worktreePath: null,
    mergeCommitSha: "abc123456789",
    metadata: null,
    ...overrides,
  };
}

function createTaskContext(overrides?: Partial<TaskContext>): TaskContext {
  return {
    task: {
      id: "task-123",
      project_id: "project-456",
      category: "feature",
      title: "Test Task",
      description: "Implement the selected plan item.",
      priority: 2,
      internal_status: "merged",
      needs_review_point: false,
      ideation_session_id: "session-123",
      created_at: "2026-01-28T12:00:00+00:00",
      updated_at: "2026-01-28T12:15:00+00:00",
      started_at: "2026-01-28T12:05:00+00:00",
      completed_at: "2026-01-28T12:30:00+00:00",
      archived_at: null,
      blocked_reason: null,
      task_branch: "ralphx/ralphx/task-123",
      worktree_path: null,
      merge_commit_sha: "abc123456789",
      metadata: null,
    },
    sourceProposal: {
      id: "proposal-123",
      title: "Fix graph crash",
      description: "The graph should not crash without an active plan.",
      acceptanceCriteria: [],
      implementationNotes: null,
      planVersionAtCreation: 4,
    },
    planArtifact: {
      id: "artifact-123",
      title: "Fix graph crash when no active plan selected",
      artifactType: "specification",
      currentVersion: 4,
      contentPreview: "Guard graph rendering when no plan is selected.",
    },
    relatedArtifacts: [],
    contextHints: [],
    ...overrides,
  };
}

function createPlanBranch(overrides?: Partial<PlanBranch>): PlanBranch {
  return {
    id: "plan-branch-123",
    planArtifactId: "artifact-123",
    sessionId: "session-123",
    projectId: "project-456",
    branchName: "ralphx/ralphx/plan-a3612efd",
    sourceBranch: "main",
    status: "merged",
    mergeTaskId: "merge-task-123",
    createdAt: "2026-01-28T12:00:00+00:00",
    mergedAt: "2026-01-28T12:30:00+00:00",
    prNumber: 68,
    prUrl: "https://github.com/aigentive/ralphx.app/pull/68",
    prDraft: false,
    prPushStatus: "pushed",
    prStatus: null,
    prPollingActive: false,
    prEligible: true,
    mergeCommitSha: "abc123456789",
    baseBranchOverride: null,
    ...overrides,
  };
}

function createArtifact(overrides?: Partial<Artifact>): Artifact {
  return {
    id: "artifact-123",
    type: "specification",
    name: "Fix graph crash when no active plan selected",
    content: {
      type: "inline",
      text: "# Fix graph crash\n\nKeep the graph stable when no active plan is selected.",
    },
    metadata: {
      createdAt: "2026-01-28T12:00:00+00:00",
      createdBy: "system",
      version: 4,
    },
    derivedFrom: [],
    ...overrides,
  };
}

function createProposal(overrides?: Partial<TaskProposal>): TaskProposal {
  return {
    id: "proposal-123",
    sessionId: "session-123",
    title: "Fix graph crash",
    description: "The graph should not crash without an active plan.",
    category: "bugfix",
    steps: [
      "Guard the graph query when there is no active plan.",
      "Keep the timeline scoped to the current plan execution.",
    ],
    acceptanceCriteria: [
      "No crash when the user clears the active plan.",
      "Timeline events stay scoped to the current plan.",
    ],
    suggestedPriority: "high",
    priorityScore: 92,
    priorityReason: "This blocks normal use of the graph view.",
    estimatedComplexity: "moderate",
    userPriority: "high",
    userModified: false,
    status: "accepted",
    createdTaskId: null,
    planArtifactId: "artifact-123",
    planVersionAtCreation: 4,
    sortOrder: 0,
    createdAt: "2026-01-28T12:00:00+00:00",
    updatedAt: "2026-01-28T12:15:00+00:00",
    ...overrides,
  };
}

function renderRail(task: Task, viewMode: React.ComponentProps<typeof TaskDetailContextProvider>["viewMode"]) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <TaskDetailContextProvider task={task} viewMode={viewMode}>
        <TwoColumnLayout description={task.description}>
          <div>Main detail content</div>
        </TwoColumnLayout>
      </TaskDetailContextProvider>
    </QueryClientProvider>
  );
}

describe("TaskDetailContextRail", () => {
  beforeEach(() => {
    mockTaskContextApi.getTaskContext.mockResolvedValue(createTaskContext());
    mockPlanBranchApi.getByProject.mockResolvedValue([createPlanBranch()]);
    mockArtifactApi.get.mockResolvedValue(createArtifact());
    mockIdeationApi.proposals.get.mockResolvedValue(createProposal());
  });

  it("shows description, plan, branch, PR, and merge context in the common rail", async () => {
    renderRail(createTask({
      id: "merge-task-123",
      category: "plan_merge",
      taskBranch: null,
    }), { kind: "current" });

    expect(screen.getByText("Main detail content")).toBeInTheDocument();
    expect(screen.getByText("Implement the selected plan item.")).toBeInTheDocument();
    expect(await screen.findByText("Fix graph crash when no active plan selected")).toBeInTheDocument();
    expect(screen.getByText("Source Proposal")).toBeInTheDocument();
    expect(screen.getByText("Fix graph crash")).toBeInTheDocument();
    expect(screen.queryByText("Guard graph rendering when no plan is selected.")).not.toBeInTheDocument();
    expect(screen.queryByText("The graph should not crash without an active plan.")).not.toBeInTheDocument();
    expect(screen.getByText("plan-a3612efd")).toBeInTheDocument();
    expect(screen.getAllByText("main").length).toBeGreaterThan(0);
    expect(screen.getByText("PR #68")).toBeInTheDocument();
    expect(screen.getByText("Merged")).toBeInTheDocument();
    expect(screen.getByText("abc1234")).toBeInTheDocument();
  });

  it("opens the implementation plan in a dialog instead of inlining a rail excerpt", async () => {
    const user = userEvent.setup();

    renderRail(createTask({
      id: "merge-task-123",
      category: "plan_merge",
      taskBranch: null,
    }), { kind: "current" });

    await user.click(
      await screen.findByRole("button", {
        name: "Open plan Fix graph crash when no active plan selected",
      })
    );

    expect(await screen.findByText("Plan dialog: Fix graph crash when no active plan selected")).toBeInTheDocument();
    expect(screen.getByText("Overflow actions: false")).toBeInTheDocument();
    expect(mockArtifactApi.get).toHaveBeenCalledWith("artifact-123");
  });

  it("opens the source proposal in a dialog instead of inlining proposal details in the rail", async () => {
    const user = userEvent.setup();

    renderRail(createTask({
      id: "merge-task-123",
      category: "plan_merge",
      taskBranch: null,
    }), { kind: "current" });

    await user.click(
      await screen.findByRole("button", {
        name: "Open proposal Fix graph crash",
      })
    );

    expect(await screen.findByText("Priority Rationale")).toBeInTheDocument();
    expect(screen.getByText(/This blocks normal use of the graph view\./)).toBeInTheDocument();
    expect(screen.getByText("Implementation Steps")).toBeInTheDocument();
    expect(mockIdeationApi.proposals.get).toHaveBeenCalledWith("proposal-123");
  });

  it("labels historical views as latest context instead of pretending to show a snapshot", async () => {
    renderRail(createTask(), {
      kind: "historical",
      status: "waiting_on_pr",
      timestamp: "2026-01-28T12:20:00+00:00",
    });

    expect(await screen.findByText("Historical State")).toBeInTheDocument();
    expect(screen.getByText("Waiting on PR")).toBeInTheDocument();
    expect(screen.getByText("Plan, branch, and PR values show the latest task context.")).toBeInTheDocument();
  });

  it("keeps failed context reads distinct from absent plan context", async () => {
    mockTaskContextApi.getTaskContext.mockRejectedValue({
      outcome: "commandError",
      error: "REMOTE_INTERNAL_ERROR: task context unavailable",
    });
    mockPlanBranchApi.getByProject.mockRejectedValue({
      outcome: "commandError",
      error: "REMOTE_INTERNAL_ERROR: plan branches unavailable",
    });

    renderRail(createTask({ taskBranch: null, mergeCommitSha: null, completedAt: null }), {
      kind: "current",
    });

    expect(
      await screen.findByText("Plan, branch, and merge details could not be loaded."),
    ).toBeInTheDocument();
    expect(screen.queryByText("Plan branch")).not.toBeInTheDocument();
  });
});
