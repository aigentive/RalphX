import { describe, expect, it, vi } from "vitest";

import type { StateTransition } from "@/api/tasks";
import type { InternalStatus } from "@/types/task";

import {
  buildTaskHistoryEntries,
  entryToHistoryState,
  isSelectedTaskHistoryEntry,
} from "./taskHistoryEntries";

const t0 = "2026-07-07T10:00:00Z";
const t1 = "2026-07-07T10:05:00Z";
const t2 = "2026-07-07T10:10:00Z";
const t3 = "2026-07-07T10:15:00Z";

function transition(toStatus: InternalStatus, timestamp: string, extra: Partial<StateTransition> = {}): StateTransition {
  return { fromStatus: null, toStatus, trigger: "system", timestamp, ...extra };
}

describe("buildTaskHistoryEntries", () => {
  it("retains distinct retry attempts and their transcript metadata", () => {
    const entries = buildTaskHistoryEntries([
      transition("executing", t0, { conversationId: "execution-1" }),
      transition("reviewing", t1, { conversationId: "review-1" }),
      transition("revision_needed", t2),
      transition("re_executing", t3, { conversationId: "execution-2" }),
    ], "re_executing");

    expect(entries.map((entry) => [entry.label, entry.attemptIndex, entry.conversationId])).toEqual([
      ["Execution attempt 1", 1, "execution-1"],
      ["Review attempt 1", 1, "review-1"],
      ["Revision Needed", undefined, undefined],
      ["Execution attempt 2", 2, "execution-2"],
    ]);
    expect(entries.at(-1)?.isCurrent).toBe(true);
  });

  it("uses explicit runtime context and stable transition ids when selecting history", () => {
    const [entry] = buildTaskHistoryEntries([
      transition("executing", t0, {
        conversationId: "merge-conversation",
        contextType: "merge",
        transitionId: "stable-transition-id",
      }),
      transition("approved", t1),
    ], "approved");

    expect(entry).toBeDefined();
    expect(entryToHistoryState(entry!)).toMatchObject({
      contextType: "merge",
      transitionId: "stable-transition-id",
      hasConversation: true,
    });
    expect(isSelectedTaskHistoryEntry(entry!, {
      status: "executing",
      timestamp: t0,
      transitionId: "stable-transition-id",
    })).toBe(true);
  });

  it("filters transient historical states but retains a transient current state", () => {
    expect(buildTaskHistoryEntries([
      transition("executing", t0),
      transition("pending_merge", t1),
      transition("merged", t2),
    ], "merged").map((entry) => entry.status)).toEqual(["executing", "merged"]);

    expect(buildTaskHistoryEntries([
      transition("executing", t0),
      transition("pending_merge", t1),
    ], "pending_merge").map((entry) => entry.status)).toEqual(["executing", "pending_merge"]);
  });

  it("keeps one merge attempt across its intermediate runtime stages", () => {
    const entries = buildTaskHistoryEntries([
      transition("merging", t0, { conversationId: "merge-1" }),
      transition("waiting_on_pr", t1, { conversationId: "merge-1" }),
      transition("merged", t2, { conversationId: "merge-1" }),
    ], "merged");

    expect(entries.map((entry) => [entry.label, entry.attemptIndex])).toEqual([
      ["Merge attempt 1", 1],
      ["Merge attempt 1", 1],
      ["Merge attempt 1", 1],
    ]);
    expect(entries.at(-1)?.isCurrent).toBe(true);
  });

  it("builds a standalone current stage and suppresses terminal retry intermediates", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-07T10:20:00Z"));
    try {
      expect(buildTaskHistoryEntries(undefined, "executing")[0]).toMatchObject({
        status: "executing",
        label: "Execution attempt 1",
        contextType: "task_execution",
        attemptIndex: 1,
        hasConversation: false,
      });
      expect(buildTaskHistoryEntries(undefined, "pending_merge")).toEqual([]);
      expect(buildTaskHistoryEntries([
        transition("executing", t0),
        transition("merge_incomplete", t1),
        transition("merged", t2),
      ], "merged").map((entry) => entry.status)).toEqual(["executing", "merged"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps non-runtime states selectable without a runtime context", () => {
    const [backlog] = buildTaskHistoryEntries([
      transition("backlog", t0),
      transition("cancelled", t1),
    ], "cancelled");

    expect(backlog).toMatchObject({ status: "backlog", hasConversation: false });
    expect(backlog).not.toHaveProperty("contextType");
    expect(backlog).not.toHaveProperty("attemptIndex");
  });

  it("adds a missing current stage without inventing a conversation", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-07T10:20:00Z"));
    try {
      const entries = buildTaskHistoryEntries([transition("executing", t0, { conversationId: "execution-1" })], "review_passed");
      expect(entries.at(-1)).toMatchObject({
        status: "review_passed",
        isCurrent: true,
        contextType: "review",
        hasConversation: false,
      });
    } finally {
      vi.useRealTimers();
    }
  });
});
