import { describe, expect, it } from "vitest";

import { getWorkspaceRepairFocusTarget } from "./useAgentsViewController";

describe("getWorkspaceRepairFocusTarget", () => {
  it("prioritizes a Review fixer over a durable workspace repair attempt", () => {
    expect(
      getWorkspaceRepairFocusTarget({
        reviewFixerConversationId: "review-fixer-child",
        repairRuntimeConversationId: "repair-child",
        repairFixerKind: "workspace_repair",
      }),
    ).toEqual({
      type: "workspace_repair",
      conversationId: "review-fixer-child",
    });
  });

  it("uses only a durable workspace repair attempt and never a PR fixer", () => {
    expect(
      getWorkspaceRepairFocusTarget({
        reviewFixerConversationId: null,
        repairRuntimeConversationId: "repair-child",
        repairFixerKind: "workspace_repair",
      }),
    ).toEqual({ type: "workspace_repair", conversationId: "repair-child" });
    expect(
      getWorkspaceRepairFocusTarget({
        reviewFixerConversationId: null,
        repairRuntimeConversationId: "pr-fixer-child",
        repairFixerKind: "pr_fixer",
      }),
    ).toBeNull();
  });
});
