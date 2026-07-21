import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  createTaskProposal,
  updateTaskProposal,
  deleteTaskProposal,
  reorderProposals,
  assessProposalPriority,
  assessAllPriorities,
  addProposalDependency,
  removeProposalDependency,
  analyzeDependencies,
  applyProposalsToKanban,
  proposalApi,
} from "./proposal";

// Cast invoke to a mock function for testing
const mockInvoke = invoke as ReturnType<typeof vi.fn>;

// Helper to create mock task proposal (snake_case - matches Rust backend)
const createMockProposalRaw = (overrides = {}) => ({
  id: "proposal-1",
  session_id: "session-1",
  title: "Test Proposal",
  description: null,
  category: "feature",
  steps: [],
  acceptance_criteria: [],
  suggested_priority: "medium",
  priority_score: 50,
  priority_reason: null,
  estimated_complexity: "moderate",
  user_priority: null,
  user_modified: false,
  status: "pending",
  created_task_id: null,
  plan_artifact_id: null,
  plan_version_at_creation: null,
  sort_order: 0,
  created_at: "2026-01-24T12:00:00Z",
  updated_at: "2026-01-24T12:00:00Z",
  ...overrides,
});

describe("createTaskProposal", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call create_task_proposal with sessionId and data", async () => {
    const proposal = createMockProposalRaw();
    mockInvoke.mockResolvedValue(proposal);

    await createTaskProposal("session-1", {
      title: "New Feature",
      category: "feature",
    });

    expect(mockInvoke).toHaveBeenCalledWith("create_task_proposal", {
      input: {
        session_id: "session-1",
        title: "New Feature",
        category: "feature",
        description: undefined,
        steps: undefined,
        acceptance_criteria: undefined,
        priority: undefined,
        complexity: undefined,
      },
    });
  });

  it("should pass all optional fields", async () => {
    const proposal = createMockProposalRaw();
    mockInvoke.mockResolvedValue(proposal);

    await createTaskProposal("session-1", {
      title: "New Feature",
      category: "feature",
      description: "A description",
      steps: ["Step 1", "Step 2"],
      acceptanceCriteria: ["AC1"],
      priority: "high",
      complexity: "complex",
    });

    expect(mockInvoke).toHaveBeenCalledWith("create_task_proposal", {
      input: {
        session_id: "session-1",
        title: "New Feature",
        category: "feature",
        description: "A description",
        steps: ["Step 1", "Step 2"],
        acceptance_criteria: ["AC1"],
        priority: "high",
        complexity: "complex",
      },
    });
  });

  it("should return created proposal with camelCase fields", async () => {
    const proposal = createMockProposalRaw({
      title: "Created Proposal",
      suggested_priority: "high",
      priority_score: 75,
    });
    mockInvoke.mockResolvedValue(proposal);

    const result = await createTaskProposal("session-1", {
      title: "Created Proposal",
      category: "feature",
    });

    expect(result.title).toBe("Created Proposal");
    expect(result.suggestedPriority).toBe("high");
    expect(result.priorityScore).toBe(75);
    expect(result.sessionId).toBe("session-1");
  });

  it("should validate proposal schema", async () => {
    mockInvoke.mockResolvedValue({ invalid: "proposal" });

    await expect(
      createTaskProposal("session-1", { title: "Test", category: "feature" })
    ).rejects.toThrow();
  });
});

describe("updateTaskProposal", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call update_task_proposal with proposalId and changes", async () => {
    const proposal = createMockProposalRaw({ title: "Updated" });
    mockInvoke.mockResolvedValue(proposal);

    await updateTaskProposal("proposal-1", { title: "Updated" });

    expect(mockInvoke).toHaveBeenCalledWith("update_task_proposal", {
      id: "proposal-1",
      input: {
        title: "Updated",
        description: undefined,
        category: undefined,
        steps: undefined,
        acceptance_criteria: undefined,
        user_priority: undefined,
        complexity: undefined,
      },
    });
  });

  it("should pass all fields when provided", async () => {
    const proposal = createMockProposalRaw();
    mockInvoke.mockResolvedValue(proposal);

    await updateTaskProposal("proposal-1", {
      title: "Updated",
      description: "New desc",
      category: "setup",
      steps: ["New step"],
      acceptanceCriteria: ["New AC"],
      userPriority: "critical",
      complexity: "simple",
    });

    expect(mockInvoke).toHaveBeenCalledWith("update_task_proposal", {
      id: "proposal-1",
      input: {
        title: "Updated",
        description: "New desc",
        category: "setup",
        steps: ["New step"],
        acceptance_criteria: ["New AC"],
        user_priority: "critical",
        complexity: "simple",
      },
    });
  });

  it("should return updated proposal with userModified flag", async () => {
    const proposal = createMockProposalRaw({
      title: "Updated Title",
      user_modified: true,
    });
    mockInvoke.mockResolvedValue(proposal);

    const result = await updateTaskProposal("proposal-1", {
      title: "Updated Title",
    });

    expect(result.title).toBe("Updated Title");
    expect(result.userModified).toBe(true);
  });
});

describe("deleteTaskProposal", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call delete_task_proposal with proposalId", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await deleteTaskProposal("proposal-1");

    expect(mockInvoke).toHaveBeenCalledWith("delete_task_proposal", {
      id: "proposal-1",
    });
  });

  it("should propagate errors", async () => {
    mockInvoke.mockRejectedValue(new Error("Proposal not found"));

    await expect(deleteTaskProposal("nonexistent")).rejects.toThrow(
      "Proposal not found"
    );
  });
});

describe("reorderProposals", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call reorder_proposals with sessionId and proposalIds", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await reorderProposals("session-1", ["p1", "p2", "p3"]);

    expect(mockInvoke).toHaveBeenCalledWith("reorder_proposals", {
      sessionId: "session-1",
      proposalIds: ["p1", "p2", "p3"],
    });
  });

  it("should handle empty array", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await reorderProposals("session-1", []);

    expect(mockInvoke).toHaveBeenCalledWith("reorder_proposals", {
      sessionId: "session-1",
      proposalIds: [],
    });
  });
});

describe("assessProposalPriority", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call assess_proposal_priority with proposalId", async () => {
    mockInvoke.mockResolvedValue({
      proposal_id: "proposal-1",
      priority: "high",
      score: 75,
      reason: "Blocks 2 tasks",
    });

    const result = await assessProposalPriority("proposal-1");

    expect(mockInvoke).toHaveBeenCalledWith("assess_proposal_priority", {
      id: "proposal-1",
    });
    expect(result.priority).toBe("high");
    expect(result.score).toBe(75);
    expect(result.proposalId).toBe("proposal-1");
  });

  it("should transform snake_case to camelCase", async () => {
    mockInvoke.mockResolvedValue({
      proposal_id: "p1",
      priority: "critical",
      score: 90,
      reason: "Critical path item",
    });

    const result = await assessProposalPriority("p1");

    expect(result.proposalId).toBe("p1");
    expect(result.reason).toBe("Critical path item");
  });
});

describe("assessAllPriorities", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call assess_all_priorities with sessionId", async () => {
    mockInvoke.mockResolvedValue([
      { proposal_id: "p1", priority: "high", score: 80, reason: "Reason 1" },
      { proposal_id: "p2", priority: "low", score: 30, reason: "Reason 2" },
    ]);

    const result = await assessAllPriorities("session-1");

    expect(mockInvoke).toHaveBeenCalledWith("assess_all_priorities", {
      sessionId: "session-1",
    });
    expect(result).toHaveLength(2);
    expect(result[0]?.proposalId).toBe("p1");
    expect(result[1]?.proposalId).toBe("p2");
  });

  it("should return empty array when no proposals", async () => {
    mockInvoke.mockResolvedValue([]);

    const result = await assessAllPriorities("session-1");

    expect(result).toEqual([]);
  });
});

describe("addProposalDependency", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call add_proposal_dependency with both IDs", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await addProposalDependency("proposal-1", "proposal-2");

    expect(mockInvoke).toHaveBeenCalledWith("add_proposal_dependency", {
      proposalId: "proposal-1",
      dependsOnId: "proposal-2",
    });
  });

  it("should propagate errors on self-dependency", async () => {
    mockInvoke.mockRejectedValue(new Error("Self-dependency not allowed"));

    await expect(addProposalDependency("p1", "p1")).rejects.toThrow(
      "Self-dependency not allowed"
    );
  });
});

describe("removeProposalDependency", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call remove_proposal_dependency with both IDs", async () => {
    mockInvoke.mockResolvedValue(undefined);

    await removeProposalDependency("proposal-1", "proposal-2");

    expect(mockInvoke).toHaveBeenCalledWith("remove_proposal_dependency", {
      proposalId: "proposal-1",
      dependsOnId: "proposal-2",
    });
  });
});

describe("analyzeDependencies", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call analyze_dependencies with sessionId", async () => {
    const graph = {
      nodes: [{ proposal_id: "p1", title: "P1", in_degree: 0, out_degree: 1 }],
      edges: [{ from: "p1", to: "p2", reason: null }],
      critical_path: ["p1", "p2"],
      has_cycles: false,
      cycles: null,
    };
    mockInvoke.mockResolvedValue(graph);

    const result = await analyzeDependencies("session-1");

    expect(mockInvoke).toHaveBeenCalledWith("analyze_dependencies", {
      sessionId: "session-1",
    });
    expect(result.hasCycles).toBe(false);
    expect(result.criticalPath).toEqual(["p1", "p2"]);
  });

  it("should transform nodes to camelCase", async () => {
    const graph = {
      nodes: [{ proposal_id: "p1", title: "Task 1", in_degree: 2, out_degree: 3 }],
      edges: [],
      critical_path: [],
      has_cycles: false,
      cycles: null,
    };
    mockInvoke.mockResolvedValue(graph);

    const result = await analyzeDependencies("session-1");

    expect(result.nodes[0]?.proposalId).toBe("p1");
    expect(result.nodes[0]?.inDegree).toBe(2);
    expect(result.nodes[0]?.outDegree).toBe(3);
  });

  it("should handle cycles", async () => {
    const graph = {
      nodes: [],
      edges: [],
      critical_path: [],
      has_cycles: true,
      cycles: [["p1", "p2", "p3"]],
    };
    mockInvoke.mockResolvedValue(graph);

    const result = await analyzeDependencies("session-1");

    expect(result.hasCycles).toBe(true);
    expect(result.cycles).toEqual([["p1", "p2", "p3"]]);
  });

  it("should handle empty graph", async () => {
    const graph = {
      nodes: [],
      edges: [],
      critical_path: [],
      has_cycles: false,
      cycles: null,
    };
    mockInvoke.mockResolvedValue(graph);

    const result = await analyzeDependencies("session-1");

    expect(result.nodes).toEqual([]);
    expect(result.edges).toEqual([]);
  });
});

describe("applyProposalsToKanban", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call apply_proposals_to_kanban with options", async () => {
    mockInvoke.mockResolvedValue({
      created_task_ids: ["task-1", "task-2"],
      dependencies_created: 1,
      warnings: [],
      session_converted: false,
    });

    await applyProposalsToKanban({
      sessionId: "session-1",
      proposalIds: ["p1", "p2"],
      targetColumn: "backlog",
    });

    expect(mockInvoke).toHaveBeenCalledWith("apply_proposals_to_kanban", {
      input: {
        session_id: "session-1",
        proposal_ids: ["p1", "p2"],
        target_column: "backlog",
      },
    });
  });

  it("should support all target columns", async () => {
    const result = {
      created_task_ids: [],
      dependencies_created: 0,
      warnings: [],
      session_converted: false,
    };
    mockInvoke.mockResolvedValue(result);

    for (const column of ["draft", "backlog", "todo"] as const) {
      await applyProposalsToKanban({
        sessionId: "s1",
        proposalIds: ["p1"],
        targetColumn: column,
      });
    }

    expect(mockInvoke).toHaveBeenCalledTimes(3);
  });

  it("should return apply result with camelCase fields", async () => {
    mockInvoke.mockResolvedValue({
      created_task_ids: ["task-1", "task-2"],
      dependencies_created: 1,
      warnings: ["Some dep not preserved"],
      session_converted: true,
    });

    const result = await applyProposalsToKanban({
      sessionId: "session-1",
      proposalIds: ["p1", "p2"],
      targetColumn: "todo",
    });

    expect(result.createdTaskIds).toEqual(["task-1", "task-2"]);
    expect(result.dependenciesCreated).toBe(1);
    expect(result.warnings).toHaveLength(1);
    expect(result.sessionConverted).toBe(true);
  });

  it("should handle empty proposal selection", async () => {
    mockInvoke.mockResolvedValue({
      created_task_ids: [],
      dependencies_created: 0,
      warnings: [],
      session_converted: false,
    });

    const result = await applyProposalsToKanban({
      sessionId: "session-1",
      proposalIds: [],
      targetColumn: "draft",
    });

    expect(result.createdTaskIds).toEqual([]);
  });
});

describe("proposalApi namespace", () => {
  it("should export all functions", () => {
    expect(proposalApi.createTaskProposal).toBe(createTaskProposal);
    expect(proposalApi.updateTaskProposal).toBe(updateTaskProposal);
    expect(proposalApi.deleteTaskProposal).toBe(deleteTaskProposal);
    expect(proposalApi.reorderProposals).toBe(reorderProposals);
    expect(proposalApi.assessProposalPriority).toBe(assessProposalPriority);
    expect(proposalApi.assessAllPriorities).toBe(assessAllPriorities);
    expect(proposalApi.addProposalDependency).toBe(addProposalDependency);
    expect(proposalApi.removeProposalDependency).toBe(removeProposalDependency);
    expect(proposalApi.analyzeDependencies).toBe(analyzeDependencies);
    expect(proposalApi.applyProposalsToKanban).toBe(applyProposalsToKanban);
  });

  it("should have 10 functions", () => {
    expect(Object.keys(proposalApi)).toHaveLength(10);
  });
});
