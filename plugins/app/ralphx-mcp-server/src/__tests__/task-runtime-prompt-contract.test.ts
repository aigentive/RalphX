import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

import { getAllTools } from "../tools.js";

const taskAgentPromptPaths = [
  "agents/ralphx-execution-worker/claude/prompt.md",
  "agents/ralphx-execution-worker/codex/prompt.md",
  "agents/ralphx-execution-reviewer/claude/prompt.md",
  "agents/ralphx-execution-reviewer/codex/prompt.md",
  "agents/ralphx-execution-coder/claude/prompt.md",
  "agents/ralphx-execution-coder/codex/prompt.md",
] as const;

function readRepoFile(relativePath: string): string {
  return readFileSync(new URL(`../../../../../${relativePath}`, import.meta.url), "utf8");
}

describe("task runtime prompt contract", () => {
  it("describes get_task_context as the authoritative task refresh, not an unconditional first action", () => {
    const tool = getAllTools().find((candidate) => candidate.name === "get_task_context");
    expect(tool).toBeDefined();

    const description = tool?.description ?? "";
    expect(description).toContain("authoritative");
    expect(description).toContain("bootstrap");
    expect(description).not.toMatch(/call this first/i);
  });

  it.each(taskAgentPromptPaths)(
    "%s treats injected runtime context as bootstrap-only task context",
    (relativePath) => {
      const prompt = readRepoFile(relativePath);

      expect(prompt).toContain("<task_runtime_context>");
      expect(prompt).toMatch(/bootstrap context/i);
      expect(prompt).toMatch(/not final authority/i);
      expect(prompt).toMatch(/get_task_context/);
      expect(prompt).toMatch(/absent|blocked|stale|incomplete|full/i);
      expect(prompt).not.toMatch(/Start with `get_task_context/i);
      expect(prompt).not.toMatch(/Call `get_task_context\(task_id\)` before coding/i);
      expect(prompt).not.toMatch(/`get_task_context`\s*\|\s*ALWAYS/i);
      expect(prompt).not.toMatch(
        /visible synthetic|synthetic user-message|Execute task:|Review task:|Re-execute task/i
      );
    }
  );

  it("keeps mandatory review and validation reads in task agent prompts", () => {
    for (const relativePath of [
      "agents/ralphx-execution-worker/claude/prompt.md",
      "agents/ralphx-execution-worker/codex/prompt.md",
      "agents/ralphx-execution-coder/claude/prompt.md",
      "agents/ralphx-execution-coder/codex/prompt.md",
    ]) {
      const prompt = readRepoFile(relativePath);
      expect(prompt).toContain("get_review_notes");
      expect(prompt).toContain("get_task_issues");
      expect(prompt).toContain("run_task_validation");
    }

    for (const relativePath of [
      "agents/ralphx-execution-reviewer/claude/prompt.md",
      "agents/ralphx-execution-reviewer/codex/prompt.md",
    ]) {
      const prompt = readRepoFile(relativePath);
      expect(prompt).toContain("get_task_diff");
      expect(prompt).toContain("get_task_validation_summary");
      expect(prompt).toContain("complete_review");
    }
  });
});
