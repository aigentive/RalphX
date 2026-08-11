import { describe, expect, it } from "vitest";
import type { ToolCall } from "./ToolCallIndicator";
import {
  formatToolActivitySummary,
  summarizeToolActivity,
} from "./tool-activity-summary";

function call(
  id: string,
  name: string,
  options: Partial<ToolCall> = {},
): ToolCall {
  return {
    id,
    name,
    arguments: {},
    ...options,
  };
}

describe("tool activity summaries", () => {
  it.each([
    ["Claude", "Write", "Edit", "mcp__ralphx__delegate_start"],
    ["Codex", "write", "edit", "ralphx::delegate_start"],
  ])("classifies equivalent %s activity with provider-neutral copy", (_provider, write, edit, delegate) => {
    const summary = summarizeToolActivity({
      toolCalls: [
        call("create-a", write, {
          arguments: { file_path: " src\\new.ts " },
          diffContext: { filePath: "src\\new.ts", oldFileExists: false },
        }),
        call("edit-a", edit, {
          arguments: { file_path: "src/new.ts" },
          diffContext: { filePath: "src/new.ts", oldFileExists: true },
        }),
        call("edit-b", edit, {
          arguments: { file_path: "src/existing.ts" },
          diffContext: { filePath: "src/existing.ts", oldFileExists: true },
        }),
        call("delegate", delegate, {
          arguments: { agent_name: "ralphx-general-explorer" },
          result: { job_id: "job-1", status: "running" },
        }),
      ],
    });

    expect(summary).toEqual({
      totalTools: 4,
      createdPaths: ["src/new.ts"],
      editedPaths: ["src/existing.ts"],
      changedPaths: [],
      delegatedJobKeys: ["job-1"],
    });
    expect(formatToolActivitySummary(summary)).toBe(
      "Agent called 4 tools, created 1 file, edited 1 file, and delegated 1 agent.",
    );
  });

  it("dedupes lifecycle updates, distinct files, and delegation controls truthfully", () => {
    const start = call("delegate-start", "delegate_start", {
      arguments: { agent_name: "ralphx-general-worker" },
      result: { job_id: "job-1", status: "running" },
    });
    const summary = summarizeToolActivity({
      toolCalls: [
        call("write-1", "write", {
          arguments: { file_path: "src/unknown.ts" },
          diffContext: { filePath: "src/unknown.ts" },
        }),
        call("write-1", "write", {
          arguments: { file_path: "src/unknown.ts" },
          diffContext: { filePath: "src/unknown.ts" },
          result: "done",
        }),
        start,
        { ...start, result: { job_id: "job-1", status: "completed" } },
        call("delegate-wait", "ralphx::delegate_wait", {
          arguments: { job_id: "job-1" },
        }),
      ],
    });

    expect(summary.totalTools).toBe(2);
    expect(summary.changedPaths).toEqual(["src/unknown.ts"]);
    expect(summary.delegatedJobKeys).toEqual(["job-1"]);
    expect(formatToolActivitySummary(summary)).toBe(
      "Agent called 2 tools, changed 1 file, and delegated 1 agent.",
    );
  });

  it("dedupes matching streaming task metadata against persisted tool calls", () => {
    const summary = summarizeToolActivity({
      toolCalls: [
        call("delegate-1", "ralphx::delegate_start", {
          result: { job_id: "job-1", status: "running" },
        }),
      ],
      tasks: [
        {
          toolUseId: "delegate-1",
          toolName: "ralphx::delegate_start",
          delegatedJobId: "job-1",
        },
      ],
    });

    expect(summary.totalTools).toBe(1);
    expect(summary.delegatedJobKeys).toEqual(["job-1"]);
  });

  it("counts provider and lifecycle aliases for one job as one logical tool", () => {
    const summary = summarizeToolActivity({
      tasks: [
        {
          toolUseId: "provider-tool",
          toolName: "delegate_start",
          delegatedJobId: "job-1",
        },
        {
          toolUseId: "delegate-job:job-1",
          toolName: "delegate_start",
          delegatedJobId: "job-1",
        },
      ],
    });

    expect(summary.totalTools).toBe(1);
    expect(summary.delegatedJobKeys).toEqual(["job-1"]);
    expect(formatToolActivitySummary(summary)).toBe(
      "Agent called 1 tool and delegated 1 agent.",
    );
  });

  it("falls back to an inclusive generic count when metadata is incomplete", () => {
    const summary = summarizeToolActivity({
      toolCalls: [call("one", "custom_tool"), call("two", "write")],
    });

    expect(formatToolActivitySummary(summary)).toBe("Agent called 2 tools.");
  });
});
