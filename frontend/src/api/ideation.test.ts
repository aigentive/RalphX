import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { backendApiUrl } from "./backend";
import { ideationApi } from "./ideation";
import {
  VerificationResponseSchema,
  CreateChildSessionResponseSchema,
} from "./ideation.schemas";

// Cast invoke to a mock function for testing
const mockInvoke = invoke as ReturnType<typeof vi.fn>;

// Helper to create mock ideation session (snake_case - matches Rust backend)
const createMockSessionRaw = (overrides = {}) => ({
  id: "session-1",
  project_id: "project-1",
  title: null,
  status: "active",
  plan_artifact_id: null,
  parent_session_id: null,
  created_at: "2026-01-24T12:00:00Z",
  updated_at: "2026-01-24T12:00:00Z",
  archived_at: null,
  converted_at: null,
  ...overrides,
});

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

// Helper to create mock chat message (snake_case - matches Rust backend)
const createMockMessageRaw = (overrides = {}) => ({
  id: "message-1",
  session_id: "session-1",
  project_id: null,
  task_id: null,
  role: "user",
  content: "Hello",
  metadata: null,
  tool_calls: null,
  parent_message_id: null,
  created_at: "2026-01-24T12:00:00Z",
  ...overrides,
});

describe("ideationApi.sessions", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("create", () => {
    it("should call create_ideation_session with project_id and title", async () => {
      const session = createMockSessionRaw({ title: "My Session" });
      mockInvoke.mockResolvedValue(session);

      await ideationApi.sessions.create("project-1", "My Session");

      expect(mockInvoke).toHaveBeenCalledWith("create_ideation_session", {
        input: { project_id: "project-1", title: "My Session", seed_task_id: undefined },
      });
    });

    it("should call create_ideation_session with just project_id", async () => {
      const session = createMockSessionRaw();
      mockInvoke.mockResolvedValue(session);

      await ideationApi.sessions.create("project-1");

      expect(mockInvoke).toHaveBeenCalledWith("create_ideation_session", {
        input: { project_id: "project-1", title: undefined, seed_task_id: undefined },
      });
    });

    it("should return created session", async () => {
      const session = createMockSessionRaw({ title: "New Session" });
      mockInvoke.mockResolvedValue(session);

      const result = await ideationApi.sessions.create("project-1", "New Session");

      expect(result.title).toBe("New Session");
      expect(result.status).toBe("active");
    });

    it("should preserve follow-up provenance fields", async () => {
      mockInvoke.mockResolvedValue(
        createMockSessionRaw({
          source_project_id: "project-source",
          source_session_id: "session-source",
          source_task_id: "task-123",
          source_context_type: "task_execution",
          source_context_id: "task-123",
          spawn_reason: "out_of_scope_failure",
        })
      );

      const result = await ideationApi.sessions.create("project-1");

      expect(result.sourceProjectId).toBe("project-source");
      expect(result.sourceSessionId).toBe("session-source");
      expect(result.sourceTaskId).toBe("task-123");
      expect(result.sourceContextType).toBe("task_execution");
      expect(result.sourceContextId).toBe("task-123");
      expect(result.spawnReason).toBe("out_of_scope_failure");
    });

    it("should validate session schema", async () => {
      mockInvoke.mockResolvedValue({ invalid: "session" });

      await expect(ideationApi.sessions.create("project-1")).rejects.toThrow();
    });


    it("sends analysis base selection when provided", async () => {
      const session = createMockSessionRaw();
      mockInvoke.mockResolvedValue(session);

      await ideationApi.sessions.create(
        "project-1",
        "Title",
        undefined,
        {
          kind: "current_branch",
          ref: "feature/current",
          displayName: "Current branch (feature/current)",
        },
      );

      expect(mockInvoke).toHaveBeenCalledWith("create_ideation_session", {
        input: expect.objectContaining({
          analysis_base_ref_kind: "current_branch",
          analysis_base_ref: "feature/current",
          analysis_base_display_name: "Current branch (feature/current)",
        }),
      });
    });
  });

  describe("get", () => {
    it("should call get_ideation_session with id", async () => {
      const session = createMockSessionRaw();
      mockInvoke.mockResolvedValue(session);

      await ideationApi.sessions.get("session-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_ideation_session", {
        id: "session-1",
      });
    });

    it("should return session when found", async () => {
      const session = createMockSessionRaw({ title: "Found Session" });
      mockInvoke.mockResolvedValue(session);

      const result = await ideationApi.sessions.get("session-1");

      expect(result?.title).toBe("Found Session");
    });

    it("should return null when not found", async () => {
      mockInvoke.mockResolvedValue(null);

      const result = await ideationApi.sessions.get("nonexistent");

      expect(result).toBeNull();
    });
  });

  describe("resolveAgentWorkspace", () => {
    it("resolves the linked Agent workspace through the typed command", async () => {
      mockInvoke.mockResolvedValue({
        conversation_id: "conversation-1",
        project_id: "project-1",
        title: "Agent workspace",
      });

      await expect(
        ideationApi.sessions.resolveAgentWorkspace("session-1"),
      ).resolves.toEqual({
        conversationId: "conversation-1",
        projectId: "project-1",
        title: "Agent workspace",
      });
      expect(mockInvoke).toHaveBeenCalledWith("get_ideation_agent_workspace", {
        sessionId: "session-1",
      });
    });

    it("returns null when no active linked workspace exists", async () => {
      mockInvoke.mockResolvedValue(null);

      await expect(
        ideationApi.sessions.resolveAgentWorkspace("missing-session"),
      ).resolves.toBeNull();
    });
  });

  describe("getWithData", () => {
    it("should call get_ideation_session_with_data with id", async () => {
      const data = {
        session: createMockSessionRaw(),
        proposals: [],
        messages: [],
      };
      mockInvoke.mockResolvedValue(data);

      await ideationApi.sessions.getWithData("session-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_ideation_session_with_data", {
        id: "session-1",
      });
    });

    it("should return session with proposals and messages", async () => {
      const data = {
        session: createMockSessionRaw(),
        proposals: [createMockProposalRaw()],
        messages: [createMockMessageRaw()],
      };
      mockInvoke.mockResolvedValue(data);

      const result = await ideationApi.sessions.getWithData("session-1");

      expect(result?.proposals).toHaveLength(1);
      expect(result?.messages).toHaveLength(1);
    });

    it("should return null when session not found", async () => {
      mockInvoke.mockResolvedValue(null);

      const result = await ideationApi.sessions.getWithData("nonexistent");

      expect(result).toBeNull();
    });
  });

  describe("getLatestChildSessionId", () => {
    it("should call get_latest_child_session_id with purpose and archive options", async () => {
      mockInvoke.mockResolvedValue({
        session_id: "session-1",
        purpose: "verification",
        latest_child_session_id: "child-1",
      });

      const result = await ideationApi.sessions.getLatestChildSessionId(
        "session-1",
        "verification",
        { includeArchived: true },
      );

      expect(mockInvoke).toHaveBeenCalledWith("get_latest_child_session_id", {
        sessionId: "session-1",
        purpose: "verification",
        includeArchived: true,
      });
      expect(result).toEqual({
        sessionId: "session-1",
        purpose: "verification",
        latestChildSessionId: "child-1",
      });
    });
  });

  describe("list", () => {
    it("should call list_ideation_sessions with project_id", async () => {
      mockInvoke.mockResolvedValue([createMockSessionRaw()]);

      await ideationApi.sessions.list("project-1");

      expect(mockInvoke).toHaveBeenCalledWith("list_ideation_sessions", {
        projectId: "project-1",
        purpose: "general",
      });
    });

    it("should return array of sessions", async () => {
      const sessions = [
        createMockSessionRaw({ id: "s1" }),
        createMockSessionRaw({ id: "s2", title: "Session 2" }),
      ];
      mockInvoke.mockResolvedValue(sessions);

      const result = await ideationApi.sessions.list("project-1");

      expect(result).toHaveLength(2);
      expect(result[0]?.id).toBe("s1");
      expect(result[1]?.title).toBe("Session 2");
    });

    it("should return empty array when no sessions", async () => {
      mockInvoke.mockResolvedValue([]);

      const result = await ideationApi.sessions.list("project-1");

      expect(result).toEqual([]);
    });
  });

  describe("listByGroup", () => {
    it("should call list_sessions_by_group and transform sessions", async () => {
      mockInvoke.mockResolvedValue({
        sessions: [
          {
            ...createMockSessionRaw({
              id: "archived-session",
              title: "Archived agent",
              status: "archived",
              archived_at: "2026-04-22T12:00:00Z",
            }),
            progress: null,
            parentSessionTitle: null,
            verificationChildCount: 2,
            hasPendingPrompt: false,
          },
        ],
        total: 1,
        hasMore: false,
        offset: 0,
      });

      const result = await ideationApi.sessions.listByGroup(
        "project-1",
        "archived"
      );

      expect(mockInvoke).toHaveBeenCalledWith("list_sessions_by_group", {
        projectId: "project-1",
        group: "archived",
        offset: 0,
        limit: 200,
      });
      expect(result.sessions[0]?.id).toBe("archived-session");
      expect(result.sessions[0]?.title).toBe("Archived agent");
      expect(result.sessions[0]?.archivedAt).toBe("2026-04-22T12:00:00Z");
      expect(result.sessions[0]?.verificationChildCount).toBe(2);
      expect(result.hasMore).toBe(false);
    });
  });

  describe("archive", () => {
    it("should call archive_ideation_session with id", async () => {
      mockInvoke.mockResolvedValue(undefined);

      await ideationApi.sessions.archive("session-1");

      expect(mockInvoke).toHaveBeenCalledWith("archive_ideation_session", {
        id: "session-1",
      });
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("Session not found"));

      await expect(ideationApi.sessions.archive("nonexistent")).rejects.toThrow(
        "Session not found"
      );
    });
  });

  describe("restartImplementation", () => {
    it("should call restart_ideation_implementation and transform the result", async () => {
      mockInvoke.mockResolvedValue({
        session_id: "session-1",
        old_execution_plan_id: "exec-old",
        execution_plan_id: "exec-new",
        archived_task_count: 2,
        created_task_ids: ["task-1", "task-2"],
      });

      const result = await ideationApi.sessions.restartImplementation("session-1");

      expect(mockInvoke).toHaveBeenCalledWith("restart_ideation_implementation", {
        sessionId: "session-1",
      });
      expect(result).toEqual({
        sessionId: "session-1",
        oldExecutionPlanId: "exec-old",
        executionPlanId: "exec-new",
        archivedTaskCount: 2,
        createdTaskIds: ["task-1", "task-2"],
      });
    });
  });

});


describe("ideationApi.proposals", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("create", () => {
    it("should call create_task_proposal with input", async () => {
      const proposal = createMockProposalRaw();
      mockInvoke.mockResolvedValue(proposal);

      await ideationApi.proposals.create({
        sessionId: "session-1",
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

      await ideationApi.proposals.create({
        sessionId: "session-1",
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

    it("should return created proposal", async () => {
      const proposal = createMockProposalRaw({ title: "Created Proposal" });
      mockInvoke.mockResolvedValue(proposal);

      const result = await ideationApi.proposals.create({
        sessionId: "session-1",
        title: "Created Proposal",
        category: "feature",
      });

      expect(result.title).toBe("Created Proposal");
    });
  });

  describe("get", () => {
    it("should call get_task_proposal with id", async () => {
      const proposal = createMockProposalRaw();
      mockInvoke.mockResolvedValue(proposal);

      await ideationApi.proposals.get("proposal-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_task_proposal", {
        id: "proposal-1",
      });
    });

    it("should return null when not found", async () => {
      mockInvoke.mockResolvedValue(null);

      const result = await ideationApi.proposals.get("nonexistent");

      expect(result).toBeNull();
    });
  });

  describe("list", () => {
    it("should call list_session_proposals with session_id", async () => {
      mockInvoke.mockResolvedValue([createMockProposalRaw()]);

      await ideationApi.proposals.list("session-1");

      expect(mockInvoke).toHaveBeenCalledWith("list_session_proposals", {
        sessionId: "session-1",
      });
    });

    it("should return array of proposals", async () => {
      const proposals = [
        createMockProposalRaw({ id: "p1" }),
        createMockProposalRaw({ id: "p2", title: "Proposal 2" }),
      ];
      mockInvoke.mockResolvedValue(proposals);

      const result = await ideationApi.proposals.list("session-1");

      expect(result).toHaveLength(2);
    });
  });

  describe("update", () => {
    it("should call update_task_proposal with id and input", async () => {
      const proposal = createMockProposalRaw({ title: "Updated" });
      mockInvoke.mockResolvedValue(proposal);

      await ideationApi.proposals.update("proposal-1", { title: "Updated" });

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

    it("should return updated proposal", async () => {
      const proposal = createMockProposalRaw({ title: "Updated Title", user_modified: true });
      mockInvoke.mockResolvedValue(proposal);

      const result = await ideationApi.proposals.update("proposal-1", {
        title: "Updated Title",
      });

      expect(result.title).toBe("Updated Title");
      expect(result.userModified).toBe(true);
    });
  });

  describe("delete", () => {
    it("should call delete_task_proposal with id", async () => {
      mockInvoke.mockResolvedValue(undefined);

      await ideationApi.proposals.delete("proposal-1");

      expect(mockInvoke).toHaveBeenCalledWith("delete_task_proposal", {
        id: "proposal-1",
      });
    });
  });

  describe("reorder", () => {
    it("should call reorder_proposals with session_id and proposal_ids", async () => {
      mockInvoke.mockResolvedValue(undefined);

      await ideationApi.proposals.reorder("session-1", ["p1", "p2", "p3"]);

      expect(mockInvoke).toHaveBeenCalledWith("reorder_proposals", {
        sessionId: "session-1",
        proposalIds: ["p1", "p2", "p3"],
      });
    });
  });

  describe("assessPriority", () => {
    it("should call assess_proposal_priority with id", async () => {
      mockInvoke.mockResolvedValue({
        proposal_id: "proposal-1",
        priority: "high",
        score: 75,
        reason: "Blocks 2 tasks",
      });

      const result = await ideationApi.proposals.assessPriority("proposal-1");

      expect(mockInvoke).toHaveBeenCalledWith("assess_proposal_priority", {
        id: "proposal-1",
      });
      expect(result.priority).toBe("high");
      expect(result.score).toBe(75);
    });
  });

  describe("assessAllPriorities", () => {
    it("should call assess_all_priorities with session_id", async () => {
      mockInvoke.mockResolvedValue([
        { proposal_id: "p1", priority: "high", score: 80, reason: "Reason 1" },
        { proposal_id: "p2", priority: "low", score: 30, reason: "Reason 2" },
      ]);

      const result = await ideationApi.proposals.assessAllPriorities("session-1");

      expect(mockInvoke).toHaveBeenCalledWith("assess_all_priorities", {
        sessionId: "session-1",
      });
      expect(result).toHaveLength(2);
    });
  });
});

describe("ideationApi.dependencies", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("remove", () => {
    it("should call remove_proposal_dependency", async () => {
      mockInvoke.mockResolvedValue(undefined);

      await ideationApi.dependencies.remove("proposal-1", "proposal-2");

      expect(mockInvoke).toHaveBeenCalledWith("remove_proposal_dependency", {
        proposalId: "proposal-1",
        dependsOnId: "proposal-2",
      });
    });
  });

  describe("getDependencies", () => {
    it("should call get_proposal_dependencies", async () => {
      mockInvoke.mockResolvedValue(["p2", "p3"]);

      const result = await ideationApi.dependencies.getDependencies("proposal-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_proposal_dependencies", {
        proposalId: "proposal-1",
      });
      expect(result).toEqual(["p2", "p3"]);
    });
  });

  describe("getDependents", () => {
    it("should call get_proposal_dependents", async () => {
      mockInvoke.mockResolvedValue(["p4", "p5"]);

      const result = await ideationApi.dependencies.getDependents("proposal-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_proposal_dependents", {
        proposalId: "proposal-1",
      });
      expect(result).toEqual(["p4", "p5"]);
    });
  });

  describe("analyze", () => {
    it("should call analyze_dependencies with session_id", async () => {
      const graph = {
        nodes: [{ proposal_id: "p1", title: "P1", in_degree: 0, out_degree: 1 }],
        edges: [{ from: "p1", to: "p2" }],
        critical_path: ["p1", "p2"],
        has_cycles: false,
        cycles: null,
      };
      mockInvoke.mockResolvedValue(graph);

      const result = await ideationApi.dependencies.analyze("session-1");

      expect(mockInvoke).toHaveBeenCalledWith("analyze_dependencies", {
        sessionId: "session-1",
      });
      expect(result.hasCycles).toBe(false);
      expect(result.criticalPath).toEqual(["p1", "p2"]);
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

      const result = await ideationApi.dependencies.analyze("session-1");

      expect(result.hasCycles).toBe(true);
      expect(result.cycles).toEqual([["p1", "p2", "p3"]]);
    });
  });
});

describe("ideationApi.apply", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("toKanban", () => {
    it("should call apply_proposals_to_kanban with input", async () => {
      mockInvoke.mockResolvedValue({
        created_task_ids: ["task-1", "task-2"],
        dependencies_created: 1,
        warnings: [],
        session_converted: false,
      });

      await ideationApi.apply.toKanban({
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

    it("should return apply result", async () => {
      mockInvoke.mockResolvedValue({
        created_task_ids: ["task-1", "task-2"],
        dependencies_created: 1,
        warnings: ["Some dep not preserved"],
        session_converted: true,
      });

      const result = await ideationApi.apply.toKanban({
        sessionId: "session-1",
        proposalIds: ["p1", "p2"],
        targetColumn: "todo",
      });

      expect(result.createdTaskIds).toEqual(["task-1", "task-2"]);
      expect(result.dependenciesCreated).toBe(1);
      expect(result.warnings).toHaveLength(1);
      expect(result.sessionConverted).toBe(true);
    });

    it("sends base_branch_override in snake_case when baseBranchOverride provided", async () => {
      mockInvoke.mockResolvedValue({
        created_task_ids: ["task-1"],
        dependencies_created: 0,
        warnings: [],
        session_converted: false,
      });

      await ideationApi.apply.toKanban({
        sessionId: "session-1",
        proposalIds: ["p1"],
        targetColumn: "backlog",
        baseBranchOverride: "develop",
      });

      expect(mockInvoke).toHaveBeenCalledWith(
        "apply_proposals_to_kanban",
        expect.objectContaining({
          input: expect.objectContaining({
            base_branch_override: "develop",
          }),
        })
      );
    });

    it("omits base_branch_override key when baseBranchOverride is undefined", async () => {
      mockInvoke.mockResolvedValue({
        created_task_ids: ["task-1"],
        dependencies_created: 0,
        warnings: [],
        session_converted: false,
      });

      await ideationApi.apply.toKanban({
        sessionId: "session-1",
        proposalIds: ["p1"],
        targetColumn: "backlog",
      });

      const invokeArgs = mockInvoke.mock.calls[0]![1] as { input: Record<string, unknown> };
      expect(invokeArgs.input).not.toHaveProperty("base_branch_override");
    });
  });
});

describe("ideationApi.taskDependencies", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("getBlockers", () => {
    it("should call get_task_blockers with task_id", async () => {
      mockInvoke.mockResolvedValue(["task-2", "task-3"]);

      const result = await ideationApi.taskDependencies.getBlockers("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_task_blockers", {
        taskId: "task-1",
      });
      expect(result).toEqual(["task-2", "task-3"]);
    });
  });

  describe("getBlocked", () => {
    it("should call get_blocked_tasks with task_id", async () => {
      mockInvoke.mockResolvedValue(["task-4"]);

      const result = await ideationApi.taskDependencies.getBlocked("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_blocked_tasks", {
        taskId: "task-1",
      });
      expect(result).toEqual(["task-4"]);
    });
  });
});

// ============================================================================
// Schema unit tests (no network/invoke required)
// ============================================================================

describe("VerificationResponseSchema", () => {
  it("parses the model-native action and exact-proof response", () => {
    const raw = {
      session_id: "session-1",
      status: "verifying",
      in_progress: true,
      plan_artifact_id: "plan-v2",
      verified_plan_artifact_id: null,
      agent_run_id: "run-1",
      started_at: "2026-07-15T12:00:00Z",
      completed_at: null,
      error: null,
    };
    const result = VerificationResponseSchema.parse(raw);
    expect(result.status).toBe("verifying");
    expect(result.plan_artifact_id).toBe("plan-v2");
    expect(result.agent_run_id).toBe("run-1");
  });

  it("rejects retired verifier payloads", () => {
    expect(() =>
      VerificationResponseSchema.parse({
        session_id: "session-1",
        status: "reviewing",
        in_progress: true,
      }),
    ).toThrow();
  });
});

describe("CreateChildSessionResponseSchema", () => {
  it("preserves generation field when present", () => {
    const raw = {
      session_id: "child-session-1",
      parent_session_id: "parent-session-1",
      title: "Verification Session",
      status: "active",
      created_at: "2026-01-24T12:00:00Z",
      generation: 1,
    };
    const result = CreateChildSessionResponseSchema.parse(raw);
    expect(result.generation).toBe(1);
  });

  it("parses successfully when generation is absent", () => {
    const raw = {
      session_id: "child-session-1",
      parent_session_id: "parent-session-1",
      title: null,
      status: "active",
      created_at: "2026-01-24T12:00:00Z",
    };
    const result = CreateChildSessionResponseSchema.parse(raw);
    expect(result.generation).toBeUndefined();
  });

  it("preserves higher generation numbers", () => {
    const raw = {
      session_id: "child-session-1",
      parent_session_id: "parent-session-1",
      title: null,
      status: "active",
      created_at: "2026-01-24T12:00:00Z",
      generation: 5,
    };
    const result = CreateChildSessionResponseSchema.parse(raw);
    expect(result.generation).toBe(5);
  });
});

// ============================================================================
// ideationApi.verification — fetch-based HTTP endpoint tests
// ============================================================================

describe("ideationApi.verification", () => {
  const mockFetch = vi.fn();

  beforeEach(() => {
    vi.stubGlobal("fetch", mockFetch);
    mockFetch.mockReset();
  });

  const makeVerificationRaw = (overrides = {}) => ({
    session_id: "session-1",
    status: "verifying",
    in_progress: true,
    plan_artifact_id: "plan-v2",
    verified_plan_artifact_id: null,
    agent_run_id: "run-1",
    started_at: "2026-07-15T12:00:00Z",
    completed_at: null,
    error: null,
    ...overrides,
  });

  describe("getStatus", () => {
    it("fetches GET and returns transformed VerificationStatusResponse", async () => {
      mockFetch.mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(makeVerificationRaw()),
      });

      const result = await ideationApi.verification.getStatus("session-1");

      expect(mockFetch).toHaveBeenCalledWith(
        backendApiUrl("ideation/sessions/session-1/verification")
      );
      expect(result.sessionId).toBe("session-1");
      expect(result.status).toBe("verifying");
      expect(result.inProgress).toBe(true);
      expect(result.planArtifactId).toBe("plan-v2");
      expect(result.verifiedPlanArtifactId).toBeNull();
      expect(result.agentRunId).toBe("run-1");
    });

    it("throws when response is not ok", async () => {
      mockFetch.mockResolvedValue({ ok: false, status: 404 });
      await expect(ideationApi.verification.getStatus("session-1")).rejects.toThrow(
        "Failed to get verification status: 404"
      );
    });
  });
});

describe("ideationApi.settings.update — payload regression", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  const baseSettingsResponse = {
    plan_mode: "optional",
    require_plan_approval: false,
    suggest_plans_for_complex: true,
    auto_link_proposals: true,
    auto_verify_plans: false,
    auto_verify_draft_plans: true,
    require_accept_for_finalize: true,
    require_verification_for_accept: false,
    require_verification_for_proposals: false,
    external_overrides: {
      auto_verify_plans: null,
      require_verification_for_accept: null,
      require_verification_for_proposals: null,
      require_accept_for_finalize: null,
    },
  };

  it("includes require_accept_for_finalize in the update payload (bug fix)", async () => {
    mockInvoke.mockResolvedValue(baseSettingsResponse);

    await ideationApi.settings.update({
      autoVerifyDraftPlans: true,
      autoVerifyPlans: false,
      requireAcceptForFinalize: true,
      requireVerificationForAccept: false,
      externalOverrides: {
        autoVerifyPlans: null,
        requireVerificationForAccept: null,
        requireAcceptForFinalize: null,
      },
    });

    const calledSettings = mockInvoke.mock.calls[0]![1].settings;
    expect(calledSettings).toHaveProperty("require_accept_for_finalize", true);
  });

  it("includes all new fields in the update payload", async () => {
    mockInvoke.mockResolvedValue(baseSettingsResponse);

    await ideationApi.settings.update({
      autoVerifyDraftPlans: false,
      autoVerifyPlans: true,
      requireAcceptForFinalize: false,
      requireVerificationForAccept: true,
      externalOverrides: {
        autoVerifyPlans: false,
        requireVerificationForAccept: false,
        requireAcceptForFinalize: null,
      },
    });

    const calledSettings = mockInvoke.mock.calls[0]![1].settings;
    expect(calledSettings).toHaveProperty("require_verification_for_accept", true);
    expect(calledSettings).toHaveProperty("auto_verify_draft_plans", false);
    expect(calledSettings).toHaveProperty("auto_verify_plans", true);
    expect(calledSettings).toHaveProperty("require_verification_for_proposals", false);
    expect(calledSettings).toHaveProperty("external_overrides", {
      auto_verify_plans: false,
      require_verification_for_accept: false,
      require_verification_for_proposals: null,
      require_accept_for_finalize: null,
    });
  });
});

describe("ideationApi.settings — Tasks feature controls", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("maps the disable impact response to the frontend contract", async () => {
    mockInvoke.mockResolvedValue({
      active_standalone_tasks: 2,
      active_attached_agent_workspaces: 1,
      paused_or_blocked_tasks: 3,
      active_branch_update_operations: 1,
      affected_task_ids: ["task-1"],
      affected_conversation_ids: ["conversation-1"],
      affected_project_ids: ["project-1"],
    });

    await expect(ideationApi.settings.getDisableImpact()).resolves.toEqual({
      activeStandaloneTasks: 2,
      activeAttachedAgentWorkspaces: 1,
      pausedOrBlockedTasks: 3,
      activeBranchUpdateOperations: 1,
      affectedTaskIds: ["task-1"],
      affectedConversationIds: ["conversation-1"],
      affectedProjectIds: ["project-1"],
    });
    expect(mockInvoke).toHaveBeenCalledWith("get_tasks_disable_impact", {});
  });

  it("sends the requested Tasks feature state and transforms the response", async () => {
    mockInvoke.mockResolvedValue({
      tasks_enabled: false,
      tasks_feature_state: "disabled",
      require_accept_for_finalize: false,
    });

    await expect(ideationApi.settings.setTasksEnabled(false)).resolves.toMatchObject({
      tasksEnabled: false,
      tasksFeatureState: "disabled",
    });
    expect(mockInvoke).toHaveBeenCalledWith("set_tasks_feature_enabled", { enabled: false });
  });
});
