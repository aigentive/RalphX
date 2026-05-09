import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ApiKeyContext } from "../../src/types.js";

// ─── Mock BackendClient ──────────────────────────────────────────────────────

const mockGet = vi.fn();
const mockPost = vi.fn();

vi.mock("../../src/backend-client.js", () => ({
  getBackendClient: () => ({
    get: mockGet,
    post: mockPost,
  }),
  BackendError: class BackendError extends Error {
    statusCode: number;
    constructor(statusCode: number, message: string) {
      super(message);
      this.name = "BackendError";
      this.statusCode = statusCode;
    }
  },
}));

// Mock composites that require additional dependencies
vi.mock("../../src/composites/start-ideation.js", () => ({
  startIdeation: vi.fn(),
}));

vi.mock("../../src/composites/accept-and-schedule.js", () => ({
  acceptAndSchedule: vi.fn(),
}));

const {
  handleGetIdeationStatus,
  handleSendIdeationMessage,
  handleListProposals,
  handleGetProposalDetail,
  handleGetPlan,
  handleStartIdeation,
  handleAcceptPlanAndSchedule,
  handleModifyProposal,
  handleAnalyzeDependencies,
  handleGetSessionTasks,
  handleAppendTaskToPlan,
} = await import("../../src/tools/ideation.js");

const { startIdeation } = await import(
  "../../src/composites/start-ideation.js"
);
const { acceptAndSchedule } = await import(
  "../../src/composites/accept-and-schedule.js"
);

// ─── Test fixtures ────────────────────────────────────────────────────────────

const testContext: ApiKeyContext = {
  keyId: "key-test",
  projectIds: ["proj-alpha"],
  permissions: 3,
};

// ─── handleGetIdeationStatus ──────────────────────────────────────────────────

describe("handleGetIdeationStatus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns ideation session status", async () => {
    const backendPayload = {
      session_id: "sess-001",
      project_id: "proj-alpha",
      title: "Auth Feature",
      status: "active",
      agent_running: true,
      proposal_count: 3,
      created_at: "2024-01-01T00:00:00Z",
    };
    mockGet.mockResolvedValueOnce({ status: 200, body: backendPayload });

    const result = await handleGetIdeationStatus(
      { session_id: "sess-001" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.session_id).toBe("sess-001");
    expect(parsed.status).toBe("active");
    expect(parsed.agent_running).toBe(true);
    expect(parsed.proposal_count).toBe(3);
    expect(mockGet).toHaveBeenCalledWith(
      "/api/external/ideation_status/sess-001",
      testContext
    );
  });

  it("returns missing_argument when session_id not provided", async () => {
    const result = await handleGetIdeationStatus({}, testContext);
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(parsed.message).toContain("session_id");
    expect(mockGet).not.toHaveBeenCalled();
  });

  it("handles backend error", async () => {
    const { BackendError } = await import("../../src/backend-client.js");
    mockGet.mockRejectedValueOnce(new BackendError(404, "Session not found"));

    const result = await handleGetIdeationStatus(
      { session_id: "nonexistent" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("backend_error");
    expect(parsed.status).toBe(404);
  });
});

// ─── handleSendIdeationMessage ────────────────────────────────────────────────

describe("handleSendIdeationMessage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("sends a message to ideation agent", async () => {
    mockPost.mockResolvedValueOnce({
      status: 200,
      body: { queued: true, message_id: "msg-001" },
    });

    const result = await handleSendIdeationMessage(
      { session_id: "sess-001", message: "Focus on OAuth" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.queued).toBe(true);
    expect(mockPost).toHaveBeenCalledWith(
      "/api/external/ideation_message",
      testContext,
      { session_id: "sess-001", message: "Focus on OAuth" }
    );
  });

  it("returns missing_argument when session_id absent", async () => {
    const result = await handleSendIdeationMessage(
      { message: "hello" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(parsed.message).toContain("session_id");
  });

  it("returns missing_argument when message absent", async () => {
    const result = await handleSendIdeationMessage(
      { session_id: "sess-001" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(parsed.message).toContain("message");
  });
});

// ─── handleListProposals ──────────────────────────────────────────────────────

describe("handleListProposals", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("lists proposals for a session", async () => {
    const backendPayload = {
      proposals: [
        { id: "prop-001", title: "Auth module" },
        { id: "prop-002", title: "Profile page" },
      ],
    };
    mockGet.mockResolvedValueOnce({ status: 200, body: backendPayload });

    const result = await handleListProposals(
      { session_id: "sess-001" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.proposals).toHaveLength(2);
    expect(parsed.proposals[0].id).toBe("prop-001");
    expect(mockGet).toHaveBeenCalledWith(
      "/api/list_session_proposals/sess-001",
      testContext
    );
  });

  it("returns missing_argument when session_id not provided", async () => {
    const result = await handleListProposals({}, testContext);
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(mockGet).not.toHaveBeenCalled();
  });
});

// ─── handleStartIdeation ──────────────────────────────────────────────────────

describe("handleStartIdeation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("delegates to startIdeation composite and returns result", async () => {
    vi.mocked(startIdeation).mockResolvedValueOnce({
      sessionId: "sess-new",
      status: "started",
    });

    const result = await handleStartIdeation(
      { project_id: "proj-alpha", prompt: "Build auth system" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.sessionId).toBe("sess-new");
    expect(parsed.status).toBe("started");
    expect(startIdeation).toHaveBeenCalledWith(
      { projectId: "proj-alpha", prompt: "Build auth system" },
      testContext
    );
  });

  it("returns missing_argument when project_id absent", async () => {
    const result = await handleStartIdeation(
      { prompt: "test" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(parsed.message).toContain("project_id");
    expect(startIdeation).not.toHaveBeenCalled();
  });

  it("returns missing_argument when prompt absent", async () => {
    const result = await handleStartIdeation(
      { project_id: "proj-alpha" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(parsed.message).toContain("prompt");
    expect(startIdeation).not.toHaveBeenCalled();
  });
});

// ─── handleAcceptPlanAndSchedule ──────────────────────────────────────────────

describe("handleAcceptPlanAndSchedule", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("delegates to acceptAndSchedule composite and returns result", async () => {
    vi.mocked(acceptAndSchedule).mockResolvedValueOnce({
      success: true,
      taskIds: ["task-001", "task-002"],
      progress: {
        step: "schedule_tasks",
        completed: [
          "load_session",
          "apply_proposals",
          "create_tasks",
          "schedule_tasks",
        ],
      },
    });

    const result = await handleAcceptPlanAndSchedule(
      { session_id: "sess-001" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.success).toBe(true);
    expect(parsed.taskIds).toHaveLength(2);
    expect(acceptAndSchedule).toHaveBeenCalledWith(
      { sessionId: "sess-001" },
      testContext
    );
  });

  it("returns missing_argument when session_id absent", async () => {
    const result = await handleAcceptPlanAndSchedule({}, testContext);
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(acceptAndSchedule).not.toHaveBeenCalled();
  });
});

// ─── handleModifyProposal ─────────────────────────────────────────────────────

describe("handleModifyProposal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("sends proposal update to backend", async () => {
    mockPost.mockResolvedValueOnce({ status: 200, body: { success: true } });

    const result = await handleModifyProposal(
      {
        proposal_id: "prop-001",
        changes: { title: "Updated Auth Module" },
      },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.success).toBe(true);
    expect(mockPost).toHaveBeenCalledWith(
      "/api/update_task_proposal",
      testContext,
      { proposal_id: "prop-001", title: "Updated Auth Module" }
    );
  });

  it("returns missing_argument when proposal_id absent", async () => {
    const result = await handleModifyProposal(
      { changes: { title: "something" } },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(parsed.message).toContain("proposal_id");
  });

  it("returns missing_argument when changes absent", async () => {
    const result = await handleModifyProposal(
      { proposal_id: "prop-001" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(parsed.message).toContain("changes");
  });
});

// ─── handleGetSessionTasks ────────────────────────────────────────────────────

describe("handleGetSessionTasks", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns task list and delivery_status for a session", async () => {
    const backendPayload = {
      session_id: "sess-001",
      tasks: [
        {
          id: "task-001",
          title: "Implement auth",
          status: "executing",
          proposal_id: "prop-001",
          category: "regular",
          priority: 50,
          created_at: "2024-01-01T00:00:00Z",
        },
      ],
      delivery_status: "in_progress",
      task_count: 1,
    };
    mockGet.mockResolvedValueOnce({ status: 200, body: backendPayload });

    const result = await handleGetSessionTasks(
      { session_id: "sess-001" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.session_id).toBe("sess-001");
    expect(parsed.delivery_status).toBe("in_progress");
    expect(parsed.task_count).toBe(1);
    expect(parsed.tasks).toHaveLength(1);
    expect(parsed.tasks[0].id).toBe("task-001");
    expect(mockGet).toHaveBeenCalledWith(
      "/api/external/sessions/sess-001/tasks",
      testContext
    );
  });

  it("returns empty task list with not_scheduled for session with no tasks", async () => {
    mockGet.mockResolvedValueOnce({
      status: 200,
      body: {
        session_id: "sess-empty",
        tasks: [],
        delivery_status: "not_scheduled",
        task_count: 0,
      },
    });

    const result = await handleGetSessionTasks(
      { session_id: "sess-empty" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.tasks).toHaveLength(0);
    expect(parsed.delivery_status).toBe("not_scheduled");
    expect(parsed.task_count).toBe(0);
  });

  it("returns missing_argument when session_id not provided", async () => {
    const result = await handleGetSessionTasks({}, testContext);
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(parsed.message).toContain("session_id");
    expect(mockGet).not.toHaveBeenCalled();
  });

  it("handles backend 404 error", async () => {
    const { BackendError } = await import("../../src/backend-client.js");
    mockGet.mockRejectedValueOnce(new BackendError(404, "Session not found"));

    const result = await handleGetSessionTasks(
      { session_id: "nonexistent" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("backend_error");
    expect(parsed.status).toBe(404);
  });
});

// ─── handleAppendTaskToPlan ─────────────────────────────────────────────────

describe("handleAppendTaskToPlan", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("appends a one-off task to an accepted active ideation plan", async () => {
    const backendPayload = {
      sessionId: "sess-001",
      taskId: "task-new",
      executionPlanId: "plan-001",
      planBranchId: "branch-001",
      mergeTaskId: "merge-001",
      taskStatus: "ready",
      dependenciesCreated: 1,
      anyReadyTasks: true,
    };
    mockPost.mockResolvedValueOnce({ status: 200, body: backendPayload });

    const result = await handleAppendTaskToPlan(
      {
        session_id: "sess-001",
        title: "Add keyboard shortcut",
        description: "One-off follow-up after plan acceptance",
        steps: ["Wire command", "Add regression test"],
        acceptance_criteria: ["Shortcut works in Agents and Ideation modes"],
        depends_on_task_ids: ["task-existing"],
        priority: 40,
      },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.taskId).toBe("task-new");
    expect(mockPost).toHaveBeenCalledWith(
      "/api/external/sessions/sess-001/tasks",
      testContext,
      {
        title: "Add keyboard shortcut",
        description: "One-off follow-up after plan acceptance",
        steps: ["Wire command", "Add regression test"],
        acceptanceCriteria: ["Shortcut works in Agents and Ideation modes"],
        dependsOnTaskIds: ["task-existing"],
        priority: 40,
      }
    );
  });

  it("returns missing_argument when title is absent", async () => {
    const result = await handleAppendTaskToPlan(
      { session_id: "sess-001" },
      testContext
    );
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("missing_argument");
    expect(parsed.message).toContain("title");
    expect(mockPost).not.toHaveBeenCalled();
  });
});
