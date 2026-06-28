import { describe, expect, it } from "vitest";

import { shouldPreserveExecutionPopoverForTarget } from "./executionPopoverDismissal";

describe("shouldPreserveExecutionPopoverForTarget", () => {
  it("preserves execution popovers for clicks inside agent session rows", () => {
    const row = document.createElement("div");
    row.setAttribute("data-testid", "agents-session-conversation-1");
    const child = document.createElement("span");
    row.appendChild(child);

    expect(shouldPreserveExecutionPopoverForTarget(child)).toBe(true);
  });

  it("does not preserve execution popovers for unrelated outside targets", () => {
    const button = document.createElement("button");
    button.setAttribute("data-testid", "settings-button");

    expect(shouldPreserveExecutionPopoverForTarget(button)).toBe(false);
    expect(shouldPreserveExecutionPopoverForTarget(null)).toBe(false);
  });
});
