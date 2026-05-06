import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { HookEventMessage } from "./HookEventMessage";
import type {
  HookStartedEvent,
  HookCompletedEvent,
  HookBlockEvent,
} from "@/types/hook-event";

const base = {
  conversationId: "c1",
  contextType: "ideation",
  contextId: "session-1",
  timestamp: Date.now(),
} as const;

describe("HookEventMessage", () => {
  it("renders the started variant with running label", () => {
    const event: HookStartedEvent = {
      ...base,
      type: "started",
      hookName: "lint",
      hookEvent: "PreToolUse",
      hookId: "h1",
    };
    render(<HookEventMessage event={event} />);
    expect(screen.getByText(/Running PreToolUse hook/i)).toBeInTheDocument();
  });

  it("renders the completed success variant with collapsible output", async () => {
    const user = userEvent.setup();
    const event: HookCompletedEvent = {
      ...base,
      type: "completed",
      hookName: "lint",
      hookEvent: "PostToolUse",
      hookId: "h1",
      output: "lint output line\nanother",
      outcome: "ok",
      exitCode: 0,
    };
    render(<HookEventMessage event={event} />);
    const trigger = screen.getByRole("button");
    await user.click(trigger);
    expect(screen.getByText(/lint output line/i)).toBeInTheDocument();
  });

  it("renders the block variant with reason copy", () => {
    const event: HookBlockEvent = {
      ...base,
      type: "block",
      hookName: "stop-guard",
      reason: "Stop hook blocked: dirty worktree",
    };
    render(<HookEventMessage event={event} />);
    expect(screen.getAllByText(/Stop hook blocked/i).length).toBeGreaterThan(0);
  });
});
