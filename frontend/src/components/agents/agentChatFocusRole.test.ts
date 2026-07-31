import { describe, expect, it } from "vitest";

import {
  getChatFocusRuntimeLabel,
  getChatFocusRuntimeRole,
  getChatFocusRuntimeTag,
} from "./agentChatFocusRole";

describe("chat focus role derivation", () => {
  it("derives reviewer and fixer role controls only from focused child chats", () => {
    expect(getChatFocusRuntimeRole({ type: "workspace" })).toBeNull();
    expect(
      getChatFocusRuntimeRole({
        type: "workspace_review",
        conversationId: "review-1",
      }),
    ).toBe("workspace_reviewer");
    expect(
      getChatFocusRuntimeRole({
        type: "workspace_repair",
        conversationId: "repair-1",
      }),
    ).toBe("workspace_repair");
    expect(
      getChatFocusRuntimeRole({ type: "pr_fixer", conversationId: "pr-1" }),
    ).toBe("pr_fixer");
    expect(
      getChatFocusRuntimeLabel({ type: "pr_fixer", conversationId: "pr-1" }),
    ).toBe("PR Fixer");
    expect(
      getChatFocusRuntimeTag({ type: "workspace_review", conversationId: "review-1" }),
    ).toBe("REV");
    expect(
      getChatFocusRuntimeTag({ type: "workspace_repair", conversationId: "repair-1" }),
    ).toBe("FIX");
  });
});
