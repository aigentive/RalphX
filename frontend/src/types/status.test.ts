import { describe, it, expect } from "vitest";
import {
  InternalStatusSchema,
  INTERNAL_STATUS_VALUES,
  IDLE_STATUSES,
  ACTIVE_STATUSES,
  TERMINAL_STATUSES,
  isTerminalStatus,
  isActiveStatus,
  isIdleStatus,
  categorizeStatus,
  getStatusCounts,
} from "./status";
import type { StatusCounts } from "./status";
import type { InternalStatus } from "./status";

describe("InternalStatusSchema", () => {
  it("should have exactly 28 status values", () => {
    expect(INTERNAL_STATUS_VALUES.length).toBe(28);
  });

  it("should parse all valid status values", () => {
    const validStatuses = [
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
      "waiting_on_pr",
      "updating_plan_branch",
      "updating_task_branch",
      "branch_update_blocked",
      "failed",
      "cancelled",
    ];

    for (const status of validStatuses) {
      expect(() => InternalStatusSchema.parse(status)).not.toThrow();
    }
  });

  it("should reject invalid status values", () => {
    const invalidStatuses = [
      "invalid",
      "Backlog", // Wrong case
      "READY",
      "pending", // partial match
      "",
      null,
      undefined,
      123,
    ];

    for (const status of invalidStatuses) {
      expect(() => InternalStatusSchema.parse(status)).toThrow();
    }
  });

  it("should provide helpful error for invalid values", () => {
    const result = InternalStatusSchema.safeParse("invalid");
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0]?.message).toBeDefined();
    }
  });
});

describe("Status Categories", () => {
  it("should classify branch-update attention as idle/waiting", () => {
    expect(IDLE_STATUSES.length).toBe(4);
    expect(IDLE_STATUSES).toContain("backlog");
    expect(IDLE_STATUSES).toContain("ready");
    expect(IDLE_STATUSES).toContain("blocked");
    expect(IDLE_STATUSES).toContain("branch_update_blocked");
  });

  it("should have 14 active statuses", () => {
    expect(ACTIVE_STATUSES.length).toBe(14);
    expect(ACTIVE_STATUSES).toContain("executing");
    expect(ACTIVE_STATUSES).toContain("re_executing");
    expect(ACTIVE_STATUSES).toContain("qa_refining");
    expect(ACTIVE_STATUSES).toContain("qa_testing");
    expect(ACTIVE_STATUSES).toContain("pending_review");
    expect(ACTIVE_STATUSES).toContain("reviewing");
    expect(ACTIVE_STATUSES).toContain("review_passed");
    expect(ACTIVE_STATUSES).toContain("escalated");
    expect(ACTIVE_STATUSES).toContain("revision_needed");
    expect(ACTIVE_STATUSES).toContain("pending_merge");
    expect(ACTIVE_STATUSES).toContain("merging");
    expect(ACTIVE_STATUSES).toContain("waiting_on_pr");
    expect(ACTIVE_STATUSES).toContain("updating_plan_branch");
    expect(ACTIVE_STATUSES).toContain("updating_task_branch");
  });

  it("should classify merge-incomplete as terminal-like", () => {
    expect(TERMINAL_STATUSES.length).toBe(6);
    expect(TERMINAL_STATUSES).toContain("approved");
    expect(TERMINAL_STATUSES).toContain("merged");
    expect(TERMINAL_STATUSES).toContain("failed");
    expect(TERMINAL_STATUSES).toContain("cancelled");
    expect(TERMINAL_STATUSES).toContain("stopped");
    expect(TERMINAL_STATUSES).toContain("merge_incomplete");
  });

  it("should have no overlap between categories", () => {
    const idleSet = new Set(IDLE_STATUSES);
    const activeSet = new Set(ACTIVE_STATUSES);
    const terminalSet = new Set(TERMINAL_STATUSES);

    for (const status of IDLE_STATUSES) {
      expect(activeSet.has(status)).toBe(false);
      expect(terminalSet.has(status)).toBe(false);
    }

    for (const status of ACTIVE_STATUSES) {
      expect(idleSet.has(status)).toBe(false);
      expect(terminalSet.has(status)).toBe(false);
    }

    for (const status of TERMINAL_STATUSES) {
      expect(idleSet.has(status)).toBe(false);
      expect(activeSet.has(status)).toBe(false);
    }
  });

  it("should categorize all statuses except paused and merge_conflict exactly once", () => {
    const allCategorized = [
      ...IDLE_STATUSES,
      ...ACTIVE_STATUSES,
      ...TERMINAL_STATUSES,
      "qa_passed",
      "qa_failed",
    ];
    expect(allCategorized.length).toBe(26);
    expect(new Set(allCategorized).size).toBe(26);
  });
});

describe("Status Helper Functions", () => {
  describe("isTerminalStatus", () => {
    it("should return true for terminal statuses", () => {
      expect(isTerminalStatus("approved")).toBe(true);
      expect(isTerminalStatus("merged")).toBe(true);
      expect(isTerminalStatus("failed")).toBe(true);
      expect(isTerminalStatus("cancelled")).toBe(true);
      expect(isTerminalStatus("stopped")).toBe(true);
    });

    it("should return false for non-terminal statuses", () => {
      expect(isTerminalStatus("backlog")).toBe(false);
      expect(isTerminalStatus("executing")).toBe(false);
      expect(isTerminalStatus("qa_testing")).toBe(false);
      expect(isTerminalStatus("paused")).toBe(false);
    });
  });

  describe("isActiveStatus", () => {
    it("should return true for active statuses", () => {
      expect(isActiveStatus("executing")).toBe(true);
      expect(isActiveStatus("qa_refining")).toBe(true);
      expect(isActiveStatus("qa_testing")).toBe(true);
      expect(isActiveStatus("pending_review")).toBe(true);
      expect(isActiveStatus("revision_needed")).toBe(true);
    });

    it("should return false for non-active statuses", () => {
      expect(isActiveStatus("backlog")).toBe(false);
      expect(isActiveStatus("approved")).toBe(false);
      expect(isActiveStatus("qa_passed")).toBe(false);
    });
  });

  describe("isIdleStatus", () => {
    it("should return true for idle statuses", () => {
      expect(isIdleStatus("backlog")).toBe(true);
      expect(isIdleStatus("ready")).toBe(true);
      expect(isIdleStatus("blocked")).toBe(true);
    });

    it("should return false for non-idle statuses", () => {
      expect(isIdleStatus("executing")).toBe(false);
      expect(isIdleStatus("approved")).toBe(false);
      expect(isIdleStatus("qa_testing")).toBe(false);
    });
  });
});

describe("categorizeStatus", () => {
  it("should return 'idle' for idle statuses", () => {
    expect(categorizeStatus("backlog")).toBe("idle");
    expect(categorizeStatus("ready")).toBe("idle");
    expect(categorizeStatus("blocked")).toBe("idle");
    expect(categorizeStatus("branch_update_blocked")).toBe("idle");
  });

  it("should return 'done' for terminal statuses", () => {
    expect(categorizeStatus("approved")).toBe("done");
    expect(categorizeStatus("merged")).toBe("done");
    expect(categorizeStatus("failed")).toBe("done");
    expect(categorizeStatus("cancelled")).toBe("done");
    expect(categorizeStatus("stopped")).toBe("done");
    expect(categorizeStatus("merge_incomplete")).toBe("done");
  });

  it("should return 'active' for all other statuses", () => {
    expect(categorizeStatus("executing")).toBe("active");
    expect(categorizeStatus("re_executing")).toBe("active");
    expect(categorizeStatus("qa_testing")).toBe("active");
    expect(categorizeStatus("pending_review")).toBe("active");
    expect(categorizeStatus("reviewing")).toBe("active");
    expect(categorizeStatus("paused")).toBe("active");
    expect(categorizeStatus("qa_passed")).toBe("active");
    expect(categorizeStatus("merge_conflict")).toBe("active");
    expect(categorizeStatus("updating_plan_branch")).toBe("active");
  });
});

describe("getStatusCounts", () => {
  function makeTasks(statuses: InternalStatus[]) {
    return statuses.map((s) => ({ internalStatus: s }) as { internalStatus: InternalStatus });
  }

  it("should return zero counts for empty array", () => {
    const counts: StatusCounts = getStatusCounts([]);
    expect(counts).toEqual({ idle: 0, active: 0, done: 0, total: 0 });
  });

  it("should count idle tasks", () => {
    const counts = getStatusCounts(makeTasks(["backlog", "ready", "blocked"]));
    expect(counts).toEqual({ idle: 3, active: 0, done: 0, total: 3 });
  });

  it("should count active tasks", () => {
    const counts = getStatusCounts(makeTasks(["executing", "qa_testing"]));
    expect(counts).toEqual({ idle: 0, active: 2, done: 0, total: 2 });
  });

  it("should count done tasks", () => {
    const counts = getStatusCounts(makeTasks(["merged", "approved"]));
    expect(counts).toEqual({ idle: 0, active: 0, done: 2, total: 2 });
  });

  it("should count mixed statuses correctly", () => {
    const counts = getStatusCounts(
      makeTasks(["backlog", "executing", "merged", "ready", "failed"])
    );
    expect(counts).toEqual({ idle: 2, active: 1, done: 2, total: 5 });
  });
});
