import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  getWorkflows,
  getWorkflow,
  createWorkflow,
  updateWorkflow,
  setDefaultWorkflow,
  getActiveWorkflowColumns,
  getBuiltinWorkflows,
  WorkflowResponseSchema,
  WorkflowColumnResponseSchema,
  CreateWorkflowInputSchema,
  UpdateWorkflowInputSchema,
} from "./workflows";

// Mock Tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

// Test data helpers
const createMockWorkflowColumn = (overrides = {}) => ({
  id: "backlog",
  name: "Backlog",
  maps_to: "backlog",
  color: "",
  icon: "",
  skip_review: false,
  auto_advance: false,
  agent_profile: "",
  ...overrides,
});

const createMockWorkflow = (overrides = {}) => ({
  id: "test-workflow-1",
  name: "Test Workflow",
  description: "A test workflow",
  columns: [
    createMockWorkflowColumn({ id: "backlog", name: "Backlog", maps_to: "backlog" }),
    createMockWorkflowColumn({ id: "ready", name: "Ready", maps_to: "ready" }),
    createMockWorkflowColumn({ id: "done", name: "Done", maps_to: "approved" }),
  ],
  is_default: false,
  worker_profile: "",
  reviewer_profile: "",
  ...overrides,
});

const createMockLinearSync = () => ({
  provider: "linear",
  mapping: {
    "In Progress": {
      external_status: "In Progress",
      internal_status: "executing",
      column_id: "in_progress",
    },
  },
  sync: {
    direction: "bidirectional",
    webhook: true,
  },
  conflict_resolution: "external_wins",
});

describe("WorkflowColumnResponseSchema", () => {
  it("should parse valid column response", () => {
    const column = createMockWorkflowColumn();
    expect(() => WorkflowColumnResponseSchema.parse(column)).not.toThrow();
  });

  it("should parse column with all optional fields", () => {
    const column = createMockWorkflowColumn({
      color: "#ff6b35",
      icon: "rocket",
      skip_review: true,
      auto_advance: false,
      agent_profile: "fast-worker",
    });
    const result = WorkflowColumnResponseSchema.parse(column);
    expect(result.color).toBe("#ff6b35");
    expect(result.skip_review).toBe(true);
  });

  it("should allow any string for maps_to in backend response schema", () => {
    const column = createMockWorkflowColumn({ maps_to: "invalid_status" });
    expect(() => WorkflowColumnResponseSchema.parse(column)).not.toThrow();
  });

  it("should reject column without required fields", () => {
    expect(() => WorkflowColumnResponseSchema.parse({})).toThrow();
    expect(() => WorkflowColumnResponseSchema.parse({ id: "col" })).toThrow();
  });
});

describe("WorkflowResponseSchema", () => {
  it("should parse valid workflow response", () => {
    const workflow = createMockWorkflow();
    expect(() => WorkflowResponseSchema.parse(workflow)).not.toThrow();
  });

  it("should parse workflow with all optional fields", () => {
    const workflow = createMockWorkflow({
      worker_profile: "fast-worker",
      reviewer_profile: "strict-reviewer",
      external_sync: createMockLinearSync(),
    });
    const result = WorkflowResponseSchema.parse(workflow);
    expect(result.worker_profile).toBe("fast-worker");
    expect(result.reviewer_profile).toBe("strict-reviewer");
    expect(result.external_sync?.provider).toBe("linear");
    expect(result.external_sync?.mapping["In Progress"]?.column_id).toBe("in_progress");
  });

  it("should parse workflow without description", () => {
    const workflow = createMockWorkflow({ description: undefined });
    const result = WorkflowResponseSchema.parse(workflow);
    expect(result.description).toBeUndefined();
  });

  it("should reject workflow without required fields", () => {
    expect(() => WorkflowResponseSchema.parse({})).toThrow();
    expect(() => WorkflowResponseSchema.parse({ id: "wf" })).toThrow();
  });

  it("should allow workflow with empty columns in raw response schema", () => {
    const workflow = createMockWorkflow({ columns: [] });
    expect(() => WorkflowResponseSchema.parse(workflow)).not.toThrow();
  });

  it("should allow columns with unmapped maps_to values in raw response schema", () => {
    const workflow = createMockWorkflow({
      columns: [createMockWorkflowColumn({ maps_to: "invalid_status" })],
    });
    expect(() => WorkflowResponseSchema.parse(workflow)).not.toThrow();
  });
});

describe("CreateWorkflowInputSchema", () => {
  it("should parse valid create input", () => {
    const input = {
      name: "New Workflow",
      columns: [
        { id: "backlog", name: "Backlog", maps_to: "backlog" },
        { id: "done", name: "Done", maps_to: "approved" },
      ],
    };
    expect(() => CreateWorkflowInputSchema.parse(input)).not.toThrow();
  });

  it("should parse input with all optional fields", () => {
    const input = {
      name: "Full Workflow",
      description: "A full workflow",
      columns: [{ id: "col", name: "Column", maps_to: "ready" }],
      is_default: true,
      worker_profile: "worker-1",
      reviewer_profile: "reviewer-1",
      external_sync: createMockLinearSync(),
    };
    const result = CreateWorkflowInputSchema.parse(input);
    expect(result.is_default).toBe(true);
    expect(result.worker_profile).toBe("worker-1");
    expect(result.external_sync?.provider).toBe("linear");
  });

  it("should reject input without name", () => {
    const input = {
      columns: [{ id: "col", name: "Column", maps_to: "ready" }],
    };
    expect(() => CreateWorkflowInputSchema.parse(input)).toThrow();
  });

  it("should reject input without columns", () => {
    const input = { name: "No Columns" };
    expect(() => CreateWorkflowInputSchema.parse(input)).toThrow();
  });

  it("should reject input with empty columns array", () => {
    const input = { name: "Empty Columns", columns: [] };
    expect(() => CreateWorkflowInputSchema.parse(input)).toThrow();
  });

  it("should parse column with behavior options", () => {
    const input = {
      name: "Behavioral",
      columns: [
        {
          id: "auto",
          name: "Auto Advance",
          maps_to: "executing",
          skip_review: true,
          auto_advance: true,
          agent_profile: "speedy",
        },
      ],
    };
    const result = CreateWorkflowInputSchema.parse(input);
    expect(result.columns[0]?.skip_review).toBe(true);
    expect(result.columns[0]?.auto_advance).toBe(true);
  });
});

describe("UpdateWorkflowInputSchema", () => {
  it("should parse partial update (name only)", () => {
    const input = { name: "Updated Name" };
    expect(() => UpdateWorkflowInputSchema.parse(input)).not.toThrow();
  });

  it("should parse partial update (columns only)", () => {
    const input = {
      columns: [{ id: "new", name: "New Column", maps_to: "ready" }],
    };
    expect(() => UpdateWorkflowInputSchema.parse(input)).not.toThrow();
  });

  it("should parse full update", () => {
    const input = {
      name: "Updated",
      description: "Updated desc",
      columns: [{ id: "col", name: "Col", maps_to: "backlog" }],
      is_default: true,
      worker_profile: "new-worker",
      reviewer_profile: "new-reviewer",
      external_sync: createMockLinearSync(),
    };
    const result = UpdateWorkflowInputSchema.parse(input);
    expect(result.name).toBe("Updated");
    expect(result.is_default).toBe(true);
    expect(result.external_sync?.mapping["In Progress"]?.internal_status).toBe("executing");
  });

  it("should allow empty object (no changes)", () => {
    const input = {};
    expect(() => UpdateWorkflowInputSchema.parse(input)).not.toThrow();
  });

  it("should reject if columns array is provided but empty", () => {
    const input = { columns: [] };
    expect(() => UpdateWorkflowInputSchema.parse(input)).toThrow();
  });
});

describe("getWorkflows", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call get_workflows command", async () => {
    mockInvoke.mockResolvedValue([createMockWorkflow()]);

    await getWorkflows();

    expect(mockInvoke).toHaveBeenCalledWith("get_workflows", {});
  });

  it("should return validated array of workflows", async () => {
    const workflows = [
      createMockWorkflow({ id: "wf1", name: "Workflow 1" }),
      createMockWorkflow({ id: "wf2", name: "Workflow 2" }),
    ];
    mockInvoke.mockResolvedValue(workflows);

    const result = await getWorkflows();

    expect(result).toHaveLength(2);
    expect(result[0]?.name).toBe("Workflow 1");
    expect(result[1]?.name).toBe("Workflow 2");
  });

  it("should return empty array when no workflows", async () => {
    mockInvoke.mockResolvedValue([]);

    const result = await getWorkflows();

    expect(result).toEqual([]);
  });

  it("should throw on invalid response", async () => {
    mockInvoke.mockResolvedValue([{ invalid: "workflow" }]);

    await expect(getWorkflows()).rejects.toThrow();
  });

  it("should propagate backend errors", async () => {
    mockInvoke.mockRejectedValue(new Error("Database error"));

    await expect(getWorkflows()).rejects.toThrow("Database error");
  });
});

describe("getWorkflow", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call get_workflow command with id", async () => {
    mockInvoke.mockResolvedValue(createMockWorkflow());

    await getWorkflow("wf-123");

    expect(mockInvoke).toHaveBeenCalledWith("get_workflow", { id: "wf-123" });
  });

  it("should return validated workflow", async () => {
    const workflow = createMockWorkflow({ name: "Found Workflow" });
    mockInvoke.mockResolvedValue(workflow);

    const result = await getWorkflow("wf-123");

    expect(result?.name).toBe("Found Workflow");
  });

  it("should return null when workflow not found", async () => {
    mockInvoke.mockResolvedValue(null);

    const result = await getWorkflow("nonexistent");

    expect(result).toBeNull();
  });

  it("should throw on invalid response", async () => {
    mockInvoke.mockResolvedValue({ invalid: "workflow" });

    await expect(getWorkflow("wf-123")).rejects.toThrow();
  });
});

describe("createWorkflow", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call create_workflow command with input", async () => {
    mockInvoke.mockResolvedValue(createMockWorkflow());
    const input = {
      name: "New Workflow",
      columns: [{ id: "col", name: "Column", maps_to: "ready" as const }],
    };

    await createWorkflow(input);

    expect(mockInvoke).toHaveBeenCalledWith("create_workflow", { input });
  });

  it("should return created workflow", async () => {
    const created = createMockWorkflow({ name: "Created" });
    mockInvoke.mockResolvedValue(created);

    const result = await createWorkflow({
      name: "Created",
      columns: [{ id: "col", name: "Column", maps_to: "ready" }],
    });

    expect(result.name).toBe("Created");
  });

  it("should validate input before sending", async () => {
    const invalidInput = { name: "No Columns" } as never;

    await expect(createWorkflow(invalidInput)).rejects.toThrow();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("should throw on invalid response", async () => {
    mockInvoke.mockResolvedValue({ invalid: "workflow" });

    await expect(
      createWorkflow({
        name: "Test",
        columns: [{ id: "col", name: "Col", maps_to: "ready" }],
      })
    ).rejects.toThrow();
  });
});

describe("updateWorkflow", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call update_workflow command with id and input", async () => {
    mockInvoke.mockResolvedValue(createMockWorkflow());
    const input = { name: "Updated Name" };

    await updateWorkflow("wf-123", input);

    expect(mockInvoke).toHaveBeenCalledWith("update_workflow", {
      id: "wf-123",
      input,
    });
  });

  it("should return updated workflow", async () => {
    const updated = createMockWorkflow({ name: "Updated" });
    mockInvoke.mockResolvedValue(updated);

    const result = await updateWorkflow("wf-123", { name: "Updated" });

    expect(result.name).toBe("Updated");
  });

  it("should allow partial updates", async () => {
    mockInvoke.mockResolvedValue(createMockWorkflow({ description: "New desc" }));

    const result = await updateWorkflow("wf-123", { description: "New desc" });

    expect(result.description).toBe("New desc");
  });

  it("should validate input before sending", async () => {
    const invalidInput = { columns: [] };

    await expect(updateWorkflow("wf-123", invalidInput)).rejects.toThrow();
    expect(mockInvoke).not.toHaveBeenCalled();
  });
});

describe("setDefaultWorkflow", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call set_default_workflow command with id", async () => {
    mockInvoke.mockResolvedValue(createMockWorkflow({ is_default: true }));

    await setDefaultWorkflow("wf-123");

    expect(mockInvoke).toHaveBeenCalledWith("set_default_workflow", { id: "wf-123" });
  });

  it("should return updated workflow with is_default true", async () => {
    const workflow = createMockWorkflow({ is_default: true });
    mockInvoke.mockResolvedValue(workflow);

    const result = await setDefaultWorkflow("wf-123");

    expect(result.isDefault).toBe(true);
  });

  it("should throw on invalid response", async () => {
    mockInvoke.mockResolvedValue({ invalid: "workflow" });

    await expect(setDefaultWorkflow("wf-123")).rejects.toThrow();
  });
});

describe("getActiveWorkflowColumns", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call get_active_workflow_columns command", async () => {
    mockInvoke.mockResolvedValue([createMockWorkflowColumn()]);

    await getActiveWorkflowColumns();

    expect(mockInvoke).toHaveBeenCalledWith("get_active_workflow_columns", {});
  });

  it("should return validated array of columns", async () => {
    const columns = [
      createMockWorkflowColumn({ id: "col1", name: "Column 1" }),
      createMockWorkflowColumn({ id: "col2", name: "Column 2" }),
    ];
    mockInvoke.mockResolvedValue(columns);

    const result = await getActiveWorkflowColumns();

    expect(result).toHaveLength(2);
    expect(result[0]?.name).toBe("Column 1");
  });

  it("should throw on invalid column response", async () => {
    mockInvoke.mockResolvedValue([{ invalid: "column" }]);

    await expect(getActiveWorkflowColumns()).rejects.toThrow();
  });
});

describe("getBuiltinWorkflows", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("should call get_builtin_workflows command", async () => {
    mockInvoke.mockResolvedValue([
      createMockWorkflow({ id: "ralphx-default", name: "RalphX Default" }),
      createMockWorkflow({ id: "jira-compat", name: "Jira Compatible" }),
      createMockWorkflow({
        id: "linear-compat",
        name: "Linear Compatible",
        external_sync: createMockLinearSync(),
      }),
    ]);

    await getBuiltinWorkflows();

    expect(mockInvoke).toHaveBeenCalledWith("get_builtin_workflows", {});
  });

  it("should return validated builtin workflows", async () => {
    const builtins = [
      createMockWorkflow({ id: "ralphx-default", name: "RalphX Default" }),
      createMockWorkflow({ id: "jira-compat", name: "Jira Compatible" }),
      createMockWorkflow({
        id: "linear-compat",
        name: "Linear Compatible",
        external_sync: createMockLinearSync(),
      }),
    ];
    mockInvoke.mockResolvedValue(builtins);

    const result = await getBuiltinWorkflows();

    expect(result).toHaveLength(3);
    expect(result.map((w) => w.name)).toContain("RalphX Default");
    expect(result.map((w) => w.name)).toContain("Jira Compatible");
    expect(result.map((w) => w.name)).toContain("Linear Compatible");
    expect(result.find((w) => w.id === "linear-compat")?.externalSync?.provider).toBe("linear");
  });

  it("should throw on invalid response", async () => {
    mockInvoke.mockResolvedValue([{ invalid: "workflow" }]);

    await expect(getBuiltinWorkflows()).rejects.toThrow();
  });
});
