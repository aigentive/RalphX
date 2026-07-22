import { beforeEach, describe, expect, it, vi } from "vitest";

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

import { tasksApi } from "./tasks";

describe("tasksApi", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("pauses an execution plan through the backend command", async () => {
    mockInvoke.mockResolvedValueOnce({
      execution_plan_id: "exec-plan-1",
      affected_count: 2,
    });

    await expect(
      tasksApi.pauseExecutionPlan({
        projectId: "project-1",
        sessionId: "session-1",
      }),
    ).resolves.toEqual({
      executionPlanId: "exec-plan-1",
      affectedCount: 2,
    });

    expect(mockInvoke).toHaveBeenCalledWith("pause_execution_plan", {
      input: {
        projectId: "project-1",
        sessionId: "session-1",
        executionPlanId: null,
      },
    });
  });

  it("resumes an execution plan through the backend command", async () => {
    mockInvoke.mockResolvedValueOnce({
      execution_plan_id: "exec-plan-2",
      affected_count: 1,
    });

    await expect(
      tasksApi.resumeExecutionPlan({
        projectId: "project-1",
        sessionId: "session-1",
        executionPlanId: "exec-plan-2",
      }),
    ).resolves.toEqual({
      executionPlanId: "exec-plan-2",
      affectedCount: 1,
    });

    expect(mockInvoke).toHaveBeenCalledWith("resume_execution_plan", {
      input: {
        projectId: "project-1",
        sessionId: "session-1",
        executionPlanId: "exec-plan-2",
      },
    });
  });

  it("stops an execution plan through the backend command", async () => {
    mockInvoke.mockResolvedValueOnce({
      execution_plan_id: "exec-plan-3",
      affected_count: 3,
    });

    await expect(
      tasksApi.stopExecutionPlan({
        projectId: "project-1",
        sessionId: "session-1",
        executionPlanId: "exec-plan-3",
      }),
    ).resolves.toEqual({
      executionPlanId: "exec-plan-3",
      affectedCount: 3,
    });

    expect(mockInvoke).toHaveBeenCalledWith("stop_execution_plan", {
      input: {
        projectId: "project-1",
        sessionId: "session-1",
        executionPlanId: "exec-plan-3",
      },
    });
  });

  it("resolves a task's owning Agent conversation workspace", async () => {
    mockInvoke.mockResolvedValueOnce({
      conversation_id: "conversation-1",
      project_id: "project-1",
      title: "Task owner",
    });

    await expect(tasksApi.resolveAgentWorkspace("task-1")).resolves.toEqual({
      conversationId: "conversation-1",
      projectId: "project-1",
      title: "Task owner",
    });

    expect(mockInvoke).toHaveBeenCalledWith("get_task_agent_workspace", {
      taskId: "task-1",
    });
  });

  it("restarts a task with a note", async () => {
    mockInvoke.mockResolvedValueOnce({
      type: "Success",
      task: { id: "task-1" },
      category: "direct",
      resumed_to_status: "ready",
      disposition: "restarted_to_ready",
    });

    await expect(
      tasksApi.restart("task-1", false, "try again"),
    ).resolves.toEqual({
      type: "Success",
      task: { id: "task-1" },
      category: "direct",
      resumedToStatus: "ready",
      disposition: "restarted_to_ready",
    });

    expect(mockInvoke).toHaveBeenCalledWith("restart_task", {
      taskId: "task-1",
      force: false,
      note: "try again",
    });
  });
});
