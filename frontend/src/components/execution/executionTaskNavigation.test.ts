import type { InfiniteData } from "@tanstack/react-query";
import { beforeEach, describe, expect, it } from "vitest";
import type { AgentSidebarConversationGroup } from "@/api/chat";
import { getQueryClient } from "@/lib/queryClient";
import type { Task } from "@/types/task";
import {
  mergePipelineTaskTarget,
  resolveExecutionTaskAgentWorkspace,
  runningProcessTaskTarget,
  taskRowNavigationTarget,
} from "./executionTaskNavigation";

function cacheSidebarRows(
  rows: AgentSidebarConversationGroup["rows"],
  key: readonly unknown[] = ["agents", "sidebar-conversations"],
) {
  const data: InfiniteData<AgentSidebarConversationGroup> = {
    pages: [
      {
        key: "active",
        label: "Active",
        total: rows.length,
        offset: 0,
        limit: 25,
        hasMore: false,
        rows,
      },
    ],
    pageParams: [0],
  };
  getQueryClient().setQueryData(key, data);
}

function row({
  conversationId,
  projectId = "project-1",
  title = "Agent Workspace",
  linkedPlanBranchId = null,
  linkedIdeationSessionId = null,
  archivedAt = null,
}: {
  conversationId: string;
  projectId?: string;
  title?: string | null;
  linkedPlanBranchId?: string | null;
  linkedIdeationSessionId?: string | null;
  archivedAt?: string | null;
}): AgentSidebarConversationGroup["rows"][number] {
  return {
    conversation: {
      id: conversationId,
      title,
      archivedAt,
    },
    workspace: {
      conversationId,
      projectId,
      linkedPlanBranchId,
      linkedIdeationSessionId,
    },
  } as AgentSidebarConversationGroup["rows"][number];
}

function task(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    category: "feature",
    title: "Task",
    description: null,
    priority: 1,
    internalStatus: "ready",
    needsReviewPoint: false,
    createdAt: "2026-07-08T00:00:00Z",
    updatedAt: "2026-07-08T00:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    ...overrides,
  };
}

describe("executionTaskNavigation", () => {
  beforeEach(() => {
    getQueryClient().clear();
  });

  it("builds navigation targets for each execution task source", () => {
    const agentWorkspace = {
      conversationId: "conversation-1",
      projectId: "project-1",
      title: "Workspace",
    };

    expect(
      runningProcessTaskTarget({
        taskId: "running-task",
        title: "Running task",
        internalStatus: "executing",
        stepProgress: null,
        elapsedSeconds: null,
        triggerOrigin: null,
        taskBranch: null,
        agentWorkspace,
      }),
    ).toEqual({
      taskId: "running-task",
      source: "running",
      projectId: "project-1",
      agentWorkspace,
    });

    expect(
      mergePipelineTaskTarget({
        taskId: "merge-task",
        title: "Merge",
        internalStatus: "pending_merge",
        sourceBranch: "feature",
        targetBranch: "main",
        isDeferred: false,
        isMainMergeDeferred: false,
        blockingBranch: null,
        conflictFiles: null,
        errorContext: null,
        agentWorkspace,
      }),
    ).toEqual({
      taskId: "merge-task",
      source: "merge",
      projectId: "project-1",
      agentWorkspace,
    });

    expect(
      taskRowNavigationTarget(
        task({
          id: "queued-task",
          ideationSessionId: "session-1",
          executionPlanId: "plan-branch-1",
        }),
        "queued",
      ),
    ).toEqual({
      taskId: "queued-task",
      source: "queued",
      projectId: "project-1",
      ideationSessionId: "session-1",
      executionPlanId: "plan-branch-1",
    });
  });

  it("uses an explicit backend agent workspace without reading sidebar cache", () => {
    const agentWorkspace = {
      conversationId: "conversation-explicit",
      projectId: "project-1",
      title: "Explicit Workspace",
    };

    expect(
      resolveExecutionTaskAgentWorkspace({
        taskId: "task-1",
        source: "running",
        agentWorkspace,
      }),
    ).toBe(agentWorkspace);
  });

  it("resolves cached Agent workspace by plan branch before session", () => {
    cacheSidebarRows([
      row({
        conversationId: "conversation-session",
        title: "Session Workspace",
        linkedIdeationSessionId: "session-1",
      }),
      row({
        conversationId: "conversation-plan",
        title: "Plan Workspace",
        linkedPlanBranchId: "plan-branch-1",
        linkedIdeationSessionId: "session-1",
      }),
    ]);

    expect(
      resolveExecutionTaskAgentWorkspace({
        taskId: "task-1",
        source: "queued",
        projectId: "project-1",
        executionPlanId: "plan-branch-1",
        ideationSessionId: "session-1",
      }),
    ).toEqual({
      conversationId: "conversation-plan",
      projectId: "project-1",
      title: "Plan Workspace",
    });
  });

  it("skips archived and cross-project cached conversations", () => {
    cacheSidebarRows([
      row({
        conversationId: "conversation-archived",
        title: "Archived Workspace",
        linkedIdeationSessionId: "session-1",
        archivedAt: "2026-07-08T00:00:00Z",
      }),
      row({
        conversationId: "conversation-other-project",
        projectId: "project-2",
        title: "Other Project Workspace",
        linkedIdeationSessionId: "session-1",
      }),
      row({
        conversationId: "conversation-active",
        title: null,
        linkedIdeationSessionId: "session-1",
      }),
    ]);

    expect(
      resolveExecutionTaskAgentWorkspace({
        taskId: "task-1",
        source: "paused",
        projectId: "project-1",
        ideationSessionId: "session-1",
      }),
    ).toEqual({
      conversationId: "conversation-active",
      projectId: "project-1",
      title: "Agent conversation",
    });
  });

  it("returns null when no workspace link is available", () => {
    expect(
      resolveExecutionTaskAgentWorkspace({
        taskId: "task-1",
        source: "queued",
      }),
    ).toBeNull();

    cacheSidebarRows([]);
    expect(
      resolveExecutionTaskAgentWorkspace({
        taskId: "task-1",
        source: "queued",
        projectId: "project-1",
        ideationSessionId: "missing-session",
      }),
    ).toBeNull();
  });
});
