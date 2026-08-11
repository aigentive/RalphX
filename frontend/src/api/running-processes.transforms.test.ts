/**
 * running-processes transform tests — snake_case → camelCase conversion
 */

import { describe, it, expect } from "vitest";
import {
  transformExecutionCapacitySummary,
  transformExecutionLaneUsage,
  transformRunningProcessesResponse,
  transformRunningProcess,
  transformRunningWorkspaceSession,
} from "./running-processes.transforms";
import { transformExecutionTaskAgentWorkspace } from "./execution-task-agent-workspace";

describe("transformExecutionTaskAgentWorkspace", () => {
  it("renames Agent workspace target fields from snake_case", () => {
    expect(
      transformExecutionTaskAgentWorkspace({
        conversation_id: "conversation-1",
        project_id: "project-1",
        title: "Agent Workspace",
      }),
    ).toEqual({
      conversationId: "conversation-1",
      projectId: "project-1",
      title: "Agent Workspace",
    });
  });
});


describe("transformRunningProcess", () => {
  const baseRaw = {
    task_id: "task-1",
    title: "Auth feature",
    internal_status: "executing",
    step_progress: null,
    elapsed_seconds: 120,
    trigger_origin: "user",
    task_branch: "task/auth-feature",
  };


  it("transforms optional Agent workspace target when present", () => {
    const result = transformRunningProcess({
      ...baseRaw,
      agent_workspace: {
        conversation_id: "conversation-1",
        project_id: "project-1",
        title: "Agent Workspace",
      },
    });

    expect(result.agentWorkspace).toEqual({
      conversationId: "conversation-1",
      projectId: "project-1",
      title: "Agent Workspace",
    });
  });
});

describe("transformRunningWorkspaceSession", () => {
  it("renames workspace fields from snake_case", () => {
    const result = transformRunningWorkspaceSession({
      conversation_id: "conversation-1",
      project_id: "project-1",
      automation_id: "automation-1",
      automation_run_id: "run-1",
      title: "Workspace run",
      elapsed_seconds: 30,
      model: "gpt-5.5",
    });

    expect(result.conversationId).toBe("conversation-1");
    expect(result.projectId).toBe("project-1");
    expect(result.automationId).toBe("automation-1");
    expect(result.automationRunId).toBe("run-1");
    expect(result.elapsedSeconds).toBe(30);
    expect(result.model).toBe("gpt-5.5");
  });
});

describe("transformExecutionLaneUsage", () => {
  it("renames lane capacity fields from snake_case", () => {
    const result = transformExecutionLaneUsage({
      lane: "workspaces",
      active: 3,
      idle: 0,
      waiting: 2,
      max: 10,
      borrowed: 1,
      priority_rank: 1,
    });

    expect(result.lane).toBe("workspaces");
    expect(result.priorityRank).toBe(1);
    expect(result.borrowed).toBe(1);
  });
});

describe("transformExecutionCapacitySummary", () => {
  it("renames capacity fields from snake_case", () => {
    const result = transformExecutionCapacitySummary({
      total_active: 5,
      global_max_concurrent: 20,
      borrowing_enabled: true,
      priority: ["workspaces", "tasks", "ideation"],
    });

    expect(result.totalActive).toBe(5);
    expect(result.globalMaxConcurrent).toBe(20);
    expect(result.borrowingEnabled).toBe(true);
  });
});

describe("transformRunningProcessesResponse", () => {
  it("includes workspace, lane, and capacity data", () => {
    const result = transformRunningProcessesResponse({
      processes: [],
      ideation_sessions: [],
      workspace_sessions: [
        {
          conversation_id: "conversation-1",
          project_id: "project-1",
          automation_id: null,
          automation_run_id: null,
          title: "Workspace run",
          elapsed_seconds: null,
          model: null,
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
          priority_rank: 1,
        },
      ],
      capacity: {
        total_active: 1,
        global_max_concurrent: 20,
        borrowing_enabled: false,
        priority: ["workspaces", "tasks", "ideation"],
      },
    });

    expect(result.workspaceSessions).toHaveLength(1);
    expect(result.lanes[0]?.priorityRank).toBe(1);
    expect(result.capacity.priority).toEqual(["workspaces", "tasks", "ideation"]);
  });
});
