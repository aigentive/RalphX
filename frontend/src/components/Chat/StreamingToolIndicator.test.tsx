/**
 * StreamingToolIndicator tests
 *
 * Covers:
 * - Basic rendering (no tool calls, with tool calls)
 * - Elapsed timer renders for active bash tool call with toolCallStartTimes
 * - No elapsed timer for completed tool calls (with result)
 * - No elapsed timer when isActive=false
 * - Timer updates every second via setInterval
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import { StreamingToolIndicator } from "./StreamingToolIndicator";
import type { ToolCall } from "./ToolCallIndicator";

// ============================================================================
// Helpers
// ============================================================================

function makeBashCall(id: string, withResult = false): ToolCall {
  const tc: ToolCall = {
    id,
    name: "Bash",
    arguments: { command: "cargo test --lib", description: "Run tests" },
  };
  if (withResult) {
    tc.result = "test output";
  }
  return tc;
}

function makeReadCall(id: string): ToolCall {
  return {
    id,
    name: "Read",
    arguments: { file_path: "src/main.rs" },
  };
}

/** Click the expand button to open the tool call list */
function expandIndicator() {
  fireEvent.click(screen.getByRole("button"));
}

// ============================================================================
// Basic rendering
// ============================================================================

describe("StreamingToolIndicator — basic", () => {
  it("returns null when toolCalls is empty", () => {
    const { container } = render(
      <StreamingToolIndicator toolCalls={[]} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders indicator with tool calls present", () => {
    render(
      <StreamingToolIndicator toolCalls={[makeBashCall("tc-1")]} />
    );
    expect(screen.getByTestId("streaming-tool-indicator")).toBeInTheDocument();
  });

  it("shows count of tool calls in header", () => {
    const calls = [makeBashCall("tc-1"), makeReadCall("tc-2")];
    render(<StreamingToolIndicator toolCalls={calls} />);
    expect(screen.getByText(/2 tool calls/)).toBeInTheDocument();
  });

  it("expands on click to show tool call list", () => {
    render(
      <StreamingToolIndicator toolCalls={[makeBashCall("tc-1")]} />
    );
    expandIndicator();
    expect(screen.getByText("Running")).toBeInTheDocument();
  });
});

// ============================================================================
// Elapsed timer — active tool calls
// ============================================================================

describe("StreamingToolIndicator — elapsed timer", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows elapsed timer for active bash tool call when expanded", () => {
    const now = Date.now();
    const toolCallId = "tc-bash-1";
    const startedAt = now - 63_000; // 1m 3s ago

    render(
      <StreamingToolIndicator
        toolCalls={[makeBashCall(toolCallId)]}
        isActive={true}
        toolCallStartTimes={{ [toolCallId]: startedAt }}
      />
    );

    expandIndicator();

    expect(screen.getByTestId("elapsed-timer")).toBeInTheDocument();
    expect(screen.getByText(/Running 1m 3s/)).toBeInTheDocument();
  });

  it("elapsed timer updates after 1 second", () => {
    const now = Date.now();
    const toolCallId = "tc-bash-2";
    const startedAt = now - 10_000; // 10s ago

    render(
      <StreamingToolIndicator
        toolCalls={[makeBashCall(toolCallId)]}
        isActive={true}
        toolCallStartTimes={{ [toolCallId]: startedAt }}
      />
    );

    expandIndicator();
    expect(screen.getByText(/Running 10s/)).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(1000);
    });

    expect(screen.getByText(/Running 11s/)).toBeInTheDocument();
  });

  it("does not show elapsed timer for tool call with result (completed)", () => {
    const now = Date.now();
    const toolCallId = "tc-bash-completed";

    render(
      <StreamingToolIndicator
        toolCalls={[makeBashCall(toolCallId, true)]}
        isActive={true}
        toolCallStartTimes={{ [toolCallId]: now - 30_000 }}
      />
    );

    expandIndicator();

    expect(screen.queryByTestId("elapsed-timer")).not.toBeInTheDocument();
  });

  it("does not show elapsed timer when isActive is false", () => {
    const now = Date.now();
    const toolCallId = "tc-bash-3";

    render(
      <StreamingToolIndicator
        toolCalls={[makeBashCall(toolCallId)]}
        isActive={false}
        toolCallStartTimes={{ [toolCallId]: now - 30_000 }}
      />
    );

    expandIndicator();

    expect(screen.queryByTestId("elapsed-timer")).not.toBeInTheDocument();
  });

  it("does not show elapsed timer when toolCallStartTimes is not provided", () => {
    render(
      <StreamingToolIndicator
        toolCalls={[makeBashCall("tc-bash-4")]}
        isActive={true}
      />
    );

    expandIndicator();

    expect(screen.queryByTestId("elapsed-timer")).not.toBeInTheDocument();
  });

  it("shows elapsed timer only for last active call when multiple calls present", () => {
    const now = Date.now();
    const completedId = "tc-bash-completed";
    const activeId = "tc-bash-active";

    const calls = [
      makeBashCall(completedId, true), // completed
      makeBashCall(activeId),           // active
    ];

    render(
      <StreamingToolIndicator
        toolCalls={calls}
        isActive={true}
        toolCallStartTimes={{
          [completedId]: now - 60_000,
          [activeId]: now - 5_000,
        }}
      />
    );

    expandIndicator();

    // Exactly one elapsed timer shown (for the active call only)
    const timers = screen.getAllByTestId("elapsed-timer");
    expect(timers).toHaveLength(1);
    expect(screen.getByText(/Running 5s/)).toBeInTheDocument();
  });
});

// ============================================================================
// Tool summary formatting — covers createToolSummary + getToolVerb branches
// ============================================================================

describe("StreamingToolIndicator — tool formatting", () => {
  it("renders Read tool with file_path", () => {
    const tc: ToolCall = {
      id: "r1",
      name: "Read",
      arguments: { file_path: "/path/to/file.rs" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Reading")).toBeInTheDocument();
    expect(screen.getByText("/path/to/file.rs")).toBeInTheDocument();
  });

  it("renders Read tool with default fallback when file_path missing", () => {
    const tc: ToolCall = { id: "r2", name: "Read", arguments: {} };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Reading file")).toBeInTheDocument();
  });

  it("renders Write tool with file_path", () => {
    const tc: ToolCall = {
      id: "w1",
      name: "Write",
      arguments: { file_path: "out.txt" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Writing")).toBeInTheDocument();
    expect(screen.getByText("out.txt")).toBeInTheDocument();
  });

  it("renders Write tool fallback", () => {
    const tc: ToolCall = { id: "w2", name: "Write", arguments: {} };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Writing file")).toBeInTheDocument();
  });

  it("renders Edit tool with file_path", () => {
    const tc: ToolCall = {
      id: "e1",
      name: "Edit",
      arguments: { file_path: "src/x.ts" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Editing")).toBeInTheDocument();
    expect(screen.getByText("src/x.ts")).toBeInTheDocument();
  });

  it("renders Edit tool fallback", () => {
    const tc: ToolCall = { id: "e2", name: "Edit", arguments: {} };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Editing file")).toBeInTheDocument();
  });

  it("renders Glob with pattern and path detail", () => {
    const tc: ToolCall = {
      id: "g1",
      name: "Glob",
      arguments: { pattern: "**/*.ts", path: "src" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Finding")).toBeInTheDocument();
    expect(screen.getByText("**/*.ts")).toBeInTheDocument();
    expect(screen.getByText("in src")).toBeInTheDocument();
  });

  it("renders Glob fallback when no pattern/path", () => {
    const tc: ToolCall = { id: "g2", name: "Glob", arguments: {} };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Searching files")).toBeInTheDocument();
  });

  it("renders Grep with pattern and path", () => {
    const tc: ToolCall = {
      id: "gr1",
      name: "Grep",
      arguments: { pattern: "TODO", path: "src" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Searching")).toBeInTheDocument();
    expect(screen.getByText('"TODO"')).toBeInTheDocument();
    expect(screen.getByText("in src")).toBeInTheDocument();
  });

  it("renders Grep fallback when no pattern", () => {
    const tc: ToolCall = { id: "gr2", name: "Grep", arguments: {} };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Searching content")).toBeInTheDocument();
  });

  it("renders create_task_proposal with title", () => {
    const tc: ToolCall = {
      id: "ctp1",
      name: "create_task_proposal",
      arguments: { title: "Add login" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Creating")).toBeInTheDocument();
    expect(screen.getByText("Add login")).toBeInTheDocument();
  });

  it("renders create_task_proposal fallback", () => {
    const tc: ToolCall = { id: "ctp2", name: "create_task_proposal", arguments: {} };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Creating proposal")).toBeInTheDocument();
  });

  it("renders update_task_proposal with title", () => {
    const tc: ToolCall = {
      id: "u1",
      name: "update_task_proposal",
      arguments: { title: "Updated title" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Updating")).toBeInTheDocument();
    expect(screen.getByText("Updated title")).toBeInTheDocument();
  });

  it("renders update_task_proposal with proposal_id fallback", () => {
    const tc: ToolCall = {
      id: "u2",
      name: "update_task_proposal",
      arguments: { proposal_id: "abcd1234ef" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText(/Updating abcd1234/)).toBeInTheDocument();
  });

  it("renders update_task_proposal with no id/title fallback", () => {
    const tc: ToolCall = { id: "u3", name: "update_task_proposal", arguments: {} };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText(/Updating proposal/)).toBeInTheDocument();
  });

  it("renders get_task_context", () => {
    const tc: ToolCall = { id: "gt1", name: "get_task_context", arguments: {} };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Fetching")).toBeInTheDocument();
    expect(screen.getByText("Fetching task context")).toBeInTheDocument();
  });

  it("renders Bash with description-only (no command)", () => {
    const tc: ToolCall = {
      id: "b1",
      name: "Bash",
      arguments: { description: "Run setup" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Run setup")).toBeInTheDocument();
  });

  it("renders Bash with command-only (no description)", () => {
    const tc: ToolCall = {
      id: "b2",
      name: "Bash",
      arguments: { command: "ls -la" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText(/\$ ls -la/)).toBeInTheDocument();
  });

  it("renders Bash with long command truncation", () => {
    const longCmd = "echo " + "x".repeat(200);
    const tc: ToolCall = {
      id: "b3",
      name: "Bash",
      arguments: { command: longCmd },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    // Truncated to 50 chars + ellipsis on the primary line "$ echo xxx...x..."
    expect(screen.getByText(/^\$ echo x+\.\.\.$/)).toBeInTheDocument();
  });

  it("renders Bash fallback with no command/description", () => {
    const tc: ToolCall = { id: "b4", name: "Bash", arguments: {} };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Running command")).toBeInTheDocument();
  });

  it("renders Bash with both desc and long command shows command detail", () => {
    const longCmd = "git log --all --oneline --decorate --graph " + "y".repeat(30);
    const tc: ToolCall = {
      id: "b5",
      name: "Bash",
      arguments: { command: longCmd, description: "Show graph" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Show graph")).toBeInTheDocument();
    // command detail line truncated at 60 chars
    expect(screen.getByText(/git log/)).toBeInTheDocument();
  });

  it("renders unknown tool with snake_case name converted to readable", () => {
    const tc: ToolCall = {
      id: "u4",
      name: "delete_task_proposal",
      arguments: { proposal_id: "p1", reason: "duplicate" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Deleting")).toBeInTheDocument();
    expect(screen.getByText("delete task proposal")).toBeInTheDocument();
    // Args extracted as details
    expect(screen.getByText(/proposal id: p1/)).toBeInTheDocument();
    expect(screen.getByText(/reason: duplicate/)).toBeInTheDocument();
  });

  it("renders get_artifact verb (Fetching)", () => {
    const tc: ToolCall = {
      id: "ga1",
      name: "get_artifact",
      arguments: { artifact_id: "art1" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Fetching")).toBeInTheDocument();
  });

  it("uses default 'Calling' verb for fully unknown tool", () => {
    const tc: ToolCall = {
      id: "x1",
      name: "some_random_tool",
      arguments: { foo: "bar" },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("Calling")).toBeInTheDocument();
  });

  it("extractArgumentSummary truncates long string values", () => {
    const longVal = "z".repeat(80);
    const tc: ToolCall = {
      id: "x2",
      name: "custom_tool",
      arguments: { description: longVal },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText(/description: z+\.\.\./)).toBeInTheDocument();
  });

  it("extractArgumentSummary skips nested object/array and null values", () => {
    const tc: ToolCall = {
      id: "x3",
      name: "custom_tool",
      arguments: {
        nested: { a: 1 },
        list: [1, 2],
        empty: null,
        valid: "ok",
      },
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText(/valid: ok/)).toBeInTheDocument();
    expect(screen.queryByText(/nested:/)).not.toBeInTheDocument();
    expect(screen.queryByText(/list:/)).not.toBeInTheDocument();
    expect(screen.queryByText(/empty:/)).not.toBeInTheDocument();
  });

  it("extractArgumentSummary returns no details when args is array", () => {
    const tc: ToolCall = {
      id: "x4",
      name: "custom_tool",
      arguments: ["a", "b"],
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("custom tool")).toBeInTheDocument();
  });

  it("extractArgumentSummary handles undefined args", () => {
    const tc: ToolCall = {
      id: "x5",
      name: "custom_tool",
      arguments: undefined,
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    expect(screen.getByText("custom tool")).toBeInTheDocument();
  });
});

// ============================================================================
// Filtering, header state, expand/collapse
// ============================================================================

describe("StreamingToolIndicator — filtering and state", () => {
  it("filters out result:toolu* tool calls", () => {
    const calls: ToolCall[] = [
      makeBashCall("tc-1"),
      { id: "tc-r", name: "result:toolu_abc", arguments: {} },
    ];
    render(<StreamingToolIndicator toolCalls={calls} />);
    // Only 1 tool call counted in header
    expect(screen.getByText(/1 tool call\b/)).toBeInTheDocument();
    expect(screen.queryByText(/2 tool calls/)).not.toBeInTheDocument();
  });

  it("returns null if all tool calls are filtered out (only result:toolu*)", () => {
    const calls: ToolCall[] = [
      { id: "r1", name: "result:toolu_a", arguments: {} },
      { id: "r2", name: "result:toolu_b", arguments: {} },
    ];
    const { container } = render(<StreamingToolIndicator toolCalls={calls} />);
    expect(container.firstChild).toBeNull();
  });

  it("shows 'Working...' header when isActive=true", () => {
    render(<StreamingToolIndicator toolCalls={[makeBashCall("a")]} isActive={true} />);
    expect(screen.getByText(/Working\.\.\./)).toBeInTheDocument();
  });

  it("shows 'Done' header when isActive=false", () => {
    render(<StreamingToolIndicator toolCalls={[makeBashCall("a")]} isActive={false} />);
    expect(screen.getByText(/Done/)).toBeInTheDocument();
  });

  it("toggles aria-expanded between collapsed and expanded", () => {
    render(<StreamingToolIndicator toolCalls={[makeBashCall("a")]} />);
    const btn = screen.getByRole("button");
    expect(btn).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(btn);
    expect(btn).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(btn);
    expect(btn).toHaveAttribute("aria-expanded", "false");
  });

  it("does not show tool list when collapsed", () => {
    render(<StreamingToolIndicator toolCalls={[makeReadCall("r")]} />);
    expect(screen.queryByText("Reading")).not.toBeInTheDocument();
  });

  it("renders error styling when tool call has error", () => {
    const tc: ToolCall = {
      id: "err1",
      name: "Bash",
      arguments: { command: "false" },
      error: "exit code 1",
    };
    render(<StreamingToolIndicator toolCalls={[tc]} />);
    expandIndicator();
    // Error tool still rendered
    expect(screen.getByText("Running")).toBeInTheDocument();
  });

  it("renders pluralization correctly: '1 tool call' singular", () => {
    render(<StreamingToolIndicator toolCalls={[makeBashCall("a")]} />);
    expect(screen.getByText(/1 tool call\b/)).toBeInTheDocument();
  });

  it("active dots indicator renders only when isActive=true and expanded", () => {
    const { rerender } = render(
      <StreamingToolIndicator toolCalls={[makeBashCall("a")]} isActive={true} />
    );
    expandIndicator();
    // Active dots present in DOM (animate-pulse)
    expect(document.querySelectorAll(".animate-pulse").length).toBeGreaterThan(0);

    rerender(
      <StreamingToolIndicator toolCalls={[makeBashCall("a")]} isActive={false} />
    );
    // After rerender with isActive=false, expanded state still true
    expect(document.querySelectorAll(".animate-pulse").length).toBe(0);
  });

  it("scroll handler updates near-bottom tracking (no error on scroll)", () => {
    render(
      <StreamingToolIndicator toolCalls={[makeBashCall("a"), makeReadCall("b")]} />
    );
    expandIndicator();
    const container = screen.getByTestId("streaming-tool-indicator");
    // Find scrollable child (has overflow-y-auto class)
    const scrollable = container.querySelector(".overflow-y-auto") as HTMLElement;
    expect(scrollable).toBeTruthy();
    // Trigger scroll — should not throw
    fireEvent.scroll(scrollable, { target: { scrollTop: 100 } });
  });

  it("does not show elapsed timer line when elapsedSeconds is 0", () => {
    vi.useFakeTimers();
    const now = Date.now();
    const id = "tc-zero";
    // Start time = now (elapsed = 0)
    render(
      <StreamingToolIndicator
        toolCalls={[makeBashCall(id)]}
        isActive={true}
        toolCallStartTimes={{ [id]: now }}
      />
    );
    expandIndicator();
    expect(screen.queryByTestId("elapsed-timer")).not.toBeInTheDocument();
    vi.useRealTimers();
  });
});
