/**
 * Tests for chat-context-registry
 *
 * Verifies:
 * - All 24 internal statuses map to correct context types
 * - Store keys are formatted correctly
 * - Registry has entries for every context type
 * - Feature flags are consistent
 */

import { describe, it, expect } from "vitest";
import {
  CHAT_CONTEXT_REGISTRY,
  buildStoreKey,
  parseStoreKey,
  resolveContextType,
  getContextConfig,
  isAgentContext,
} from "./chat-context-registry";
import { CONTEXT_TYPE_VALUES } from "@/types/chat-conversation";
import { INTERNAL_STATUS_VALUES } from "@/types/status";

// ============================================================================
// Registry Completeness
// ============================================================================

describe("CHAT_CONTEXT_REGISTRY", () => {
  it("has an entry for every ContextType", () => {
    for (const ct of CONTEXT_TYPE_VALUES) {
      expect(CHAT_CONTEXT_REGISTRY[ct]).toBeDefined();
      expect(CHAT_CONTEXT_REGISTRY[ct].storeKeyPrefix).toBeTruthy();
      expect(CHAT_CONTEXT_REGISTRY[ct].placeholder).toBeTruthy();
      expect(CHAT_CONTEXT_REGISTRY[ct].label).toBeTruthy();
    }
  });

  it("has unique storeKeyPrefix for each context type", () => {
    const prefixes = Object.values(CHAT_CONTEXT_REGISTRY).map((c) => c.storeKeyPrefix);
    expect(new Set(prefixes).size).toBe(prefixes.length);
  });
});

// ============================================================================
// buildStoreKey
// ============================================================================

describe("buildStoreKey", () => {
  it("formats ideation keys as session:{id}", () => {
    expect(buildStoreKey("ideation", "sess-123")).toBe("session:sess-123");
  });

  it("formats task keys as task:{id}", () => {
    expect(buildStoreKey("task", "task-456")).toBe("task:task-456");
  });

  it("formats project keys as project:{id}", () => {
    expect(buildStoreKey("project", "proj-789")).toBe("project:proj-789");
  });

  it("formats task_execution keys as task_execution:{id}", () => {
    expect(buildStoreKey("task_execution", "task-456")).toBe("task_execution:task-456");
  });

  it("formats review keys as review:{id}", () => {
    expect(buildStoreKey("review", "task-456")).toBe("review:task-456");
  });

  it("formats merge keys as merge:{id}", () => {
    expect(buildStoreKey("merge", "task-456")).toBe("merge:task-456");
  });

  it("formats delegation keys as delegation:{id}", () => {
    expect(buildStoreKey("delegation", "child-123")).toBe("delegation:child-123");
  });
});

// ============================================================================
// parseStoreKey
// ============================================================================

describe("parseStoreKey", () => {
  it("parses ideation key back to contextType and contextId", () => {
    expect(parseStoreKey("session:sess-123")).toEqual({ contextType: "ideation", contextId: "sess-123" });
  });

  it("parses task key", () => {
    expect(parseStoreKey("task:task-456")).toEqual({ contextType: "task", contextId: "task-456" });
  });

  it("parses project key", () => {
    expect(parseStoreKey("project:proj-789")).toEqual({ contextType: "project", contextId: "proj-789" });
  });

  it("parses task_execution key", () => {
    expect(parseStoreKey("task_execution:task-456")).toEqual({ contextType: "task_execution", contextId: "task-456" });
  });

  it("parses review key", () => {
    expect(parseStoreKey("review:task-456")).toEqual({ contextType: "review", contextId: "task-456" });
  });

  it("parses merge key", () => {
    expect(parseStoreKey("merge:task-456")).toEqual({ contextType: "merge", contextId: "task-456" });
  });

  it("parses delegation key", () => {
    expect(parseStoreKey("delegation:child-123")).toEqual({
      contextType: "delegation",
      contextId: "child-123",
    });
  });

  it("is the reverse of buildStoreKey for all context types", () => {
    const id = "test-id-123";
    for (const ct of CONTEXT_TYPE_VALUES) {
      const key = buildStoreKey(ct, id);
      expect(parseStoreKey(key)).toEqual({ contextType: ct, contextId: id });
    }
  });

  it("returns null for unknown prefix", () => {
    expect(parseStoreKey("unknown:some-id")).toBeNull();
  });

  it("returns null for key without colon", () => {
    expect(parseStoreKey("nocolon")).toBeNull();
  });

  it("returns null for empty contextId", () => {
    expect(parseStoreKey("session:")).toBeNull();
  });
});

// ============================================================================
// resolveContextType — Status Mapping
// ============================================================================

describe("resolveContextType", () => {
  const TASK_ID = "task-1";

  describe("ideation always wins", () => {
    it("returns ideation when ideationSessionId is set, regardless of task or status", () => {
      expect(resolveContextType("executing", "sess-1", TASK_ID)).toBe("ideation");
      expect(resolveContextType(undefined, "sess-1", undefined)).toBe("ideation");
      expect(resolveContextType("reviewing", "sess-1", TASK_ID)).toBe("ideation");
    });
  });

  describe("execution statuses → task_execution", () => {
    const executionStatuses = ["executing", "re_executing", "qa_refining", "qa_testing", "qa_passed", "qa_failed"];
    for (const status of executionStatuses) {
      it(`maps ${status} → task_execution`, () => {
        expect(resolveContextType(status, undefined, TASK_ID)).toBe("task_execution");
      });
    }
  });

  describe("review statuses → review", () => {
    const reviewStatuses = ["pending_review", "reviewing", "review_passed", "escalated", "approved"];
    for (const status of reviewStatuses) {
      it(`maps ${status} → review`, () => {
        expect(resolveContextType(status, undefined, TASK_ID)).toBe("review");
      });
    }
  });

  describe("merge statuses → merge", () => {
    const mergeStatuses = ["pending_merge", "merging", "merge_incomplete", "merge_conflict", "merged"];
    for (const status of mergeStatuses) {
      it(`maps ${status} → merge`, () => {
        expect(resolveContextType(status, undefined, TASK_ID)).toBe("merge");
      });
    }
  });

  describe("idle/other statuses → task", () => {
    const taskStatuses = [
      "backlog",
      "ready",
      "blocked",
      "revision_needed",
      "waiting_on_pr",
      "failed",
      "cancelled",
      "paused",
      "stopped",
    ];
    for (const status of taskStatuses) {
      it(`maps ${status} → task`, () => {
        expect(resolveContextType(status, undefined, TASK_ID)).toBe("task");
      });
    }
  });

  describe("all statuses map to a valid context type", () => {
    for (const status of INTERNAL_STATUS_VALUES) {
      it(`${status} resolves to a valid ContextType`, () => {
        const result = resolveContextType(status, undefined, TASK_ID);
        expect(CONTEXT_TYPE_VALUES).toContain(result);
      });
    }
  });

  describe("fallback behaviors", () => {
    it("returns task when taskId is set but status is undefined", () => {
      expect(resolveContextType(undefined, undefined, TASK_ID)).toBe("task");
    });

    it("returns project when neither ideation nor task", () => {
      expect(resolveContextType(undefined, undefined, undefined)).toBe("project");
    });

    it("returns project when status is set but no taskId", () => {
      expect(resolveContextType("executing", undefined, undefined)).toBe("project");
    });
  });
});

// ============================================================================
// getContextConfig
// ============================================================================

describe("getContextConfig", () => {
  it("returns the correct config for each context type", () => {
    expect(getContextConfig("ideation").storeKeyPrefix).toBe("session");
    expect(getContextConfig("task_execution").agentType).toBe("worker");
    expect(getContextConfig("review").agentType).toBe("reviewer");
    expect(getContextConfig("merge").agentType).toBe("merger");
    expect(getContextConfig("delegation").storeKeyPrefix).toBe("delegation");
  });

  it("keeps live streaming text enabled for chat contexts that can run agents", () => {
    expect(getContextConfig("project").supportsStreamingText).toBe(true);
    expect(getContextConfig("task").supportsStreamingText).toBe(true);
  });
});

// ============================================================================
// isAgentContext
// ============================================================================

describe("isAgentContext", () => {
  it("returns true for agent contexts", () => {
    expect(isAgentContext("task_execution")).toBe(true);
    expect(isAgentContext("review")).toBe(true);
    expect(isAgentContext("merge")).toBe(true);
  });

  it("returns false for non-agent contexts", () => {
    expect(isAgentContext("ideation")).toBe(false);
    expect(isAgentContext("task")).toBe(false);
    expect(isAgentContext("project")).toBe(false);
  });
});
