/**
 * running-processes transform tests — snake_case → camelCase conversion
 */

import { describe, it, expect } from "vitest";
import {
  transformExecutionCapacitySummary,
  transformExecutionLaneUsage,
  transformTeammateSummary,
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

describe("transformTeammateSummary", () => {
  it("transforms required fields only (name + status)", () => {
    const raw = { name: "coder-1", status: "running" };
    const result = transformTeammateSummary(raw);

    expect(result.name).toBe("coder-1");
    expect(result.status).toBe("running");
    expect(result).not.toHaveProperty("step");
    expect(result).not.toHaveProperty("model");
    expect(result).not.toHaveProperty("color");
    expect(result).not.toHaveProperty("stepsCompleted");
    expect(result).not.toHaveProperty("stepsTotal");
    expect(result).not.toHaveProperty("wave");
  });

  it("transforms all optional fields when present", () => {
    const raw = {
      name: "coder-2",
      status: "idle",
      step: "Implement auth",
      model: "sonnet",
      color: "#3b82f6",
      steps_completed: 3,
      steps_total: 8,
      wave: 2,
    };
    const result = transformTeammateSummary(raw);

    expect(result.name).toBe("coder-2");
    expect(result.step).toBe("Implement auth");
    expect(result.model).toBe("sonnet");
    expect(result.color).toBe("#3b82f6");
    expect(result.stepsCompleted).toBe(3);
    expect(result.stepsTotal).toBe(8);
    expect(result.wave).toBe(2);
  });

  it("renames stepsCompleted/stepsTotal/wave from snake_case", () => {
    const raw = {
      name: "coder-3",
      status: "running",
      steps_completed: 0,
      steps_total: 5,
      wave: 1,
    };
    const result = transformTeammateSummary(raw);

    expect(result.stepsCompleted).toBe(0);
    expect(result.stepsTotal).toBe(5);
    expect(result.wave).toBe(1);
    // Verify snake_case keys are NOT present in output
    expect(result).not.toHaveProperty("steps_completed");
    expect(result).not.toHaveProperty("steps_total");
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

  it("transforms team fields when present", () => {
    const raw = {
      ...baseRaw,
      team_name: "auth-team",
      teammates: [
        { name: "coder-1", status: "running" },
        { name: "coder-2", status: "idle" },
      ],
      current_wave: 1,
      total_waves: 3,
    };
    const result = transformRunningProcess(raw);

    expect(result.teamName).toBe("auth-team");
    expect(result.teammates).toHaveLength(2);
    expect(result.teammates![0]!.name).toBe("coder-1");
    expect(result.currentWave).toBe(1);
    expect(result.totalWaves).toBe(3);
  });

  it("omits team fields when not in raw data", () => {
    const result = transformRunningProcess(baseRaw);

    expect(result.taskId).toBe("task-1");
    expect(result.internalStatus).toBe("executing");
    expect(result).not.toHaveProperty("teamName");
    expect(result).not.toHaveProperty("teammates");
    expect(result).not.toHaveProperty("currentWave");
    expect(result).not.toHaveProperty("totalWaves");
  });

  it("handles empty teammates array", () => {
    const raw = {
      ...baseRaw,
      team_name: "empty-team",
      teammates: [],
      current_wave: 0,
      total_waves: 0,
    };
    const result = transformRunningProcess(raw);

    expect(result.teamName).toBe("empty-team");
    expect(result.teammates).toEqual([]);
    expect(result.currentWave).toBe(0);
    expect(result.totalWaves).toBe(0);
  });

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
