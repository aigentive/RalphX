import { describe, it, expect } from "vitest";
import {
  parseToolResultAsLines,
  normalizeDisplayPath,
  shortenPath,
  parseSearchResult,
  parseReadOutput,
} from "./shared.constants";

describe("parseToolResultAsLines", () => {
  it("returns empty array for null/undefined", () => {
    expect(parseToolResultAsLines(null)).toEqual([]);
    expect(parseToolResultAsLines(undefined)).toEqual([]);
  });

  it("returns empty array for empty string", () => {
    expect(parseToolResultAsLines("")).toEqual([]);
  });

  it("splits plain string by newlines", () => {
    expect(parseToolResultAsLines("a.ts\nb.ts\nc.ts")).toEqual([
      "a.ts",
      "b.ts",
      "c.ts",
    ]);
  });

  it("trims whitespace from each line", () => {
    expect(parseToolResultAsLines("  a.ts  \n  b.ts  ")).toEqual([
      "a.ts",
      "b.ts",
    ]);
  });

  it("filters empty lines", () => {
    expect(parseToolResultAsLines("a.ts\n\n\nb.ts\n")).toEqual([
      "a.ts",
      "b.ts",
    ]);
  });

  it("parses MCP wrapper [{type: 'text', text: '...'}]", () => {
    const result = [{ type: "text", text: "mcp1.ts\nmcp2.ts" }];
    expect(parseToolResultAsLines(result)).toEqual(["mcp1.ts", "mcp2.ts"]);
  });

  it("parses object with text property", () => {
    const result = { text: "obj1.ts\nobj2.ts" };
    expect(parseToolResultAsLines(result)).toEqual(["obj1.ts", "obj2.ts"]);
  });

  it("parses string array directly", () => {
    expect(parseToolResultAsLines(["a.ts", "b.ts"])).toEqual(["a.ts", "b.ts"]);
  });

  it("filters non-string items from arrays", () => {
    expect(parseToolResultAsLines(["a.ts", 42, "b.ts", null])).toEqual([
      "a.ts",
      "b.ts",
    ]);
  });

  it("returns empty array for empty MCP text", () => {
    expect(parseToolResultAsLines([{ type: "text", text: "" }])).toEqual([]);
  });
});

describe("normalizeDisplayPath", () => {
  it("returns empty string for empty input", () => {
    expect(normalizeDisplayPath("")).toBe("");
  });

  it("converts backslashes to forward slashes", () => {
    expect(normalizeDisplayPath("src\\components\\Chat.tsx")).toBe(
      "src/components/Chat.tsx",
    );
  });

  it("removes absolute prefix for known project segments", () => {
    expect(
      normalizeDisplayPath(
        "/Users/test/ralphx-worktrees/ralphx/task-abc/src/components/Chat.tsx",
      ),
    ).toBe("src/components/Chat.tsx");
  });

  it("handles src-tauri prefix", () => {
    expect(
      normalizeDisplayPath("/some/path/src-tauri/src/main.rs"),
    ).toBe("src-tauri/src/main.rs");
  });

  it("handles tests prefix", () => {
    expect(normalizeDisplayPath("/workspace/tests/unit/test.ts")).toBe(
      "tests/unit/test.ts",
    );
  });

  it("handles specs prefix", () => {
    expect(normalizeDisplayPath("/Users/me/project/specs/plan.md")).toBe(
      "specs/plan.md",
    );
  });

  it("handles scripts prefix", () => {
    expect(normalizeDisplayPath("/root/scripts/build.sh")).toBe(
      "scripts/build.sh",
    );
  });

  it("handles docs prefix", () => {
    expect(normalizeDisplayPath("/project/docs/api.md")).toBe("docs/api.md");
  });

  it("handles mockups prefix", () => {
    expect(normalizeDisplayPath("/work/mockups/design.html")).toBe(
      "mockups/design.html",
    );
  });

  it("handles assets prefix", () => {
    expect(normalizeDisplayPath("/app/assets/logo.png")).toBe(
      "assets/logo.png",
    );
  });

  it("handles public prefix", () => {
    expect(normalizeDisplayPath("/site/public/index.html")).toBe(
      "public/index.html",
    );
  });

  it("anchors at .claude", () => {
    expect(normalizeDisplayPath("/workspace/.claude/rules/foo.md")).toBe(
      ".claude/rules/foo.md",
    );
  });

  it("removes leading /.../ artifacts", () => {
    expect(normalizeDisplayPath("/.../src/main.ts")).toBe("src/main.ts");
  });

  it("removes leading .../ artifacts", () => {
    expect(normalizeDisplayPath(".../src/main.ts")).toBe("src/main.ts");
  });

  it("falls back to basename when no anchor found", () => {
    expect(normalizeDisplayPath("/unknown/path/to/file.txt")).toBe(
      "file.txt",
    );
  });

  it("returns path as-is if already repo-relative", () => {
    expect(normalizeDisplayPath("src/components/Chat.tsx")).toBe(
      "src/components/Chat.tsx",
    );
  });

  it("prefers earliest anchor in path", () => {
    expect(normalizeDisplayPath("/project/src/tests/foo.ts")).toBe(
      "src/tests/foo.ts",
    );
  });
});

describe("shortenPath", () => {
  it("returns path unchanged if under maxLength", () => {
    expect(shortenPath("src/components/Chat.tsx", 100)).toBe(
      "src/components/Chat.tsx",
    );
  });

  it("shortens long paths by keeping first and last two segments", () => {
    const long = "src/components/deeply/nested/path/Component.tsx";
    const result = shortenPath(long, 30);
    expect(result).toContain("Component.tsx");
    expect(result.length).toBeLessThanOrEqual(30);
  });

  it("returns just filename as last resort", () => {
    const result = shortenPath("a/b/c/d/e/f/VeryLongComponentName.tsx", 10);
    expect(result).toBe("VeryLongComponentName.tsx");
  });

  it("handles paths with only 2 parts", () => {
    expect(shortenPath("src/main.ts", 5)).toBe("src/main.ts");
  });

  it("handles single-part paths", () => {
    expect(shortenPath("file.txt", 5)).toBe("file.txt");
  });
});

describe("parseSearchResult", () => {
  it("returns empty for empty input", () => {
    const result = parseSearchResult("");
    expect(result.paths).toEqual([]);
    expect(result.isEmpty).toBe(true);
  });

  it("returns empty for null/undefined", () => {
    const result = parseSearchResult(null);
    expect(result.paths).toEqual([]);
    expect(result.isEmpty).toBe(true);
  });

  it("parses grep output with path:line:match format", () => {
    const result = "src/main.ts:42:const foo = bar;\nsrc/utils.ts:100:export";
    const parsed = parseSearchResult(result);
    expect(parsed.paths).toEqual(["src/main.ts", "src/utils.ts"]);
    expect(parsed.isEmpty).toBe(false);
    expect(parsed.note).toBeUndefined();
  });

  it("filters out metadata lines", () => {
    const result =
      "Found 3 files matching pattern\nsrc/a.ts:1:import\nsrc/b.ts:2:export\nPage 1 of 1";
    const parsed = parseSearchResult(result);
    expect(parsed.paths).toEqual(["src/a.ts", "src/b.ts"]);
  });

  it("detects no-match states", () => {
    const result = "No matches found";
    const parsed = parseSearchResult(result);
    expect(parsed.paths).toEqual([]);
    expect(parsed.isEmpty).toBe(true);
    expect(parsed.note).toBe("No matches found");
  });

  it("detects no files found", () => {
    const result = "No files found";
    const parsed = parseSearchResult(result);
    expect(parsed.paths).toEqual([]);
    expect(parsed.isEmpty).toBe(true);
    expect(parsed.note).toBe("No files found");
  });

  it("detects no files matched", () => {
    const result = "No files matched";
    const parsed = parseSearchResult(result);
    expect(parsed.paths).toEqual([]);
    expect(parsed.isEmpty).toBe(true);
    expect(parsed.note).toBe("No files matched");
  });

  it("deduplicates paths", () => {
    const result = "src/main.ts:1:a\nsrc/main.ts:2:b\nsrc/main.ts:3:c";
    const parsed = parseSearchResult(result);
    expect(parsed.paths).toEqual(["src/main.ts"]);
  });

  it("normalizes extracted paths", () => {
    const result =
      "/Users/me/project/src/main.ts:1:code\n/Users/me/project/src-tauri/src/lib.rs:5:rust";
    const parsed = parseSearchResult(result);
    expect(parsed.paths).toEqual(["src/main.ts", "src-tauri/src/lib.rs"]);
  });

  it("handles MCP wrapper format", () => {
    const result = [{ type: "text", text: "src/a.ts:1:code\nsrc/b.ts:2:more" }];
    const parsed = parseSearchResult(result);
    expect(parsed.paths).toEqual(["src/a.ts", "src/b.ts"]);
  });

  it("handles plain file list (glob output)", () => {
    const result = "src/a.ts\nsrc/b.ts\nsrc/c.ts";
    const parsed = parseSearchResult(result);
    expect(parsed.paths).toEqual(["src/a.ts", "src/b.ts", "src/c.ts"]);
  });

  it("collects exclusion notes without treating them as paths", () => {
    const parsed = parseSearchResult(
      [
        "ROOT: /workspace/project",
        "MATCHES: 1",
        "NOTE: 3 paths excluded by .gitignore (node_modules).",
        "src/app.ts:42: needle",
      ].join("\n"),
    );

    expect(parsed.paths).toEqual(["src/app.ts"]);
    expect(parsed.note).toBe("3 paths excluded by .gitignore (node_modules).");
  });

  it("drops grep group separators and extracts context-line paths", () => {
    const parsed = parseSearchResult(
      [
        "OUTPUT_MODE: content",
        "src/agent-chat-system.md-42- before",
        "src/agent-chat-system.md:43: needle",
        "--",
        "src/utils.ts-100- after",
      ].join("\n"),
    );

    expect(parsed.paths).toEqual(["src/agent-chat-system.md", "src/utils.ts"]);
  });

  it("keeps explicit empty-state text before collected notes", () => {
    const parsed = parseSearchResult(
      "NOTE: node_modules was excluded.\nNo matches found",
    );

    expect(parsed.paths).toEqual([]);
    expect(parsed.note).toBe("No matches found node_modules was excluded.");
  });
});

describe("parseReadOutput", () => {
  it("returns empty for null/undefined", () => {
    const result = parseReadOutput(null);
    expect(result.lines).toEqual([]);
    expect(result.inferredStartLine).toBe(1);
  });

  it("handles empty result", () => {
    const parsed = parseReadOutput("");
    expect(parsed.lines).toEqual([]);
    expect(parsed.inferredStartLine).toBe(1);
    expect("error" in parsed).toBe(false);
  });

  it("strips line-number prefixes (N→ format)", () => {
    const raw = "     1→import React from 'react';\n     2→\n     3→export default function App() {";
    const result = parseReadOutput(raw);
    expect(result.lines).toEqual([
      "import React from 'react';",
      "",
      "export default function App() {",
    ]);
  });

  it("strips line-number prefixes like '   500→'", () => {
    const result = "   500→const foo = 'bar';\n   501→const baz = 'qux';";
    const parsed = parseReadOutput(result);
    expect(parsed.lines).toEqual([
      "const foo = 'bar';",
      "const baz = 'qux';",
    ]);
    expect(parsed.inferredStartLine).toBe(500);
  });

  it("preserves code indentation after stripping prefix", () => {
    const raw = "     1→function foo() {\n     2→  const x = 1;\n     3→    return x;\n     4→}";
    const result = parseReadOutput(raw);
    expect(result.lines[1]).toBe("  const x = 1;");
    expect(result.lines[2]).toBe("    return x;");
  });

  it("infers start line from first prefix when offset is missing", () => {
    const raw = "    50→function hello() {\n    51→  return 'world';\n    52→}";
    const result = parseReadOutput(raw);
    expect(result.inferredStartLine).toBe(50);
  });

  it("uses explicit offset over inferred start line", () => {
    const raw = "    50→function hello() {\n    51→  return 'world';";
    const result = parseReadOutput(raw, 50);
    expect(result.inferredStartLine).toBe(50);
  });

  it("extracts error from tool_use_error XML wrapper", () => {
    const raw = "<tool_use_error>File not found: src/missing.ts</tool_use_error>";
    const result = parseReadOutput(raw);
    expect(result.error).toBe("File not found: src/missing.ts");
    expect(result.lines).toEqual([]);
  });

  it("handles plain text without prefixes", () => {
    const result = "plain line 1\nplain line 2";
    const parsed = parseReadOutput(result);
    expect(parsed.lines).toEqual(["plain line 1", "plain line 2"]);
    expect(parsed.inferredStartLine).toBe(1);
  });

  it("handles MCP wrapper format", () => {
    const result = [
      { type: "text", text: "     1→const x = 1;\n     2→const y = 2;" },
    ];
    const parsed = parseReadOutput(result);
    expect(parsed.lines).toEqual(["const x = 1;", "const y = 2;"]);
    expect(parsed.inferredStartLine).toBe(1);
  });

  it("preserves empty lines from output", () => {
    const result = "     1→line one\n     2→\n     3→line three";
    const parsed = parseReadOutput(result);
    expect(parsed.lines).toEqual(["line one", "", "line three"]);
  });
});
