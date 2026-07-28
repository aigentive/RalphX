import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { typedInvoke, api, HealthResponseSchema } from "./tauri";

// Force real Tauri API path in this test file (no web-mode mock API fallback)
vi.mock("./tauri-detection", () => ({
  isWebMode: () => false,
  isTauriMode: () => true,
}));

// Cast invoke to a mock function for testing
const mockInvoke = invoke as ReturnType<typeof vi.fn>;

describe("typedInvoke", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should invoke the command with the given arguments", async () => {
    const schema = z.object({ value: z.number() });
    mockInvoke.mockResolvedValue({ value: 42 });

    await typedInvoke("test_command", { arg1: "test" }, schema);

    expect(mockInvoke).toHaveBeenCalledWith("test_command", { arg1: "test" });
  });

  it("should return validated response when schema matches", async () => {
    const schema = z.object({ value: z.number() });
    mockInvoke.mockResolvedValue({ value: 42 });

    const result = await typedInvoke("test_command", {}, schema);

    expect(result).toEqual({ value: 42 });
  });

  it("should throw when response doesn't match schema", async () => {
    const schema = z.object({ value: z.number() });
    mockInvoke.mockResolvedValue({ value: "not a number" });

    await expect(typedInvoke("test_command", {}, schema)).rejects.toThrow();
  });

  it("should throw when response is missing required fields", async () => {
    const schema = z.object({ required: z.string() });
    mockInvoke.mockResolvedValue({});

    await expect(typedInvoke("test_command", {}, schema)).rejects.toThrow();
  });

  it("should handle null values according to schema", async () => {
    const schema = z.object({ value: z.string().nullable() });
    mockInvoke.mockResolvedValue({ value: null });

    const result = await typedInvoke("test_command", {}, schema);

    expect(result).toEqual({ value: null });
  });

  it("should handle arrays according to schema", async () => {
    const schema = z.array(z.number());
    mockInvoke.mockResolvedValue([1, 2, 3]);

    const result = await typedInvoke("test_command", {}, schema);

    expect(result).toEqual([1, 2, 3]);
  });

  it("should propagate invoke errors", async () => {
    const schema = z.object({ value: z.number() });
    mockInvoke.mockRejectedValue(new Error("Backend error"));

    await expect(typedInvoke("test_command", {}, schema)).rejects.toThrow(
      "Backend error"
    );
  });
});

describe("HealthResponseSchema", () => {
  it("should parse valid health response", () => {
    const response = { status: "ok" };
    expect(() => HealthResponseSchema.parse(response)).not.toThrow();
  });

  it("should reject response without status", () => {
    expect(() => HealthResponseSchema.parse({})).toThrow();
  });

  it("should reject response with non-string status", () => {
    expect(() => HealthResponseSchema.parse({ status: 123 })).toThrow();
  });
});

describe("api.health", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call health_check command", async () => {
    mockInvoke.mockResolvedValue({ status: "ok" });

    await api.health.check();

    expect(mockInvoke).toHaveBeenCalledWith("health_check", {});
  });

  it("should return health response", async () => {
    mockInvoke.mockResolvedValue({ status: "ok" });

    const result = await api.health.check();

    expect(result).toEqual({ status: "ok" });
  });

  it("should validate response with HealthResponseSchema", async () => {
    mockInvoke.mockResolvedValue({ status: 123 }); // Invalid

    await expect(api.health.check()).rejects.toThrow();
  });

  it("should propagate backend errors", async () => {
    mockInvoke.mockRejectedValue(new Error("Connection failed"));

    await expect(api.health.check()).rejects.toThrow("Connection failed");
  });
});

// Helper to create mock task
const createMockTask = (overrides = {}) => ({
  id: "task-1",
  project_id: "project-1",
  category: "feature",
  title: "Test Task",
  description: null,
  priority: 0,
  internal_status: "backlog",
  created_at: "2026-01-24T12:00:00Z",
  updated_at: "2026-01-24T12:00:00Z",
  started_at: null,
  completed_at: null,
  archived_at: null,
  blocked_reason: null,
  ...overrides,
});

// Helper to create mock project
const createMockProject = (overrides = {}) => ({
  id: "project-1",
  name: "Test Project",
  working_directory: "/path/to/project",
  git_mode: "worktree",
  base_branch: null,
  worktree_parent_directory: null,
  use_feature_branches: true,
  merge_validation_mode: "block",
  detected_analysis: null,
  custom_analysis: null,
  analyzed_at: null,
  created_at: "2026-01-24T12:00:00Z",
  updated_at: "2026-01-24T12:00:00Z",
  ...overrides,
});

describe("api.tasks", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("list", () => {
    it("should call list_tasks with params object", async () => {
      mockInvoke.mockResolvedValue({
        tasks: [createMockTask()],
        total: 1,
        has_more: false,
        offset: 0,
      });

      await api.tasks.list({ projectId: "project-1" });

      expect(mockInvoke).toHaveBeenCalledWith("list_tasks", {
        projectId: "project-1",
      });
    });

    it("should return paginated task response", async () => {
      const tasks = [createMockTask({ id: "t1" }), createMockTask({ id: "t2" })];
      mockInvoke.mockResolvedValue({
        tasks,
        total: 2,
        has_more: false,
        offset: 0,
      });

      const result = await api.tasks.list({ projectId: "project-1" });

      expect(result.tasks).toHaveLength(2);
      expect(result.tasks[0]?.id).toBe("t1");
      expect(result.total).toBe(2);
      expect(result.hasMore).toBe(false);
    });

    it("should validate task list response schema", async () => {
      mockInvoke.mockResolvedValue({ invalid: "response" });

      await expect(api.tasks.list({ projectId: "project-1" })).rejects.toThrow();
    });
  });

  describe("get", () => {
    it("should call get_task with id", async () => {
      mockInvoke.mockResolvedValue(createMockTask());

      await api.tasks.get("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_task", { id: "task-1" });
    });

    it("should return task", async () => {
      const task = createMockTask({ title: "My Task" });
      mockInvoke.mockResolvedValue(task);

      const result = await api.tasks.get("task-1");

      expect(result.title).toBe("My Task");
    });
  });

  describe("create", () => {
    it("should call create_task with input", async () => {
      mockInvoke.mockResolvedValue(createMockTask());
      const input = { projectId: "p1", title: "New Task" };

      await api.tasks.create(input);

      expect(mockInvoke).toHaveBeenCalledWith("create_task", { input });
    });

    it("should return created task", async () => {
      const task = createMockTask({ title: "Created Task" });
      mockInvoke.mockResolvedValue(task);

      const result = await api.tasks.create({ projectId: "p1", title: "Created Task" });

      expect(result.title).toBe("Created Task");
    });
  });

  describe("update", () => {
    it("should call update_task with taskId and input", async () => {
      mockInvoke.mockResolvedValue(createMockTask());
      const input = { title: "Updated" };

      await api.tasks.update("task-1", input);

      expect(mockInvoke).toHaveBeenCalledWith("update_task", {
        taskId: "task-1",
        input,
      });
    });
  });

  describe("move", () => {
    it("should call move_task with taskId and toStatus", async () => {
      mockInvoke.mockResolvedValue(createMockTask({ internal_status: "ready" }));

      await api.tasks.move("task-1", "ready");

      expect(mockInvoke).toHaveBeenCalledWith("move_task", {
        taskId: "task-1",
        toStatus: "ready",
        note: null,
      });
    });
  });

  describe("cleanupTask", () => {
    it("should call cleanup_task with taskId", async () => {
      mockInvoke.mockResolvedValue(null);

      await api.tasks.cleanupTask("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("cleanup_task", { taskId: "task-1" });
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("Task not found"));

      await expect(api.tasks.cleanupTask("nonexistent")).rejects.toThrow("Task not found");
    });
  });
});

describe("api.projects", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("list", () => {
    it("should call list_projects", async () => {
      mockInvoke.mockResolvedValue([createMockProject()]);

      await api.projects.list();

      expect(mockInvoke).toHaveBeenCalledWith("list_projects", {});
    });

    it("should return array of projects", async () => {
      const projects = [
        createMockProject({ id: "p1" }),
        createMockProject({ id: "p2" }),
      ];
      mockInvoke.mockResolvedValue(projects);

      const result = await api.projects.list();

      expect(result).toHaveLength(2);
    });

    it("should validate project schema", async () => {
      mockInvoke.mockResolvedValue([{ invalid: "project" }]);

      await expect(api.projects.list()).rejects.toThrow();
    });
  });

  describe("get", () => {
    it("should call get_project with projectId", async () => {
      mockInvoke.mockResolvedValue(createMockProject());

      await api.projects.get("project-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_project", {
        projectId: "project-1",
      });
    });

    it("should return project", async () => {
      const project = createMockProject({ name: "My Project" });
      mockInvoke.mockResolvedValue(project);

      const result = await api.projects.get("project-1");

      expect(result.name).toBe("My Project");
    });
  });

  describe("create", () => {
    it("should call create_project with input", async () => {
      mockInvoke.mockResolvedValue(createMockProject());
      const input = { name: "New Project", workingDirectory: "/path" };

      await api.projects.create(input);

      expect(mockInvoke).toHaveBeenCalledWith("create_project", { input });
    });
  });

  describe("update", () => {
    it("should call update_project with id and input", async () => {
      mockInvoke.mockResolvedValue(createMockProject());
      const input = { name: "Updated" };

      await api.projects.update("project-1", input);

      expect(mockInvoke).toHaveBeenCalledWith("update_project", {
        id: "project-1",
        input,
      });
    });
  });

  describe("archive", () => {
    it("should call archive_project with projectId", async () => {
      mockInvoke.mockResolvedValue(createMockProject());

      await api.projects.archive("project-1");

      expect(mockInvoke).toHaveBeenCalledWith("archive_project", {
        projectId: "project-1",
      });
    });

    it("should return archived project", async () => {
      mockInvoke.mockResolvedValue(
        createMockProject({
          name: "Archived Project",
        })
      );

      const result = await api.projects.archive("project-1");

      expect(result.id).toBe("project-1");
      expect(result.name).toBe("Archived Project");
    });
  });

  describe("readPrTemplate", () => {
    it("should call read_pr_template with projectId", async () => {
      mockInvoke.mockResolvedValue("# Template\n");

      await api.projects.readPrTemplate("project-1");

      expect(mockInvoke).toHaveBeenCalledWith("read_pr_template", {
        projectId: "project-1",
      });
    });

    it("should return nullable template content", async () => {
      mockInvoke.mockResolvedValue(null);

      const result = await api.projects.readPrTemplate("project-1");

      expect(result).toBeNull();
    });
  });

  describe("writePrTemplate", () => {
    it("should call write_pr_template with camelCase payload", async () => {
      mockInvoke.mockResolvedValue(null);

      await api.projects.writePrTemplate("project-1", "## Summary\n");

      expect(mockInvoke).toHaveBeenCalledWith("write_pr_template", {
        projectId: "project-1",
        content: "## Summary\n",
      });
    });

    it("should parse Tauri unit null responses as success", async () => {
      mockInvoke.mockResolvedValue(null);

      await expect(
        api.projects.writePrTemplate("project-1", ""),
      ).resolves.toBeNull();
    });
  });
});

// Helper to create mock workflow
const createMockWorkflow = (overrides = {}) => ({
  id: "ralphx-default",
  name: "RalphX Default",
  description: "Standard kanban workflow",
  columns: [
    { id: "draft", name: "Draft", maps_to: "backlog" },
    { id: "backlog", name: "Backlog", maps_to: "backlog" },
    { id: "todo", name: "To Do", maps_to: "ready" },
    { id: "planned", name: "Planned", maps_to: "ready" },
    { id: "in_progress", name: "In Progress", maps_to: "executing" },
    { id: "in_review", name: "In Review", maps_to: "pending_review" },
    { id: "done", name: "Done", maps_to: "approved" },
  ],
  is_default: true,
  ...overrides,
});

describe("api.workflows", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("get", () => {
    it("should call get_workflow with id", async () => {
      mockInvoke.mockResolvedValue(createMockWorkflow());

      await api.workflows.get("ralphx-default");

      expect(mockInvoke).toHaveBeenCalledWith("get_workflow", {
        id: "ralphx-default",
      });
    });

    it("should return workflow", async () => {
      const workflow = createMockWorkflow({ name: "Custom Workflow" });
      mockInvoke.mockResolvedValue(workflow);

      const result = await api.workflows.get("custom");

      expect(result.name).toBe("Custom Workflow");
    });

    it("should validate workflow schema", async () => {
      mockInvoke.mockResolvedValue({ invalid: "workflow" });

      await expect(api.workflows.get("invalid")).rejects.toThrow();
    });

    it("should map unknown mapsTo values through as-is", async () => {
      const invalidWorkflow = {
        ...createMockWorkflow(),
        columns: [{ id: "col", name: "Col", maps_to: "invalid_status" }],
      };
      mockInvoke.mockResolvedValue(invalidWorkflow);

      const result = await api.workflows.get("invalid");
      expect(result?.columns[0]?.mapsTo).toBe("invalid_status");
    });
  });

  describe("list", () => {
    it("should call get_workflows", async () => {
      mockInvoke.mockResolvedValue([createMockWorkflow()]);

      await api.workflows.list();

      expect(mockInvoke).toHaveBeenCalledWith("get_workflows", {});
    });

    it("should return array of workflows", async () => {
      const workflows = [
        createMockWorkflow({ id: "w1" }),
        createMockWorkflow({ id: "w2" }),
      ];
      mockInvoke.mockResolvedValue(workflows);

      const result = await api.workflows.list();

      expect(result).toHaveLength(2);
      expect(result[0]?.id).toBe("w1");
    });

    it("should validate workflow schema for each item", async () => {
      mockInvoke.mockResolvedValue([{ invalid: "workflow" }]);

      await expect(api.workflows.list()).rejects.toThrow();
    });
  });
});

// Helper to create mock QA settings
const createMockQASettings = (overrides = {}) => ({
  qa_enabled: true,
  auto_qa_for_ui_tasks: true,
  auto_qa_for_api_tasks: false,
  qa_prep_enabled: true,
  browser_testing_enabled: true,
  browser_testing_url: "http://localhost:1420",
  ...overrides,
});

// Helper to create mock TaskQA response
const createMockTaskQAResponse = (overrides = {}) => ({
  id: "qa-1",
  task_id: "task-1",
  screenshots: [],
  created_at: "2026-01-24T12:00:00Z",
  ...overrides,
});

// Helper to create mock QA results response
const createMockQAResultsResponse = (overrides = {}) => ({
  task_id: "task-1",
  overall_status: "passed",
  total_steps: 2,
  passed_steps: 2,
  failed_steps: 0,
  steps: [
    { step_id: "QA1", status: "passed" },
    { step_id: "QA2", status: "passed" },
  ],
  ...overrides,
});

describe("api.qa", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("getSettings", () => {
    it("should call get_qa_settings", async () => {
      mockInvoke.mockResolvedValue(createMockQASettings());

      await api.qa.getSettings();

      expect(mockInvoke).toHaveBeenCalledWith("get_qa_settings", {});
    });

    it("should return QA settings", async () => {
      const settings = createMockQASettings({ qa_enabled: false });
      mockInvoke.mockResolvedValue(settings);

      const result = await api.qa.getSettings();

      expect(result.qa_enabled).toBe(false);
      expect(result.auto_qa_for_ui_tasks).toBe(true);
    });

    it("should validate settings schema", async () => {
      mockInvoke.mockResolvedValue({ invalid: "settings" });

      await expect(api.qa.getSettings()).rejects.toThrow();
    });

    it("should require browser_testing_url to be valid URL", async () => {
      mockInvoke.mockResolvedValue(
        createMockQASettings({ browser_testing_url: "not-a-url" })
      );

      await expect(api.qa.getSettings()).rejects.toThrow();
    });
  });

  describe("updateSettings", () => {
    it("should call update_qa_settings with input", async () => {
      mockInvoke.mockResolvedValue(createMockQASettings({ qa_enabled: false }));
      const input = { qa_enabled: false };

      await api.qa.updateSettings(input);

      expect(mockInvoke).toHaveBeenCalledWith("update_qa_settings", { input });
    });

    it("should return updated settings", async () => {
      const settings = createMockQASettings({
        qa_enabled: false,
        browser_testing_url: "http://localhost:3000",
      });
      mockInvoke.mockResolvedValue(settings);

      const result = await api.qa.updateSettings({
        qa_enabled: false,
        browser_testing_url: "http://localhost:3000",
      });

      expect(result.qa_enabled).toBe(false);
      expect(result.browser_testing_url).toBe("http://localhost:3000");
    });

    it("should accept partial updates", async () => {
      mockInvoke.mockResolvedValue(createMockQASettings({ qa_enabled: false }));

      const result = await api.qa.updateSettings({ qa_enabled: false });

      expect(result.auto_qa_for_ui_tasks).toBe(true); // Unchanged
    });
  });

  describe("getTaskQA", () => {
    it("should call get_task_qa with taskId", async () => {
      mockInvoke.mockResolvedValue(createMockTaskQAResponse());

      await api.qa.getTaskQA("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_task_qa", {
        taskId: "task-1",
      });
    });

    it("should return null when no TaskQA exists", async () => {
      mockInvoke.mockResolvedValue(null);

      const result = await api.qa.getTaskQA("nonexistent");

      expect(result).toBeNull();
    });

    it("should return TaskQA when exists", async () => {
      const taskQA = createMockTaskQAResponse({ task_id: "task-123" });
      mockInvoke.mockResolvedValue(taskQA);

      const result = await api.qa.getTaskQA("task-123");

      expect(result?.task_id).toBe("task-123");
    });

    it("should parse acceptance_criteria correctly", async () => {
      const taskQA = createMockTaskQAResponse({
        acceptance_criteria: [
          { id: "AC1", description: "Test criterion", testable: true, criteria_type: "visual" },
        ],
      });
      mockInvoke.mockResolvedValue(taskQA);

      const result = await api.qa.getTaskQA("task-1");

      expect(result?.acceptance_criteria).toHaveLength(1);
      expect(result?.acceptance_criteria?.[0]?.criteria_type).toBe("visual");
    });

    it("should parse qa_test_steps correctly", async () => {
      const taskQA = createMockTaskQAResponse({
        qa_test_steps: [
          {
            id: "QA1",
            criteria_id: "AC1",
            description: "Verify task board",
            commands: ["agent-browser open http://localhost:1420"],
            expected: "Task board visible",
          },
        ],
      });
      mockInvoke.mockResolvedValue(taskQA);

      const result = await api.qa.getTaskQA("task-1");

      expect(result?.qa_test_steps).toHaveLength(1);
      expect(result?.qa_test_steps?.[0]?.commands).toContain(
        "agent-browser open http://localhost:1420"
      );
    });

    it("should parse test_results correctly", async () => {
      const taskQA = createMockTaskQAResponse({
        test_results: createMockQAResultsResponse({ overall_status: "failed", failed_steps: 1 }),
      });
      mockInvoke.mockResolvedValue(taskQA);

      const result = await api.qa.getTaskQA("task-1");

      expect(result?.test_results?.overall_status).toBe("failed");
      expect(result?.test_results?.failed_steps).toBe(1);
    });

    it("should validate TaskQA schema", async () => {
      mockInvoke.mockResolvedValue({ invalid: "taskqa" });

      await expect(api.qa.getTaskQA("task-1")).rejects.toThrow();
    });
  });

  describe("getResults", () => {
    it("should call get_qa_results with taskId", async () => {
      mockInvoke.mockResolvedValue(createMockQAResultsResponse());

      await api.qa.getResults("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_qa_results", {
        taskId: "task-1",
      });
    });

    it("should return null when no results", async () => {
      mockInvoke.mockResolvedValue(null);

      const result = await api.qa.getResults("task-1");

      expect(result).toBeNull();
    });

    it("should return results when available", async () => {
      const results = createMockQAResultsResponse();
      mockInvoke.mockResolvedValue(results);

      const result = await api.qa.getResults("task-1");

      expect(result?.overall_status).toBe("passed");
      expect(result?.total_steps).toBe(2);
    });

    it("should parse step results correctly", async () => {
      const results = createMockQAResultsResponse({
        steps: [
          { step_id: "QA1", status: "passed", screenshot: "ss1.png" },
          { step_id: "QA2", status: "failed", error: "Element not found" },
        ],
      });
      mockInvoke.mockResolvedValue(results);

      const result = await api.qa.getResults("task-1");

      expect(result?.steps[0]?.screenshot).toBe("ss1.png");
      expect(result?.steps[1]?.error).toBe("Element not found");
    });

    it("should validate results schema", async () => {
      mockInvoke.mockResolvedValue({ invalid: "results" });

      await expect(api.qa.getResults("task-1")).rejects.toThrow();
    });
  });

  describe("retry", () => {
    it("should call retry_qa with taskId", async () => {
      mockInvoke.mockResolvedValue(createMockTaskQAResponse());

      await api.qa.retry("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("retry_qa", { taskId: "task-1" });
    });

    it("should return updated TaskQA", async () => {
      const taskQA = createMockTaskQAResponse({
        test_results: createMockQAResultsResponse({
          overall_status: "pending",
          passed_steps: 0,
        }),
      });
      mockInvoke.mockResolvedValue(taskQA);

      const result = await api.qa.retry("task-1");

      expect(result.test_results?.overall_status).toBe("pending");
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("No QA record found"));

      await expect(api.qa.retry("nonexistent")).rejects.toThrow(
        "No QA record found"
      );
    });
  });

  describe("skip", () => {
    it("should call skip_qa with taskId", async () => {
      mockInvoke.mockResolvedValue(createMockTaskQAResponse());

      await api.qa.skip("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("skip_qa", { taskId: "task-1" });
    });

    it("should return TaskQA with skipped steps", async () => {
      const taskQA = createMockTaskQAResponse({
        test_results: {
          task_id: "task-1",
          overall_status: "passed",
          total_steps: 1,
          passed_steps: 0,
          failed_steps: 0,
          steps: [{ step_id: "QA1", status: "skipped" }],
        },
      });
      mockInvoke.mockResolvedValue(taskQA);

      const result = await api.qa.skip("task-1");

      expect(result.test_results?.steps[0]?.status).toBe("skipped");
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("No QA record found"));

      await expect(api.qa.skip("nonexistent")).rejects.toThrow(
        "No QA record found"
      );
    });
  });
});

// Helper to create mock review
const createMockReview = (overrides = {}) => ({
  id: "review-1",
  project_id: "project-1",
  task_id: "task-1",
  reviewer_type: "ai",
  status: "pending",
  notes: null,
  created_at: "2026-01-24T12:00:00Z",
  completed_at: null,
  ...overrides,
});

// Helper to create mock review note (state history)
const createMockReviewNote = (overrides = {}) => ({
  id: "note-1",
  task_id: "task-1",
  reviewer: "ai",
  outcome: "approved",
  notes: null,
  created_at: "2026-01-24T12:00:00Z",
  ...overrides,
});

describe("api.reviews", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("getPending", () => {
    it("should call get_pending_reviews with project_id", async () => {
      mockInvoke.mockResolvedValue([createMockReview()]);

      await api.reviews.getPending("project-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_pending_reviews", {
        project_id: "project-1",
      });
    });

    it("should return array of pending reviews", async () => {
      const reviews = [
        createMockReview({ id: "r1" }),
        createMockReview({ id: "r2" }),
      ];
      mockInvoke.mockResolvedValue(reviews);

      const result = await api.reviews.getPending("project-1");

      expect(result).toHaveLength(2);
      expect(result[0]?.id).toBe("r1");
    });

    it("should return empty array when no pending reviews", async () => {
      mockInvoke.mockResolvedValue([]);

      const result = await api.reviews.getPending("project-1");

      expect(result).toEqual([]);
    });

    it("should validate review schema", async () => {
      mockInvoke.mockResolvedValue([{ invalid: "review" }]);

      await expect(api.reviews.getPending("project-1")).rejects.toThrow();
    });
  });

  describe("getById", () => {
    it("should call get_review_by_id with review_id", async () => {
      mockInvoke.mockResolvedValue(createMockReview());

      await api.reviews.getById("review-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_review_by_id", {
        reviewId: "review-1",
      });
    });

    it("should return review when found", async () => {
      const review = createMockReview({ notes: "Looks good" });
      mockInvoke.mockResolvedValue(review);

      const result = await api.reviews.getById("review-1");

      expect(result?.notes).toBe("Looks good");
    });

    it("should return null when not found", async () => {
      mockInvoke.mockResolvedValue(null);

      const result = await api.reviews.getById("nonexistent");

      expect(result).toBeNull();
    });
  });

  describe("getByTaskId", () => {
    it("should call get_reviews_by_task_id with task_id", async () => {
      mockInvoke.mockResolvedValue([createMockReview()]);

      await api.reviews.getByTaskId("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_reviews_by_task_id", {
        taskId: "task-1",
      });
    });

    it("should return array of reviews for task", async () => {
      const reviews = [
        createMockReview({ id: "r1", reviewer_type: "ai" }),
        createMockReview({ id: "r2", reviewer_type: "human" }),
      ];
      mockInvoke.mockResolvedValue(reviews);

      const result = await api.reviews.getByTaskId("task-1");

      expect(result).toHaveLength(2);
      expect(result[0]?.reviewer_type).toBe("ai");
      expect(result[1]?.reviewer_type).toBe("human");
    });
  });

  describe("getTaskStateHistory", () => {
    it("should call get_task_state_history with task_id", async () => {
      mockInvoke.mockResolvedValue([createMockReviewNote()]);

      await api.reviews.getTaskStateHistory("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_task_state_history", {
        taskId: "task-1",
      });
    });

    it("should return array of review notes", async () => {
      const notes = [
        createMockReviewNote({ id: "n1", outcome: "approved" }),
        createMockReviewNote({ id: "n2", outcome: "changes_requested", notes: "Missing tests" }),
      ];
      mockInvoke.mockResolvedValue(notes);

      const result = await api.reviews.getTaskStateHistory("task-1");

      expect(result).toHaveLength(2);
      expect(result[0]?.outcome).toBe("approved");
      expect(result[1]?.notes).toBe("Missing tests");
    });

    it("should return empty array for task with no history", async () => {
      mockInvoke.mockResolvedValue([]);

      const result = await api.reviews.getTaskStateHistory("task-1");

      expect(result).toEqual([]);
    });
  });

  describe("approve", () => {
    it("should call approve_review with input", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { review_id: "review-1", notes: "LGTM" };

      await api.reviews.approve(input);

      expect(mockInvoke).toHaveBeenCalledWith("approve_review", { input });
    });

    it("should not require notes", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { review_id: "review-1" };

      await api.reviews.approve(input);

      expect(mockInvoke).toHaveBeenCalledWith("approve_review", { input });
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("Review not found"));

      await expect(
        api.reviews.approve({ review_id: "nonexistent" })
      ).rejects.toThrow("Review not found");
    });
  });

  describe("requestChanges", () => {
    it("should call request_changes with input", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { review_id: "review-1", notes: "Missing tests" };

      await api.reviews.requestChanges(input);

      expect(mockInvoke).toHaveBeenCalledWith("request_changes", { input });
    });

    it("should return fix task ID when fix_description provided", async () => {
      mockInvoke.mockResolvedValue("fix-task-123");
      const input = {
        review_id: "review-1",
        notes: "Missing tests",
        fix_description: "Add unit tests for validation",
      };

      const result = await api.reviews.requestChanges(input);

      expect(result).toBe("fix-task-123");
    });

    it("should return null when no fix_description", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { review_id: "review-1", notes: "Missing tests" };

      const result = await api.reviews.requestChanges(input);

      expect(result).toBeNull();
    });
  });

  describe("reject", () => {
    it("should call reject_review with input", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { review_id: "review-1", notes: "Fundamentally wrong approach" };

      await api.reviews.reject(input);

      expect(mockInvoke).toHaveBeenCalledWith("reject_review", { input });
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("Review not found"));

      await expect(
        api.reviews.reject({ review_id: "nonexistent", notes: "rejected" })
      ).rejects.toThrow("Review not found");
    });
  });

  describe("approveTask", () => {
    it("should call approve_task_for_review with input", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { task_id: "task-1", notes: "LGTM" };

      await api.reviews.approveTask(input);

      expect(mockInvoke).toHaveBeenCalledWith("approve_task_for_review", { input });
    });

    it("should not require notes", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { task_id: "task-1" };

      await api.reviews.approveTask(input);

      expect(mockInvoke).toHaveBeenCalledWith("approve_task_for_review", { input });
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("Task not found"));

      await expect(
        api.reviews.approveTask({ task_id: "nonexistent" })
      ).rejects.toThrow("Task not found");
    });
  });

  describe("requestTaskChanges", () => {
    it("should call request_task_changes_for_review with input", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { task_id: "task-1", feedback: "Missing tests" };

      await api.reviews.requestTaskChanges(input);

      expect(mockInvoke).toHaveBeenCalledWith("request_task_changes_for_review", { input });
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("Task not found"));

      await expect(
        api.reviews.requestTaskChanges({ task_id: "nonexistent", feedback: "N/A" })
      ).rejects.toThrow("Task not found");
    });
  });

  describe("reReviewTask", () => {
    it("should call re_review_task_from_escalated with input", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { task_id: "task-1" };

      await api.reviews.reReviewTask(input);

      expect(mockInvoke).toHaveBeenCalledWith("re_review_task_from_escalated", { input });
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("Task not escalated"));

      await expect(
        api.reviews.reReviewTask({ task_id: "nonexistent" })
      ).rejects.toThrow("Task not escalated");
    });
  });
});

describe("api.fixTasks", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  describe("approve", () => {
    it("should call approve_fix_task with input", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = { fix_task_id: "fix-task-1" };

      await api.fixTasks.approve(input);

      expect(mockInvoke).toHaveBeenCalledWith("approve_fix_task", { input });
    });

    it("should propagate errors", async () => {
      mockInvoke.mockRejectedValue(new Error("Fix task not found"));

      await expect(
        api.fixTasks.approve({ fix_task_id: "nonexistent" })
      ).rejects.toThrow("Fix task not found");
    });
  });

  describe("reject", () => {
    it("should call reject_fix_task with input", async () => {
      mockInvoke.mockResolvedValue("new-fix-task-123");
      const input = {
        fix_task_id: "fix-task-1",
        feedback: "Try a different approach",
        original_task_id: "task-1",
      };

      await api.fixTasks.reject(input);

      expect(mockInvoke).toHaveBeenCalledWith("reject_fix_task", { input });
    });

    it("should return new fix task ID when under max attempts", async () => {
      mockInvoke.mockResolvedValue("new-fix-task-123");
      const input = {
        fix_task_id: "fix-task-1",
        feedback: "Try a different approach",
        original_task_id: "task-1",
      };

      const result = await api.fixTasks.reject(input);

      expect(result).toBe("new-fix-task-123");
    });

    it("should return null when max attempts reached (moved to backlog)", async () => {
      mockInvoke.mockResolvedValue(null);
      const input = {
        fix_task_id: "fix-task-3",
        feedback: "Still not right",
        original_task_id: "task-1",
      };

      const result = await api.fixTasks.reject(input);

      expect(result).toBeNull();
    });
  });

  describe("getAttempts", () => {
    it("should call get_fix_task_attempts with task_id", async () => {
      mockInvoke.mockResolvedValue({ task_id: "task-1", attempt_count: 2 });

      await api.fixTasks.getAttempts("task-1");

      expect(mockInvoke).toHaveBeenCalledWith("get_fix_task_attempts", {
        taskId: "task-1",
      });
    });

    it("should return attempt count", async () => {
      mockInvoke.mockResolvedValue({ task_id: "task-1", attempt_count: 2 });

      const result = await api.fixTasks.getAttempts("task-1");

      expect(result.task_id).toBe("task-1");
      expect(result.attempt_count).toBe(2);
    });

    it("should return zero when no attempts", async () => {
      mockInvoke.mockResolvedValue({ task_id: "task-1", attempt_count: 0 });

      const result = await api.fixTasks.getAttempts("task-1");

      expect(result.attempt_count).toBe(0);
    });

    it("should validate response schema", async () => {
      mockInvoke.mockResolvedValue({ invalid: "response" });

      await expect(api.fixTasks.getAttempts("task-1")).rejects.toThrow();
    });
  });
});
