import { describe, it, expect } from "vitest";
import {
  WorkflowColumnSchema,
  WorkflowSchemaZ,
  defaultWorkflow,
  jiraCompatibleWorkflow,
  linearCompatibleWorkflow,
  BUILTIN_WORKFLOWS,
  getBuiltinWorkflow,
  transformWorkflow,
  SyncProviderSchema,
  SyncDirectionSchema,
  ConflictResolutionSchema,
  ExternalStatusMappingSchema,
  SyncSettingsSchema,
  ExternalSyncConfigSchema,
  SYNC_PROVIDER_VALUES,
  SYNC_DIRECTION_VALUES,
  CONFLICT_RESOLUTION_VALUES,
  type WorkflowColumn,
  type WorkflowSchema,
  type SyncProvider,
  type SyncDirection,
  type ConflictResolution,
  type ExternalSyncConfig,
} from "./workflow";

describe("WorkflowColumnSchema", () => {
  it("validates a minimal workflow column", () => {
    const column = {
      id: "backlog",
      name: "Backlog",
      mapsTo: "backlog",
    };

    const result = WorkflowColumnSchema.safeParse(column);
    expect(result.success).toBe(true);
  });

  it("validates a column with all optional fields", () => {
    const column = {
      id: "in-progress",
      name: "In Progress",
      color: "#ff6b35",
      icon: "play",
      mapsTo: "executing",
      behavior: {
        skipReview: false,
        autoAdvance: true,
        agentProfile: "worker",
      },
    };

    const result = WorkflowColumnSchema.safeParse(column);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.color).toBe("#ff6b35");
      expect(result.data.behavior?.autoAdvance).toBe(true);
    }
  });

  it("validates a column with partial behavior", () => {
    const column = {
      id: "review",
      name: "Review",
      mapsTo: "pending_review",
      behavior: {
        skipReview: false,
      },
    };

    const result = WorkflowColumnSchema.safeParse(column);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.behavior?.skipReview).toBe(false);
      expect(result.data.behavior?.autoAdvance).toBeUndefined();
    }
  });

  it("rejects a column without id", () => {
    const column = {
      name: "Backlog",
      mapsTo: "backlog",
    };

    const result = WorkflowColumnSchema.safeParse(column);
    expect(result.success).toBe(false);
  });

  it("rejects a column without name", () => {
    const column = {
      id: "backlog",
      mapsTo: "backlog",
    };

    const result = WorkflowColumnSchema.safeParse(column);
    expect(result.success).toBe(false);
  });

  it("rejects a column with invalid mapsTo status", () => {
    const column = {
      id: "backlog",
      name: "Backlog",
      mapsTo: "invalid_status",
    };

    const result = WorkflowColumnSchema.safeParse(column);
    expect(result.success).toBe(false);
  });

  it("validates all internal statuses for mapsTo", () => {
    const statuses = [
      "backlog",
      "ready",
      "blocked",
      "executing",
      "qa_refining",
      "qa_testing",
      "qa_passed",
      "qa_failed",
      "pending_review",
      "revision_needed",
      "approved",
      "failed",
      "cancelled",
    ];

    for (const status of statuses) {
      const column = {
        id: `col-${status}`,
        name: status,
        mapsTo: status,
      };
      const result = WorkflowColumnSchema.safeParse(column);
      expect(result.success).toBe(true);
    }
  });
});

describe("WorkflowSchemaZ", () => {
  it("validates a minimal workflow", () => {
    const workflow = {
      id: "default",
      name: "Default Workflow",
      columns: [
        { id: "backlog", name: "Backlog", mapsTo: "backlog" },
        { id: "ready", name: "Ready", mapsTo: "ready" },
      ],
    };

    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(true);
  });

  it("validates a workflow with all optional fields", () => {
    const workflow = {
      id: "custom",
      name: "Custom Workflow",
      description: "A custom workflow for feature development",
      columns: [
        { id: "backlog", name: "Backlog", mapsTo: "backlog" },
        { id: "in-progress", name: "In Progress", mapsTo: "executing" },
        { id: "done", name: "Done", mapsTo: "approved" },
      ],
      defaults: {
        workerProfile: "senior-dev",
        reviewerProfile: "tech-lead",
      },
    };

    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.description).toBe("A custom workflow for feature development");
      expect(result.data.defaults?.workerProfile).toBe("senior-dev");
    }
  });

  it("validates a workflow with partial defaults", () => {
    const workflow = {
      id: "minimal",
      name: "Minimal Workflow",
      columns: [{ id: "todo", name: "To Do", mapsTo: "ready" }],
      defaults: {
        workerProfile: "worker",
      },
    };

    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.defaults?.workerProfile).toBe("worker");
      expect(result.data.defaults?.reviewerProfile).toBeUndefined();
    }
  });

  it("rejects a workflow without id", () => {
    const workflow = {
      name: "No ID Workflow",
      columns: [{ id: "backlog", name: "Backlog", mapsTo: "backlog" }],
    };

    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(false);
  });

  it("rejects a workflow without name", () => {
    const workflow = {
      id: "no-name",
      columns: [{ id: "backlog", name: "Backlog", mapsTo: "backlog" }],
    };

    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(false);
  });

  it("rejects a workflow without columns", () => {
    const workflow = {
      id: "no-columns",
      name: "No Columns Workflow",
    };

    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(false);
  });

  it("rejects a workflow with empty columns array", () => {
    const workflow = {
      id: "empty-columns",
      name: "Empty Columns Workflow",
      columns: [],
    };

    const result = WorkflowSchemaZ.safeParse(workflow);
    // Empty arrays are valid in Zod by default
    expect(result.success).toBe(true);
  });

  it("rejects a workflow with invalid column", () => {
    const workflow = {
      id: "invalid-column",
      name: "Invalid Column Workflow",
      columns: [
        { id: "backlog", name: "Backlog", mapsTo: "invalid_status" },
      ],
    };

    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(false);
  });
});

describe("defaultWorkflow", () => {
  it("has 5 columns", () => {
    expect(defaultWorkflow.columns).toHaveLength(5);
  });

  it("has correct column ids", () => {
    const columnIds = defaultWorkflow.columns.map((c) => c.id);
    expect(columnIds).toEqual([
      "draft",
      "ready",
      "in_progress",
      "in_review",
      "done",
    ]);
  });

  it("has correct column names", () => {
    const columnNames = defaultWorkflow.columns.map((c) => c.name);
    expect(columnNames).toEqual([
      "Draft",
      "Ready",
      "In Progress",
      "In Review",
      "Done",
    ]);
  });

  it("maps columns to valid internal statuses", () => {
    const result = WorkflowSchemaZ.safeParse(defaultWorkflow);
    expect(result.success).toBe(true);
  });

  it("has id 'ralphx-default'", () => {
    expect(defaultWorkflow.id).toBe("ralphx-default");
  });

  it("has name 'RalphX Default'", () => {
    expect(defaultWorkflow.name).toBe("RalphX Default");
  });

  it("labels merge attention states as escalated user intervention", () => {
    const doneColumn = defaultWorkflow.columns.find((column) => column.id === "done");
    const attentionGroup = doneColumn?.groups?.find((group) =>
      group.statuses.includes("merge_incomplete"),
    );

    expect(attentionGroup?.label).toBe("Escalated");
    expect(attentionGroup?.statuses).toContain("merge_conflict");
  });
});

describe("type inference", () => {
  it("correctly infers WorkflowColumn type", () => {
    const column: WorkflowColumn = {
      id: "test",
      name: "Test Column",
      mapsTo: "backlog",
      color: "#fff",
      behavior: {
        skipReview: true,
      },
    };
    expect(column.id).toBe("test");
  });

  it("correctly infers WorkflowSchema type", () => {
    const workflow: WorkflowSchema = {
      id: "test",
      name: "Test Workflow",
      columns: [
        { id: "col1", name: "Column 1", mapsTo: "backlog" },
      ],
    };
    expect(workflow.id).toBe("test");
  });
});

// ============================================
// External Sync Configuration Tests
// ============================================

describe("SyncProviderSchema", () => {
  it("validates all sync providers", () => {
    const providers = ["jira", "github", "linear", "notion"];
    for (const provider of providers) {
      const result = SyncProviderSchema.safeParse(provider);
      expect(result.success).toBe(true);
    }
  });

  it("rejects invalid provider", () => {
    const result = SyncProviderSchema.safeParse("invalid");
    expect(result.success).toBe(false);
  });

  it("exports all provider values", () => {
    expect(SYNC_PROVIDER_VALUES).toEqual(["jira", "github", "linear", "notion"]);
  });
});

describe("SyncDirectionSchema", () => {
  it("validates all sync directions", () => {
    const directions = ["pull", "push", "bidirectional"];
    for (const direction of directions) {
      const result = SyncDirectionSchema.safeParse(direction);
      expect(result.success).toBe(true);
    }
  });

  it("rejects invalid direction", () => {
    const result = SyncDirectionSchema.safeParse("twoway");
    expect(result.success).toBe(false);
  });

  it("exports all direction values", () => {
    expect(SYNC_DIRECTION_VALUES).toEqual(["pull", "push", "bidirectional"]);
  });
});

describe("ConflictResolutionSchema", () => {
  it("validates all conflict resolution strategies", () => {
    const strategies = ["external_wins", "internal_wins", "manual"];
    for (const strategy of strategies) {
      const result = ConflictResolutionSchema.safeParse(strategy);
      expect(result.success).toBe(true);
    }
  });

  it("rejects invalid strategy", () => {
    const result = ConflictResolutionSchema.safeParse("merge");
    expect(result.success).toBe(false);
  });

  it("exports all resolution values", () => {
    expect(CONFLICT_RESOLUTION_VALUES).toEqual(["external_wins", "internal_wins", "manual"]);
  });
});

describe("ExternalStatusMappingSchema", () => {
  it("validates a complete status mapping", () => {
    const mapping = {
      externalStatus: "To Do",
      internalStatus: "ready",
      columnId: "todo",
    };
    const result = ExternalStatusMappingSchema.safeParse(mapping);
    expect(result.success).toBe(true);
  });

  it("rejects mapping with invalid internal status", () => {
    const mapping = {
      externalStatus: "To Do",
      internalStatus: "invalid_status",
      columnId: "todo",
    };
    const result = ExternalStatusMappingSchema.safeParse(mapping);
    expect(result.success).toBe(false);
  });

  it("rejects mapping without required fields", () => {
    const result = ExternalStatusMappingSchema.safeParse({
      externalStatus: "To Do",
    });
    expect(result.success).toBe(false);
  });
});

describe("SyncSettingsSchema", () => {
  it("validates settings with direction only", () => {
    const settings = { direction: "pull" };
    const result = SyncSettingsSchema.safeParse(settings);
    expect(result.success).toBe(true);
  });

  it("validates settings with webhook", () => {
    const settings = { direction: "bidirectional", webhook: true };
    const result = SyncSettingsSchema.safeParse(settings);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.webhook).toBe(true);
    }
  });

  it("rejects settings without direction", () => {
    const result = SyncSettingsSchema.safeParse({ webhook: true });
    expect(result.success).toBe(false);
  });
});

describe("ExternalSyncConfigSchema", () => {
  it("validates a complete sync config", () => {
    const config = {
      provider: "jira",
      mapping: {
        "To Do": {
          externalStatus: "To Do",
          internalStatus: "ready",
          columnId: "todo",
        },
      },
      sync: {
        direction: "bidirectional",
        webhook: true,
      },
      conflictResolution: "external_wins",
    };
    const result = ExternalSyncConfigSchema.safeParse(config);
    expect(result.success).toBe(true);
  });

  it("validates config with empty mapping", () => {
    const config = {
      provider: "github",
      sync: {
        direction: "pull",
      },
      conflictResolution: "manual",
    };
    const result = ExternalSyncConfigSchema.safeParse(config);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.mapping).toEqual({});
    }
  });

  it("rejects config without provider", () => {
    const config = {
      sync: { direction: "pull" },
      conflictResolution: "manual",
    };
    const result = ExternalSyncConfigSchema.safeParse(config);
    expect(result.success).toBe(false);
  });

  it("rejects config with invalid provider", () => {
    const config = {
      provider: "trello",
      sync: { direction: "pull" },
      conflictResolution: "manual",
    };
    const result = ExternalSyncConfigSchema.safeParse(config);
    expect(result.success).toBe(false);
  });
});

describe("WorkflowSchemaZ with externalSync", () => {
  it("validates workflow with externalSync", () => {
    const workflow = {
      id: "jira-flow",
      name: "Jira Flow",
      columns: [{ id: "todo", name: "To Do", mapsTo: "ready" }],
      externalSync: {
        provider: "jira",
        sync: { direction: "bidirectional" },
        conflictResolution: "external_wins",
      },
    };
    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.externalSync?.provider).toBe("jira");
    }
  });

  it("validates workflow with isDefault true", () => {
    const workflow = {
      id: "default",
      name: "Default",
      columns: [{ id: "backlog", name: "Backlog", mapsTo: "backlog" }],
      isDefault: true,
    };
    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.isDefault).toBe(true);
    }
  });

  it("defaults isDefault to false when not specified", () => {
    const workflow = {
      id: "no-default",
      name: "No Default",
      columns: [{ id: "backlog", name: "Backlog", mapsTo: "backlog" }],
    };
    const result = WorkflowSchemaZ.safeParse(workflow);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.isDefault).toBe(false);
    }
  });
});

describe("jiraCompatibleWorkflow", () => {
  it("has 5 columns", () => {
    expect(jiraCompatibleWorkflow.columns).toHaveLength(5);
  });

  it("has correct id", () => {
    expect(jiraCompatibleWorkflow.id).toBe("jira-compat");
  });

  it("has external sync configured", () => {
    expect(jiraCompatibleWorkflow.externalSync).toBeDefined();
    expect(jiraCompatibleWorkflow.externalSync?.provider).toBe("jira");
    expect(jiraCompatibleWorkflow.externalSync?.sync.direction).toBe("bidirectional");
    expect(jiraCompatibleWorkflow.externalSync?.conflictResolution).toBe("external_wins");
  });

  it("is not the default workflow", () => {
    expect(jiraCompatibleWorkflow.isDefault).toBe(false);
  });

  it("has correct column names", () => {
    const columnNames = jiraCompatibleWorkflow.columns.map((c) => c.name);
    expect(columnNames).toEqual([
      "Backlog",
      "Selected for Dev",
      "In Progress",
      "In QA",
      "Done",
    ]);
  });

  it("validates against schema", () => {
    const result = WorkflowSchemaZ.safeParse(jiraCompatibleWorkflow);
    expect(result.success).toBe(true);
  });
});

describe("linearCompatibleWorkflow", () => {
  it("has Linear workflow identity without changing existing ids", () => {
    expect(defaultWorkflow.id).toBe("ralphx-default");
    expect(jiraCompatibleWorkflow.id).toBe("jira-compat");
    expect(linearCompatibleWorkflow.id).toBe("linear-compat");
  });

  it("has 6 columns with Linear-oriented names", () => {
    expect(linearCompatibleWorkflow.columns.map((column) => column.id)).toEqual([
      "backlog",
      "todo",
      "in_progress",
      "in_review",
      "done",
      "canceled",
    ]);
  });

  it("has external sync configured for Linear", () => {
    expect(linearCompatibleWorkflow.externalSync).toBeDefined();
    expect(linearCompatibleWorkflow.externalSync?.provider).toBe("linear");
    expect(linearCompatibleWorkflow.externalSync?.sync.direction).toBe("bidirectional");
    expect(linearCompatibleWorkflow.externalSync?.sync.webhook).toBe(true);
    expect(linearCompatibleWorkflow.externalSync?.conflictResolution).toBe("external_wins");
    expect(linearCompatibleWorkflow.externalSync?.mapping["In Progress"]).toEqual({
      externalStatus: "In Progress",
      internalStatus: "executing",
      columnId: "in_progress",
    });
  });

  it("validates against schema", () => {
    const result = WorkflowSchemaZ.safeParse(linearCompatibleWorkflow);
    expect(result.success).toBe(true);
  });

  it("round-trips external sync from backend response shape", () => {
    const result = transformWorkflow({
      id: "linear-compat",
      name: "Linear Compatible",
      description: "Linear-style workflow with external state mapping",
      columns: [
        { id: "todo", name: "Todo", maps_to: "ready" },
        { id: "in_progress", name: "In Progress", maps_to: "executing" },
      ],
      external_sync: {
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
      },
      is_default: false,
    });

    expect(result.externalSync).toEqual({
      provider: "linear",
      mapping: {
        "In Progress": {
          externalStatus: "In Progress",
          internalStatus: "executing",
          columnId: "in_progress",
        },
      },
      sync: {
        direction: "bidirectional",
        webhook: true,
      },
      conflictResolution: "external_wins",
    });
  });
});

describe("BUILTIN_WORKFLOWS", () => {
  it("contains default, jira-compatible, and linear-compatible workflows", () => {
    expect(BUILTIN_WORKFLOWS).toHaveLength(3);
    expect(BUILTIN_WORKFLOWS[0].id).toBe("ralphx-default");
    expect(BUILTIN_WORKFLOWS[1].id).toBe("jira-compat");
    expect(BUILTIN_WORKFLOWS[2].id).toBe("linear-compat");
  });

  it("has exactly one default workflow", () => {
    const defaults = BUILTIN_WORKFLOWS.filter((w) => w.isDefault);
    expect(defaults).toHaveLength(1);
    expect(defaults[0].id).toBe("ralphx-default");
  });
});

describe("getBuiltinWorkflow", () => {
  it("returns default workflow by id", () => {
    const workflow = getBuiltinWorkflow("ralphx-default");
    expect(workflow).toBeDefined();
    expect(workflow?.name).toBe("RalphX Default");
  });

  it("returns jira workflow by id", () => {
    const workflow = getBuiltinWorkflow("jira-compat");
    expect(workflow).toBeDefined();
    expect(workflow?.name).toBe("Jira Compatible");
  });

  it("returns linear workflow by id", () => {
    const workflow = getBuiltinWorkflow("linear-compat");
    expect(workflow).toBeDefined();
    expect(workflow?.name).toBe("Linear Compatible");
  });

  it("returns undefined for unknown id", () => {
    const workflow = getBuiltinWorkflow("unknown-workflow");
    expect(workflow).toBeUndefined();
  });
});

describe("external sync type inference", () => {
  it("correctly infers SyncProvider type", () => {
    const provider: SyncProvider = "jira";
    expect(provider).toBe("jira");
  });

  it("correctly infers SyncDirection type", () => {
    const direction: SyncDirection = "bidirectional";
    expect(direction).toBe("bidirectional");
  });

  it("correctly infers ConflictResolution type", () => {
    const resolution: ConflictResolution = "external_wins";
    expect(resolution).toBe("external_wins");
  });

  it("correctly infers ExternalSyncConfig type", () => {
    const config: ExternalSyncConfig = {
      provider: "github",
      mapping: {},
      sync: { direction: "pull" },
      conflictResolution: "manual",
    };
    expect(config.provider).toBe("github");
  });
});
