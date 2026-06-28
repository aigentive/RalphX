/**
 * Unit tests for cross_project_guide semantic keyword detection
 * and filterCrossProjectPaths path filtering.
 */

import { describe, it, expect } from "vitest";
import { CROSS_PROJECT_KEYWORDS, filterCrossProjectPaths, stripMarkdownCodeBlocks } from "../index.js";

function detectsCrossProject(planText: string): boolean {
  const regex = new RegExp(CROSS_PROJECT_KEYWORDS.join("|"), "i");
  return regex.test(planText);
}

describe("CROSS_PROJECT_KEYWORDS — new semantic keywords", () => {
  it("detects 'separate repo'", () => {
    expect(detectsCrossProject("We will create a separate repo for this service.")).toBe(true);
  });

  it("detects 'separate repository'", () => {
    expect(detectsCrossProject("Deploy to a separate repository called external-mcp-jira.")).toBe(true);
  });

  it("detects 'new repo'", () => {
    expect(detectsCrossProject("Create a new repo under the organization.")).toBe(true);
  });

  it("detects 'new repository'", () => {
    expect(detectsCrossProject("Initialize a new repository for the plugin.")).toBe(true);
  });

  it("detects 'different codebase'", () => {
    expect(detectsCrossProject("The agent lives in a different codebase.")).toBe(true);
  });

  it("detects 'other codebase'", () => {
    expect(detectsCrossProject("Changes also needed in the other codebase.")).toBe(true);
  });

  it("detects 'monorepo boundary'", () => {
    expect(detectsCrossProject("This crosses a monorepo boundary.")).toBe(true);
  });

  it("detects 'external package'", () => {
    expect(detectsCrossProject("Publish as an external package on npm.")).toBe(true);
  });

  it("detects 'external module'", () => {
    expect(detectsCrossProject("Import from an external module.")).toBe(true);
  });

  it("is case-insensitive for new keywords", () => {
    expect(detectsCrossProject("Create a Separate Repo for the mcp server.")).toBe(true);
    expect(detectsCrossProject("NEW REPOSITORY setup required.")).toBe(true);
    expect(detectsCrossProject("DIFFERENT CODEBASE integration.")).toBe(true);
  });
});

describe("CROSS_PROJECT_KEYWORDS — regression: existing keywords still detected", () => {
  it("detects 'cross-project'", () => {
    expect(detectsCrossProject("This is a cross-project task.")).toBe(true);
  });

  it("detects 'cross project' (no hyphen)", () => {
    expect(detectsCrossProject("cross project orchestration needed.")).toBe(true);
  });

  it("detects 'multi-project'", () => {
    expect(detectsCrossProject("multi-project plan involving two repos.")).toBe(true);
  });

  it("detects 'target project'", () => {
    expect(detectsCrossProject("The target project is reefbot.ai.")).toBe(true);
  });

  it("detects 'another project'", () => {
    expect(detectsCrossProject("Execute steps in another project.")).toBe(true);
  });

  it("detects 'different project'", () => {
    expect(detectsCrossProject("This belongs to a different project.")).toBe(true);
  });

  it("detects 'project b'", () => {
    expect(detectsCrossProject("Work will happen in project b alongside project a.")).toBe(true);
  });

  it("does not trigger on unrelated text", () => {
    expect(detectsCrossProject("Refactor the authentication service.")).toBe(false);
    expect(detectsCrossProject("Fix the login flow bug.")).toBe(false);
    expect(detectsCrossProject("Add unit tests for the repository layer.")).toBe(false);
  });
});

describe("filterCrossProjectPaths", () => {
  const root = "/Users/example/Code/ralphx";

  it("returns empty array when all paths are within the project", () => {
    const paths = [
      "/Users/example/Code/ralphx/src/index.ts",
      "/Users/example/Code/ralphx/src-tauri/main.rs",
    ];
    expect(filterCrossProjectPaths(paths, root)).toEqual([]);
  });

  it("returns only out-of-project paths when mixed", () => {
    const paths = [
      "/Users/example/Code/ralphx/src/index.ts",
      "/Users/example/Code/other-project/src/main.ts",
    ];
    expect(filterCrossProjectPaths(paths, root)).toEqual([
      "/Users/example/Code/other-project/src/main.ts",
    ]);
  });

  it("returns all paths when projectWorkingDir is null", () => {
    const paths = [
      "/Users/example/Code/ralphx/src/index.ts",
      "/Users/example/Code/other/main.ts",
    ];
    expect(filterCrossProjectPaths(paths, null)).toEqual(paths);
  });

  it("filters out path that exactly equals the project root", () => {
    expect(filterCrossProjectPaths([root], root)).toEqual([]);
  });

  it("filters out path equal to root with trailing slash", () => {
    expect(filterCrossProjectPaths([root + "/"], root)).toEqual([]);
  });

  it("does not filter similar-prefix directory (e.g. ralphx-other)", () => {
    const paths = ["/Users/example/Code/ralphx-other/src/main.ts"];
    expect(filterCrossProjectPaths(paths, root)).toEqual(paths);
  });

  it("does not filter relative ../paths (no project root match)", () => {
    const paths = ["../some-sibling/file.ts"];
    expect(filterCrossProjectPaths(paths, root)).toEqual(paths);
  });

  it("handles project root with trailing slash correctly", () => {
    const paths = [
      "/Users/example/Code/ralphx/src/index.ts",
      "/Users/example/Code/other/main.ts",
    ];
    expect(filterCrossProjectPaths(paths, root + "/")).toEqual([
      "/Users/example/Code/other/main.ts",
    ]);
  });
});

describe("stripMarkdownCodeBlocks()", () => {
  it("removes fenced code block — content like ...>> inside block is gone", () => {
    const input = "Some prose\n```rust\nlet x = vec![...>>];\n```\nMore prose";
    const result = stripMarkdownCodeBlocks(input);
    expect(result).not.toContain("...>>");
    expect(result).toContain("Some prose");
    expect(result).toContain("More prose");
  });

  it("removes inline code backtick spans", () => {
    const input = "Run `../scripts/build.sh` to build the project.";
    const result = stripMarkdownCodeBlocks(input);
    expect(result).not.toContain("../scripts/build.sh");
    expect(result).toContain("to build the project");
  });

  it("handles unclosed fenced block gracefully without crashing", () => {
    const input = "Prose before\n```rust\nlet x = 1;\n// fence never closed";
    expect(() => stripMarkdownCodeBlocks(input)).not.toThrow();
    // unclosed fence: non-greedy regex won't match, content preserved as-is
    const result = stripMarkdownCodeBlocks(input);
    expect(typeof result).toBe("string");
  });

  it("preserves prose outside code blocks unchanged", () => {
    const input = "Deploy to ../other-project via CI.\n\n```bash\necho hello\n```\n\nSee docs.";
    const result = stripMarkdownCodeBlocks(input);
    expect(result).toContain("Deploy to ../other-project via CI.");
    expect(result).toContain("See docs.");
    expect(result).not.toContain("echo hello");
  });
});

describe("Pattern 2 tightening — ../ relative path detection", () => {
  // Pattern 2 requires ../ (not just ..) to avoid false positives on ellipsis/spread
  const pattern2 = /(?:^|\s|["'`])(\.\.\/[^\s"'`]+)/gm;

  function matchesPattern2(text: string): boolean {
    pattern2.lastIndex = 0;
    return pattern2.test(text);
  }

  it("does NOT match Rust spread/ellipsis ...>> (no slash after dots)", () => {
    expect(matchesPattern2("let x = Some(...>>);")).toBe(false);
  });

  it("does NOT match bare .. (parent dir reference without slash)", () => {
    expect(matchesPattern2("See .. for details")).toBe(false);
    expect(matchesPattern2("range 0..10")).toBe(false);
  });

  it("DOES match ../sibling-project/src (proper relative cross-project path)", () => {
    expect(matchesPattern2("path: ../sibling-project/src")).toBe(true);
  });

  it("DOES match ../path with leading space", () => {
    expect(matchesPattern2(" ../other-repo/lib/mod.rs")).toBe(true);
  });
});

describe("Integration — stripMarkdownCodeBlocks + path detection", () => {
  // Replicate the handler's core detection pipeline without Tauri calls
  function detectPaths(planText: string): string[] {
    const scanText = stripMarkdownCodeBlocks(planText);
    const crossProjectPatterns = [
      /(?:^|\s|["'`])(\/(home|Users|workspace|projects|srv|opt)\/[^\s"'`]+)/gm,
      /(?:^|\s|["'`])(\.\.\/[^\s"'`]+)/gm,
      /(?:target[_-]?project[_-]?path|project[_-]?path|working[_-]?directory)[:\s]+["']?([^\s"'`,\n]+)/gim,
    ];
    const rawDetectedPaths: string[] = [];
    for (const pattern of crossProjectPatterns) {
      const matches = [...scanText.matchAll(pattern)];
      for (const m of matches) {
        const p = (m[1] || m[0]).trim().replace(/^["'`]|["'`]$/g, "");
        if (p && !rawDetectedPaths.includes(p)) {
          rawDetectedPaths.push(p);
        }
      }
    }
    return rawDetectedPaths;
  }

  it("plan with only Rust code blocks and no real paths → no paths detected", () => {
    const plan = `
# Task: Refactor Parser

Implement the new parser using Rust iterators.

\`\`\`rust
fn parse(input: &str) -> Vec<Token> {
    input.chars().flat_map(|c| tokenize(c)).collect::<Vec<...>>()
}
let range = 0..items.len();
let spread = foo(bar, ...>>);
\`\`\`

No external project references here.
    `;
    const paths = detectPaths(plan);
    expect(paths).toHaveLength(0);
  });

  it("plan with code blocks AND real ../other-project in prose → path still detected", () => {
    const plan = `
# Task: Cross-project integration

The service logic lives in ../other-project/src/service.ts and must be updated.

\`\`\`rust
fn parse(input: &str) -> Vec<...>> {
    // code here
}
\`\`\`

After updating ../other-project/src/service.ts, re-run integration tests.
    `;
    const paths = detectPaths(plan);
    expect(paths.some((p) => p.startsWith("../other-project"))).toBe(true);
  });
});
