/**
 * DiffToolCallView.utils tests
 *
 * Tests for isTaskToolCall() and isDiffToolCall() predicates
 * which drive the ToolCallIndicator routing logic.
 */

import { describe, it, expect } from "vitest";
import {
  isTaskToolCall,
  isDiffToolCall,
  extractWriteDiff,
  getWorkspaceRelativeDiffPath,
  getDiffFilePathDisplay,
} from "./DiffToolCallView.utils";
import type { ToolCall } from "./ToolCallIndicator";

describe("isTaskToolCall", () => {
  describe("Task tool names", () => {
    it("returns true for 'Task' (capitalized)", () => {
      expect(isTaskToolCall("Task")).toBe(true);
    });

    it("returns true for 'task' (lowercase)", () => {
      expect(isTaskToolCall("task")).toBe(true);
    });

    it("returns true for 'TASK' (uppercase)", () => {
      expect(isTaskToolCall("TASK")).toBe(true);
    });

    it("returns true for 'TaSk' (mixed case)", () => {
      expect(isTaskToolCall("TaSk")).toBe(true);
    });
  });

  describe("Agent tool names (extended support)", () => {
    it("returns true for 'Agent' (capitalized)", () => {
      expect(isTaskToolCall("Agent")).toBe(true);
    });

    it("returns true for 'agent' (lowercase)", () => {
      expect(isTaskToolCall("agent")).toBe(true);
    });

    it("returns true for 'AGENT' (uppercase)", () => {
      expect(isTaskToolCall("AGENT")).toBe(true);
    });

    it("returns true for 'aGeNt' (mixed case)", () => {
      expect(isTaskToolCall("aGeNt")).toBe(true);
    });
  });

  describe("Non-subagent tool names", () => {
    it("returns false for 'Edit'", () => {
      expect(isTaskToolCall("Edit")).toBe(false);
    });

    it("returns false for 'Write'", () => {
      expect(isTaskToolCall("Write")).toBe(false);
    });

    it("returns false for 'Read'", () => {
      expect(isTaskToolCall("Read")).toBe(false);
    });

    it("returns false for 'Bash'", () => {
      expect(isTaskToolCall("Bash")).toBe(false);
    });

    it("returns false for 'Glob'", () => {
      expect(isTaskToolCall("Glob")).toBe(false);
    });

    it("returns false for 'Grep'", () => {
      expect(isTaskToolCall("Grep")).toBe(false);
    });

    it("returns false for empty string", () => {
      expect(isTaskToolCall("")).toBe(false);
    });

    it("returns false for 'update_task'", () => {
      expect(isTaskToolCall("update_task")).toBe(false);
    });
  });
});

describe("isDiffToolCall", () => {
  describe("Diff tool names", () => {
    it("returns true for 'Edit'", () => {
      expect(isDiffToolCall("Edit")).toBe(true);
    });

    it("returns true for 'edit' (lowercase)", () => {
      expect(isDiffToolCall("edit")).toBe(true);
    });

    it("returns true for 'EDIT' (uppercase)", () => {
      expect(isDiffToolCall("EDIT")).toBe(true);
    });

    it("returns true for 'Write'", () => {
      expect(isDiffToolCall("Write")).toBe(true);
    });

    it("returns true for 'write' (lowercase)", () => {
      expect(isDiffToolCall("write")).toBe(true);
    });

    it("returns true for 'WRITE' (uppercase)", () => {
      expect(isDiffToolCall("WRITE")).toBe(true);
    });
  });

  describe("Non-diff tool names", () => {
    it("returns false for 'Task'", () => {
      expect(isDiffToolCall("Task")).toBe(false);
    });

    it("returns false for 'Agent'", () => {
      expect(isDiffToolCall("Agent")).toBe(false);
    });

    it("returns false for 'Read'", () => {
      expect(isDiffToolCall("Read")).toBe(false);
    });

    it("returns false for 'Bash'", () => {
      expect(isDiffToolCall("Bash")).toBe(false);
    });

    it("returns false for empty string", () => {
      expect(isDiffToolCall("")).toBe(false);
    });
  });
});

describe("diff file path display", () => {
  it("returns a repo-relative path for files inside the workspace root", () => {
    expect(
      getWorkspaceRelativeDiffPath(
        "/tmp/ralphx/worktrees/conversation-1/frontend/src/App.tsx",
        "/tmp/ralphx/worktrees/conversation-1"
      )
    ).toBe("frontend/src/App.tsx");
  });

  it("does not treat sibling prefixes as inside the workspace root", () => {
    expect(
      getWorkspaceRelativeDiffPath(
        "/tmp/ralphx/worktrees/conversation-10/frontend/src/App.tsx",
        "/tmp/ralphx/worktrees/conversation-1"
      )
    ).toBeNull();
  });

  it("preserves the full path when the file is outside the workspace root", () => {
    expect(
      getDiffFilePathDisplay(
        "/tmp/outside/frontend/src/App.tsx",
        "/tmp/ralphx/worktrees/conversation-1"
      )
    ).toBe("/tmp/outside/frontend/src/App.tsx");
  });

  it("keeps already-relative paths unchanged", () => {
    expect(
      getDiffFilePathDisplay(
        "frontend/src/components/Chat/DiffToolCallView.tsx",
        "/tmp/ralphx/worktrees/conversation-1"
      )
    ).toBe("frontend/src/components/Chat/DiffToolCallView.tsx");
  });

  it("falls back to the supplied path when no workspace root is known", () => {
    expect(
      getDiffFilePathDisplay(
        "/tmp/ralphx/worktrees/conversation-1/frontend/src/App.tsx",
        null
      )
    ).toBe("/tmp/ralphx/worktrees/conversation-1/frontend/src/App.tsx");
  });
});

describe("extractWriteDiff", () => {
  it("renders confirmed new-file writes as added-line diffs", () => {
    const toolCall: ToolCall = {
      id: "tool-write-new",
      name: "write",
      arguments: {
        file_path: "src/new.ts",
        content: "first line\nsecond line",
      },
      diffContext: {
        filePath: "src/new.ts",
        oldFileExists: false,
      },
    };

    const diff = extractWriteDiff(toolCall);

    expect(diff).toMatchObject({
      displayKind: "diff",
      baselineUnavailable: false,
      newFile: true,
      additions: 2,
      deletions: 0,
    });
    expect(diff?.previewDiff?.hunks[0]?.lines.map((line) => line.kind)).toEqual([
      "addition",
      "addition",
    ]);
  });

  it("keeps writes without baseline evidence on the final-content fallback", () => {
    const toolCall: ToolCall = {
      id: "tool-write-unknown",
      name: "write",
      arguments: {
        file_path: "src/generated.txt",
        content: "final only",
      },
    };

    const diff = extractWriteDiff(toolCall);

    expect(diff).toMatchObject({
      displayKind: "final-content",
      baselineUnavailable: true,
      newFile: false,
      finalContent: "final only",
    });
  });
});
