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

const { acceptAndSchedule } = await import(
  "../../src/composites/accept-and-schedule.js"
);

// ─── Test fixtures ────────────────────────────────────────────────────────────

const testContext: ApiKeyContext = {
  keyId: "key-test",
  projectIds: ["proj-alpha"],
  permissions: 3,
};

// ─── acceptAndSchedule ────────────────────────────────────────────────────────

describe("acceptAndSchedule", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns task IDs from apply_proposals on first accept", async () => {
    // Load proposals
    mockGet.mockResolvedValueOnce({
      status: 200,
      body: { proposals: [{ id: "prop-001" }, { id: "prop-002" }] },
    });
    // Apply proposals — returns created_task_ids
    mockPost.mockResolvedValueOnce({
      status: 200,
      body: {
        created_task_ids: ["task-001", "task-002"],
        session_converted: true,
        dependencies_created: 0,
        warnings: [],
      },
    });

    const result = await acceptAndSchedule(
      { sessionId: "sess-001" },
      testContext
    );

    expect(result.success).toBe(true);
    expect(result.taskIds).toEqual(["task-001", "task-002"]);
    expect(result.progress.completed).toContain("apply_proposals");
  });

  it("falls back to session tasks endpoint when session is already accepted", async () => {
    // Load proposals (session still has proposals)
    mockGet.mockResolvedValueOnce({
      status: 200,
      body: { proposals: [{ id: "prop-001" }] },
    });
    // apply_proposals returns empty task_ids + session_converted=false (already accepted)
    mockPost.mockResolvedValueOnce({
      status: 200,
      body: {
        created_task_ids: [],
        session_converted: false,
        dependencies_created: 0,
        warnings: [],
      },
    });
    // Fallback: GET /api/external/sessions/:id/tasks
    mockGet.mockResolvedValueOnce({
      status: 200,
      body: {
        session_id: "sess-001",
        tasks: [
          { id: "existing-task-001", title: "Auth module", status: "executing" },
          { id: "existing-task-002", title: "DB setup", status: "merged" },
        ],
        delivery_status: "in_progress",
        task_count: 2,
      },
    });

    const result = await acceptAndSchedule(
      { sessionId: "sess-001" },
      testContext
    );

    expect(result.success).toBe(true);
    expect(result.taskIds).toEqual(["existing-task-001", "existing-task-002"]);
    expect(mockGet).toHaveBeenCalledTimes(2);
    expect(mockGet).toHaveBeenLastCalledWith(
      "/api/external/sessions/sess-001/tasks",
      testContext
    );
  });

  it("returns empty task IDs (not failure) when already-accepted fallback fetch also fails", async () => {
    mockGet.mockResolvedValueOnce({
      status: 200,
      body: { proposals: [{ id: "prop-001" }] },
    });
    mockPost.mockResolvedValueOnce({
      status: 200,
      body: { created_task_ids: [], session_converted: false, dependencies_created: 0, warnings: [] },
    });
    // Fallback fetch fails
    mockGet.mockRejectedValueOnce(new Error("Network error"));

    const result = await acceptAndSchedule(
      { sessionId: "sess-001" },
      testContext
    );

    // Non-fatal: saga succeeds with empty task list rather than failing
    expect(result.success).toBe(true);
    expect(result.taskIds).toEqual([]);
  });

  it("returns empty task IDs (success) when session has no proposals", async () => {
    mockGet.mockResolvedValueOnce({
      status: 200,
      body: { proposals: [] },
    });

    const result = await acceptAndSchedule(
      { sessionId: "sess-no-proposals" },
      testContext
    );

    expect(result.success).toBe(true);
    expect(result.taskIds).toEqual([]);
    expect(mockPost).not.toHaveBeenCalled();
  });

  it("returns failure when load_session step fails", async () => {
    const { BackendError } = await import("../../src/backend-client.js");
    mockGet.mockRejectedValueOnce(new BackendError(404, "Session not found"));

    const result = await acceptAndSchedule(
      { sessionId: "sess-missing" },
      testContext
    );

    expect(result.success).toBe(false);
    expect(result.taskIds).toEqual([]);
    expect(result.progress.failed?.step).toBe("load_session");
  });

  it("returns an actionable verification outcome when acceptance queues verification", async () => {
    mockGet.mockResolvedValueOnce({
      status: 200,
      body: { proposals: [{ id: "prop-001" }] },
    });
    mockPost.mockResolvedValueOnce({
      status: 400,
      body: {
        error:
          "Plan verification was queued. Accept the plan again after verification completes.",
      },
    });

    const result = await acceptAndSchedule(
      { sessionId: "sess-verify" },
      testContext,
    );

    expect(result).toMatchObject({
      success: false,
      verification: {
        status: "queued",
        sessionId: "sess-verify",
        pollTool: "v1_get_plan_verification",
        retryTool: "v1_accept_plan_and_schedule",
      },
      progress: {
        failed: {
          step: "apply_proposals",
          error:
            "Plan verification was queued. Accept the plan again after verification completes.",
        },
      },
    });
  });
});
