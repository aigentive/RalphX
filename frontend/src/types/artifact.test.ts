import { describe, it, expect } from "vitest";
import {
  ArtifactTypeSchema,
  ARTIFACT_TYPE_VALUES,
  DOCUMENT_ARTIFACT_TYPES,
  CODE_ARTIFACT_TYPES,
  PROCESS_ARTIFACT_TYPES,
  CONTEXT_ARTIFACT_TYPES,
  LOG_ARTIFACT_TYPES,
  isDocumentArtifact,
  isCodeArtifact,
  isProcessArtifact,
  isContextArtifact,
  isLogArtifact,
  ArtifactContentInlineSchema,
  ArtifactContentFileSchema,
  ArtifactContentSchema,
  ArtifactMetadataSchema,
  PlanApprovalSchema,
  ArtifactSchema,
  ArtifactBucketSchema,
  ArtifactRelationTypeSchema,
  ARTIFACT_RELATION_TYPE_VALUES,
  ArtifactRelationSchema,
  ArtifactFlowEventSchema,
  ARTIFACT_FLOW_EVENT_VALUES,
  ArtifactFlowFilterSchema,
  ArtifactFlowTriggerSchema,
  ArtifactFlowStepCopySchema,
  ArtifactFlowStepSpawnProcessSchema,
  ArtifactFlowStepSchema,
  ArtifactFlowSchema,
  SYSTEM_BUCKETS,
  getSystemBucket,
  type ArtifactType,
  type ArtifactContent,
  type ArtifactMetadata,
  type PlanApproval,
  type Artifact,
  type ArtifactBucket,
  type ArtifactRelationType,
  type ArtifactRelation,
  type ArtifactFlowEvent,
  type ArtifactFlowFilter,
  type ArtifactFlowTrigger,
  type ArtifactFlowStep,
  type ArtifactFlow,
} from "./artifact";

// ============================================
// ArtifactType Tests
// ============================================

describe("ArtifactTypeSchema", () => {
  it("validates all 20 artifact types", () => {
    const types = [
      // Documents
      "prd",
      "persona",
      "research_document",
      "design_doc",
      "specification",
      // Code
      "code_change",
      "diff",
      "test_result",
      // Process
      "task_spec",
      "review_feedback",
      "pr_review",
      "approval",
      "findings",
      "recommendations",
      // Context
      "context",
      "previous_work",
      "research_brief",
      // Logs
      "activity_log",
      "alert",
      "intervention",
    ];

    for (const type of types) {
      const result = ArtifactTypeSchema.safeParse(type);
      expect(result.success).toBe(true);
    }
  });

  it("rejects invalid artifact type", () => {
    const result = ArtifactTypeSchema.safeParse("invalid_type");
    expect(result.success).toBe(false);
  });

  it("exports all artifact type values", () => {
    expect(ARTIFACT_TYPE_VALUES).toHaveLength(20);
    expect(ARTIFACT_TYPE_VALUES).toContain("prd");
    expect(ARTIFACT_TYPE_VALUES).toContain("persona");
    expect(ARTIFACT_TYPE_VALUES).toContain("pr_review");
    expect(ARTIFACT_TYPE_VALUES).toContain("code_change");
    expect(ARTIFACT_TYPE_VALUES).toContain("activity_log");
  });
});

describe("Artifact type category helpers", () => {
  it("DOCUMENT_ARTIFACT_TYPES has 5 types", () => {
    expect(DOCUMENT_ARTIFACT_TYPES).toEqual([
      "prd",
      "persona",
      "research_document",
      "design_doc",
      "specification",
    ]);
  });

  it("CODE_ARTIFACT_TYPES has 3 types", () => {
    expect(CODE_ARTIFACT_TYPES).toEqual(["code_change", "diff", "test_result"]);
  });

  it("PROCESS_ARTIFACT_TYPES has 6 types", () => {
    expect(PROCESS_ARTIFACT_TYPES).toEqual([
      "task_spec",
      "review_feedback",
      "pr_review",
      "approval",
      "findings",
      "recommendations",
    ]);
  });

  it("CONTEXT_ARTIFACT_TYPES has 3 types", () => {
    expect(CONTEXT_ARTIFACT_TYPES).toEqual([
      "context",
      "previous_work",
      "research_brief",
    ]);
  });

  it("LOG_ARTIFACT_TYPES has 3 types", () => {
    expect(LOG_ARTIFACT_TYPES).toEqual(["activity_log", "alert", "intervention"]);
  });

  it("isDocumentArtifact returns true for document types", () => {
    expect(isDocumentArtifact("prd")).toBe(true);
    expect(isDocumentArtifact("design_doc")).toBe(true);
    expect(isDocumentArtifact("code_change")).toBe(false);
  });

  it("isCodeArtifact returns true for code types", () => {
    expect(isCodeArtifact("code_change")).toBe(true);
    expect(isCodeArtifact("diff")).toBe(true);
    expect(isCodeArtifact("prd")).toBe(false);
  });

  it("isProcessArtifact returns true for process types", () => {
    expect(isProcessArtifact("task_spec")).toBe(true);
    expect(isProcessArtifact("findings")).toBe(true);
    expect(isProcessArtifact("prd")).toBe(false);
  });

  it("isContextArtifact returns true for context types", () => {
    expect(isContextArtifact("context")).toBe(true);
    expect(isContextArtifact("previous_work")).toBe(true);
    expect(isContextArtifact("prd")).toBe(false);
  });

  it("isLogArtifact returns true for log types", () => {
    expect(isLogArtifact("activity_log")).toBe(true);
    expect(isLogArtifact("alert")).toBe(true);
    expect(isLogArtifact("prd")).toBe(false);
  });
});

// ============================================
// ArtifactContent Tests
// ============================================

describe("ArtifactContentInlineSchema", () => {
  it("validates inline content", () => {
    const content = { type: "inline", text: "Hello world" };
    const result = ArtifactContentInlineSchema.safeParse(content);
    expect(result.success).toBe(true);
  });

  it("rejects inline content without text", () => {
    const content = { type: "inline" };
    const result = ArtifactContentInlineSchema.safeParse(content);
    expect(result.success).toBe(false);
  });
});

describe("ArtifactContentFileSchema", () => {
  it("validates file content", () => {
    const content = { type: "file", path: "/path/to/file.md" };
    const result = ArtifactContentFileSchema.safeParse(content);
    expect(result.success).toBe(true);
  });

  it("rejects file content without path", () => {
    const content = { type: "file" };
    const result = ArtifactContentFileSchema.safeParse(content);
    expect(result.success).toBe(false);
  });
});

describe("ArtifactContentSchema", () => {
  it("validates inline content", () => {
    const content = { type: "inline", text: "Test content" };
    const result = ArtifactContentSchema.safeParse(content);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.type).toBe("inline");
    }
  });

  it("validates file content", () => {
    const content = { type: "file", path: "/test/path" };
    const result = ArtifactContentSchema.safeParse(content);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.type).toBe("file");
    }
  });

  it("rejects unknown content type", () => {
    const content = { type: "unknown", data: "test" };
    const result = ArtifactContentSchema.safeParse(content);
    expect(result.success).toBe(false);
  });
});

// ============================================
// ArtifactMetadata Tests
// ============================================

describe("ArtifactMetadataSchema", () => {
  it("validates metadata with required fields only", () => {
    const metadata = {
      createdAt: "2024-01-01T00:00:00.000Z",
      createdBy: "user",
      version: 1,
    };
    const result = ArtifactMetadataSchema.safeParse(metadata);
    expect(result.success).toBe(true);
  });

  it("validates metadata with all fields", () => {
    const metadata = {
      createdAt: "2024-01-01T00:00:00.000Z",
      createdBy: "worker",
      taskId: "task-123",
      processId: "process-456",
      version: 2,
    };
    const result = ArtifactMetadataSchema.safeParse(metadata);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.taskId).toBe("task-123");
      expect(result.data.processId).toBe("process-456");
    }
  });

  it("defaults version to 1", () => {
    const metadata = {
      createdAt: "2024-01-01T00:00:00.000Z",
      createdBy: "user",
    };
    const result = ArtifactMetadataSchema.safeParse(metadata);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.version).toBe(1);
    }
  });

  it("rejects metadata without createdAt", () => {
    const metadata = { createdBy: "user", version: 1 };
    const result = ArtifactMetadataSchema.safeParse(metadata);
    expect(result.success).toBe(false);
  });

  it("rejects metadata without createdBy", () => {
    const metadata = { createdAt: "2024-01-01T00:00:00.000Z", version: 1 };
    const result = ArtifactMetadataSchema.safeParse(metadata);
    expect(result.success).toBe(false);
  });
});

describe("PlanApprovalSchema", () => {
  it("validates draft approval state", () => {
    const approval: PlanApproval = {
      status: "draft",
    };

    const result = PlanApprovalSchema.safeParse(approval);

    expect(result.success).toBe(true);
  });

  it("validates approved approval state", () => {
    const approval: PlanApproval = {
      status: "approved",
      approvedArtifactId: "artifact-123",
      approvedVersion: 2,
      approvedAt: "2026-05-23T05:00:00.000Z",
    };

    const result = PlanApprovalSchema.safeParse(approval);

    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.approvedVersion).toBe(2);
    }
  });
});

// ============================================
// Artifact Tests
// ============================================

describe("ArtifactSchema", () => {
  it("validates a minimal artifact", () => {
    const artifact = {
      id: "artifact-123",
      type: "prd",
      name: "Test PRD",
      content: { type: "inline", text: "PRD content" },
      metadata: {
        createdAt: "2024-01-01T00:00:00.000Z",
        createdBy: "user",
        version: 1,
      },
    };
    const result = ArtifactSchema.safeParse(artifact);
    expect(result.success).toBe(true);
  });

  it("validates an artifact with all fields", () => {
    const artifact = {
      id: "artifact-123",
      type: "code_change",
      name: "Feature Implementation",
      content: { type: "file", path: "/src/feature.ts" },
      metadata: {
        createdAt: "2024-01-01T00:00:00.000Z",
        createdBy: "worker",
        taskId: "task-456",
        version: 1,
      },
      derivedFrom: ["artifact-100", "artifact-101"],
      bucketId: "code-changes",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-123",
        approvedVersion: 1,
        approvedAt: "2026-05-23T05:00:00.000Z",
      },
    };
    const result = ArtifactSchema.safeParse(artifact);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.derivedFrom).toHaveLength(2);
      expect(result.data.bucketId).toBe("code-changes");
      expect(result.data.planApproval?.status).toBe("approved");
    }
  });

  it("defaults derivedFrom to empty array", () => {
    const artifact = {
      id: "artifact-123",
      type: "prd",
      name: "Test",
      content: { type: "inline", text: "Content" },
      metadata: {
        createdAt: "2024-01-01T00:00:00.000Z",
        createdBy: "user",
        version: 1,
      },
    };
    const result = ArtifactSchema.safeParse(artifact);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.derivedFrom).toEqual([]);
    }
  });

  it("rejects artifact with invalid type", () => {
    const artifact = {
      id: "artifact-123",
      type: "invalid_type",
      name: "Test",
      content: { type: "inline", text: "Content" },
      metadata: {
        createdAt: "2024-01-01T00:00:00.000Z",
        createdBy: "user",
        version: 1,
      },
    };
    const result = ArtifactSchema.safeParse(artifact);
    expect(result.success).toBe(false);
  });

  it("rejects artifact without id", () => {
    const artifact = {
      type: "prd",
      name: "Test",
      content: { type: "inline", text: "Content" },
      metadata: {
        createdAt: "2024-01-01T00:00:00.000Z",
        createdBy: "user",
        version: 1,
      },
    };
    const result = ArtifactSchema.safeParse(artifact);
    expect(result.success).toBe(false);
  });
});

// ============================================
// ArtifactBucket Tests
// ============================================

describe("ArtifactBucketSchema", () => {
  it("validates a minimal bucket", () => {
    const bucket = {
      id: "bucket-123",
      name: "Custom Bucket",
      acceptedTypes: [],
      writers: [],
      readers: ["all"],
      isSystem: false,
    };
    const result = ArtifactBucketSchema.safeParse(bucket);
    expect(result.success).toBe(true);
  });

  it("validates a bucket with accepted types", () => {
    const bucket = {
      id: "research-outputs",
      name: "Research Outputs",
      acceptedTypes: ["research_document", "findings", "recommendations"],
      writers: ["deep-researcher", "orchestrator"],
      readers: ["all"],
      isSystem: true,
    };
    const result = ArtifactBucketSchema.safeParse(bucket);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.acceptedTypes).toHaveLength(3);
      expect(result.data.isSystem).toBe(true);
    }
  });

  it("defaults isSystem to false", () => {
    const bucket = {
      id: "custom",
      name: "Custom",
      acceptedTypes: [],
      writers: [],
      readers: [],
    };
    const result = ArtifactBucketSchema.safeParse(bucket);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.isSystem).toBe(false);
    }
  });

  it("rejects bucket with invalid accepted type", () => {
    const bucket = {
      id: "bucket",
      name: "Bucket",
      acceptedTypes: ["invalid_type"],
      writers: [],
      readers: [],
    };
    const result = ArtifactBucketSchema.safeParse(bucket);
    expect(result.success).toBe(false);
  });
});

// ============================================
// ArtifactRelation Tests
// ============================================

describe("ArtifactRelationTypeSchema", () => {
  it("validates derived_from", () => {
    const result = ArtifactRelationTypeSchema.safeParse("derived_from");
    expect(result.success).toBe(true);
  });

  it("validates related_to", () => {
    const result = ArtifactRelationTypeSchema.safeParse("related_to");
    expect(result.success).toBe(true);
  });

  it("rejects invalid relation type", () => {
    const result = ArtifactRelationTypeSchema.safeParse("linked_to");
    expect(result.success).toBe(false);
  });

  it("exports all relation type values", () => {
    expect(ARTIFACT_RELATION_TYPE_VALUES).toEqual(["derived_from", "related_to"]);
  });
});

describe("ArtifactRelationSchema", () => {
  it("validates a complete relation", () => {
    const relation = {
      id: "rel-123",
      fromArtifactId: "artifact-1",
      toArtifactId: "artifact-2",
      relationType: "derived_from",
    };
    const result = ArtifactRelationSchema.safeParse(relation);
    expect(result.success).toBe(true);
  });

  it("rejects relation without required fields", () => {
    const relation = {
      id: "rel-123",
      fromArtifactId: "artifact-1",
    };
    const result = ArtifactRelationSchema.safeParse(relation);
    expect(result.success).toBe(false);
  });
});

// ============================================
// ArtifactFlow Tests
// ============================================

describe("ArtifactFlowEventSchema", () => {
  it("validates all flow events", () => {
    const events = ["artifact_created", "task_completed", "process_completed"];
    for (const event of events) {
      const result = ArtifactFlowEventSchema.safeParse(event);
      expect(result.success).toBe(true);
    }
  });

  it("rejects invalid event", () => {
    const result = ArtifactFlowEventSchema.safeParse("invalid_event");
    expect(result.success).toBe(false);
  });

  it("exports all event values", () => {
    expect(ARTIFACT_FLOW_EVENT_VALUES).toEqual([
      "artifact_created",
      "task_completed",
      "process_completed",
    ]);
  });
});

describe("ArtifactFlowFilterSchema", () => {
  it("validates an empty filter", () => {
    const filter = {};
    const result = ArtifactFlowFilterSchema.safeParse(filter);
    expect(result.success).toBe(true);
  });

  it("validates filter with artifact types", () => {
    const filter = { artifactTypes: ["prd", "design_doc"] };
    const result = ArtifactFlowFilterSchema.safeParse(filter);
    expect(result.success).toBe(true);
  });

  it("validates filter with source bucket", () => {
    const filter = { sourceBucket: "research-outputs" };
    const result = ArtifactFlowFilterSchema.safeParse(filter);
    expect(result.success).toBe(true);
  });

  it("validates filter with both fields", () => {
    const filter = {
      artifactTypes: ["recommendations"],
      sourceBucket: "research-outputs",
    };
    const result = ArtifactFlowFilterSchema.safeParse(filter);
    expect(result.success).toBe(true);
  });

  it("rejects filter with invalid artifact type", () => {
    const filter = { artifactTypes: ["invalid_type"] };
    const result = ArtifactFlowFilterSchema.safeParse(filter);
    expect(result.success).toBe(false);
  });
});

describe("ArtifactFlowTriggerSchema", () => {
  it("validates trigger with event only", () => {
    const trigger = { event: "artifact_created" };
    const result = ArtifactFlowTriggerSchema.safeParse(trigger);
    expect(result.success).toBe(true);
  });

  it("validates trigger with filter", () => {
    const trigger = {
      event: "artifact_created",
      filter: { artifactTypes: ["prd"] },
    };
    const result = ArtifactFlowTriggerSchema.safeParse(trigger);
    expect(result.success).toBe(true);
  });

  it("rejects trigger without event", () => {
    const trigger = { filter: { artifactTypes: ["prd"] } };
    const result = ArtifactFlowTriggerSchema.safeParse(trigger);
    expect(result.success).toBe(false);
  });
});

describe("ArtifactFlowStep schemas", () => {
  it("validates copy step", () => {
    const step = { type: "copy", toBucket: "prd-library" };
    const result = ArtifactFlowStepCopySchema.safeParse(step);
    expect(result.success).toBe(true);
  });

  it("validates spawn_process step", () => {
    const step = {
      type: "spawn_process",
      processType: "task_decomposition",
      agentProfile: "orchestrator",
    };
    const result = ArtifactFlowStepSpawnProcessSchema.safeParse(step);
    expect(result.success).toBe(true);
  });

  it("validates step using union schema (copy)", () => {
    const step = { type: "copy", toBucket: "target" };
    const result = ArtifactFlowStepSchema.safeParse(step);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.type).toBe("copy");
    }
  });

  it("validates step using union schema (spawn_process)", () => {
    const step = {
      type: "spawn_process",
      processType: "research",
      agentProfile: "deep-researcher",
    };
    const result = ArtifactFlowStepSchema.safeParse(step);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.type).toBe("spawn_process");
    }
  });

  it("rejects step with unknown type", () => {
    const step = { type: "unknown", data: "test" };
    const result = ArtifactFlowStepSchema.safeParse(step);
    expect(result.success).toBe(false);
  });
});

describe("ArtifactFlowSchema", () => {
  it("validates a minimal flow", () => {
    const flow = {
      id: "flow-123",
      name: "Test Flow",
      trigger: { event: "artifact_created" },
      steps: [],
      isActive: true,
      createdAt: "2024-01-01T00:00:00.000Z",
    };
    const result = ArtifactFlowSchema.safeParse(flow);
    expect(result.success).toBe(true);
  });

  it("validates a complete flow", () => {
    const flow = {
      id: "research-to-dev",
      name: "Research to Development",
      trigger: {
        event: "artifact_created",
        filter: {
          artifactTypes: ["recommendations"],
          sourceBucket: "research-outputs",
        },
      },
      steps: [
        { type: "copy", toBucket: "prd-library" },
        {
          type: "spawn_process",
          processType: "task_decomposition",
          agentProfile: "orchestrator",
        },
      ],
      isActive: true,
      createdAt: "2024-01-01T00:00:00.000Z",
    };
    const result = ArtifactFlowSchema.safeParse(flow);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.steps).toHaveLength(2);
    }
  });

  it("defaults isActive to true", () => {
    const flow = {
      id: "flow-123",
      name: "Test",
      trigger: { event: "artifact_created" },
      steps: [],
      createdAt: "2024-01-01T00:00:00.000Z",
    };
    const result = ArtifactFlowSchema.safeParse(flow);
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.isActive).toBe(true);
    }
  });

  it("rejects flow without required fields", () => {
    const flow = {
      id: "flow-123",
      trigger: { event: "artifact_created" },
    };
    const result = ArtifactFlowSchema.safeParse(flow);
    expect(result.success).toBe(false);
  });
});

// ============================================
// System Buckets Tests
// ============================================

describe("SYSTEM_BUCKETS", () => {
  it("contains 4 system buckets", () => {
    expect(SYSTEM_BUCKETS).toHaveLength(4);
  });

  it("has research-outputs bucket", () => {
    const bucket = SYSTEM_BUCKETS.find((b) => b.id === "research-outputs");
    expect(bucket).toBeDefined();
    expect(bucket?.acceptedTypes).toEqual([
      "research_document",
      "findings",
      "recommendations",
    ]);
    expect(bucket?.writers).toContain("deep-researcher");
  });

  it("has work-context bucket", () => {
    const bucket = SYSTEM_BUCKETS.find((b) => b.id === "work-context");
    expect(bucket).toBeDefined();
    expect(bucket?.acceptedTypes).toEqual([
      "context",
      "task_spec",
      "previous_work",
    ]);
  });

  it("has code-changes bucket", () => {
    const bucket = SYSTEM_BUCKETS.find((b) => b.id === "code-changes");
    expect(bucket).toBeDefined();
    expect(bucket?.acceptedTypes).toEqual(["code_change", "diff", "test_result"]);
  });

  it("has prd-library bucket", () => {
    const bucket = SYSTEM_BUCKETS.find((b) => b.id === "prd-library");
    expect(bucket).toBeDefined();
    expect(bucket?.acceptedTypes).toEqual(["prd", "specification", "design_doc"]);
  });

  it("all system buckets have isSystem true", () => {
    for (const bucket of SYSTEM_BUCKETS) {
      expect(bucket.isSystem).toBe(true);
    }
  });
});

describe("getSystemBucket", () => {
  it("returns bucket by id", () => {
    const bucket = getSystemBucket("research-outputs");
    expect(bucket).toBeDefined();
    expect(bucket?.name).toBe("Research Outputs");
  });

  it("returns undefined for unknown id", () => {
    const bucket = getSystemBucket("unknown-bucket");
    expect(bucket).toBeUndefined();
  });
});

// ============================================
// Type Inference Tests
// ============================================

describe("type inference", () => {
  it("correctly infers ArtifactType", () => {
    const type: ArtifactType = "prd";
    expect(type).toBe("prd");
  });

  it("correctly infers ArtifactContent", () => {
    const content: ArtifactContent = { type: "inline", text: "Test" };
    expect(content.type).toBe("inline");
  });

  it("correctly infers ArtifactMetadata", () => {
    const metadata: ArtifactMetadata = {
      createdAt: "2024-01-01T00:00:00.000Z",
      createdBy: "user",
      version: 1,
    };
    expect(metadata.createdBy).toBe("user");
  });

  it("correctly infers Artifact", () => {
    const artifact: Artifact = {
      id: "test",
      type: "prd",
      name: "Test",
      content: { type: "inline", text: "Content" },
      metadata: {
        createdAt: "2024-01-01T00:00:00.000Z",
        createdBy: "user",
        version: 1,
      },
      derivedFrom: [],
    };
    expect(artifact.type).toBe("prd");
  });

  it("correctly infers ArtifactBucket", () => {
    const bucket: ArtifactBucket = {
      id: "test",
      name: "Test",
      acceptedTypes: ["prd"],
      writers: ["user"],
      readers: ["all"],
      isSystem: false,
    };
    expect(bucket.id).toBe("test");
  });

  it("correctly infers ArtifactRelationType", () => {
    const relationType: ArtifactRelationType = "derived_from";
    expect(relationType).toBe("derived_from");
  });

  it("correctly infers ArtifactRelation", () => {
    const relation: ArtifactRelation = {
      id: "rel-1",
      fromArtifactId: "a1",
      toArtifactId: "a2",
      relationType: "derived_from",
    };
    expect(relation.relationType).toBe("derived_from");
  });

  it("correctly infers ArtifactFlowEvent", () => {
    const event: ArtifactFlowEvent = "artifact_created";
    expect(event).toBe("artifact_created");
  });

  it("correctly infers ArtifactFlowFilter", () => {
    const filter: ArtifactFlowFilter = {
      artifactTypes: ["prd"],
      sourceBucket: "research-outputs",
    };
    expect(filter.artifactTypes).toContain("prd");
  });

  it("correctly infers ArtifactFlowTrigger", () => {
    const trigger: ArtifactFlowTrigger = {
      event: "artifact_created",
      filter: { artifactTypes: ["prd"] },
    };
    expect(trigger.event).toBe("artifact_created");
  });

  it("correctly infers ArtifactFlowStep", () => {
    const step: ArtifactFlowStep = { type: "copy", toBucket: "target" };
    expect(step.type).toBe("copy");
  });

  it("correctly infers ArtifactFlow", () => {
    const flow: ArtifactFlow = {
      id: "flow-1",
      name: "Test Flow",
      trigger: { event: "artifact_created" },
      steps: [],
      isActive: true,
      createdAt: "2024-01-01T00:00:00.000Z",
    };
    expect(flow.name).toBe("Test Flow");
  });
});
