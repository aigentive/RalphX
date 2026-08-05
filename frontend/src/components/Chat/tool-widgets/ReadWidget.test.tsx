import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ReadWidget } from "./ReadWidget";
import type { ToolCall } from "./shared.constants";

function makeReadCall(overrides: Partial<ToolCall> = {}): ToolCall {
  return {
    id: "read-1",
    name: "Read",
    arguments: { file_path: "/Users/dev/project/src/app.ts" },
    result: "     1→import React from 'react';\n     2→\n     3→export default function App() {",
    ...overrides,
  };
}

describe("ReadWidget", () => {
  describe("path normalization", () => {
    it("shows repo-relative path in header for absolute path", () => {
      render(<ReadWidget toolCall={makeReadCall()} />);
      // normalizeDisplayPath anchors at "src" → "src/app.ts"
      expect(screen.getByText("src/app.ts")).toBeInTheDocument();
    });

    it("shows relative path as-is", () => {
      render(
        <ReadWidget
          toolCall={makeReadCall({
            arguments: { file_path: "src/utils/helpers.ts" },
          })}
        />,
      );
      expect(screen.getByText("src/utils/helpers.ts")).toBeInTheDocument();
    });

    it("shortens long paths", () => {
      const longPath = "/Users/dev/project/src/components/deeply/nested/path/to/Component.tsx";
      render(
        <ReadWidget
          toolCall={makeReadCall({ arguments: { file_path: longPath } })}
        />,
      );
      // Should be shortened but still contain the filename
      const header = screen.getByText(/Component\.tsx/);
      expect(header).toBeInTheDocument();
    });
  });

  describe("content parsing", () => {
    it("renders parsed lines without duplicate line-number prefixes", () => {
      const result = "     1→import React from 'react';\n     2→\n     3→export default function App() {";
      render(
        <ReadWidget toolCall={makeReadCall({ result })} />,
      );
      // Should show the code without the "     N→" prefix
      expect(screen.getByText("import React from 'react';")).toBeInTheDocument();
      expect(screen.getByText("export default function App() {")).toBeInTheDocument();
    });

    it("shows line count badge", () => {
      const result = "     1→line1\n     2→line2\n     3→line3";
      render(
        <ReadWidget toolCall={makeReadCall({ result })} />,
      );
      expect(screen.getByText("3 lines")).toBeInTheDocument();
    });

    it("shows singular line badge for 1 line", () => {
      const result = "     1→single line";
      render(
        <ReadWidget toolCall={makeReadCall({ result })} />,
      );
      expect(screen.getByText("1 line")).toBeInTheDocument();
    });
  });

  describe("start line inference", () => {
    it("infers start line from prefixed output when offset is missing", () => {
      const result = "    50→function hello() {\n    51→  return 'world';\n    52→}";
      const { container } = render(
        <ReadWidget
          toolCall={makeReadCall({
            arguments: { file_path: "src/test.ts" },
            result,
          })}
        />,
      );
      // CodePreview renders line numbers starting from inferredStartLine (50)
      expect(container.textContent).toContain("50");
      expect(container.textContent).toContain("51");
    });

    it("uses explicit offset when provided", () => {
      const result = "    10→const x = 1;\n    11→const y = 2;";
      const { container } = render(
        <ReadWidget
          toolCall={makeReadCall({
            arguments: { file_path: "src/test.ts", offset: 10 },
            result,
          })}
        />,
      );
      expect(container.textContent).toContain("10");
    });
  });

  describe("error handling", () => {
    it("shows clean error text from tool_use_error wrapper", () => {
      const result = "<tool_use_error>File not found: src/missing.ts</tool_use_error>";
      render(
        <ReadWidget
          toolCall={makeReadCall({
            arguments: { file_path: "src/missing.ts" },
            result,
          })}
        />,
      );
      expect(screen.getByText("File not found: src/missing.ts")).toBeInTheDocument();
      expect(screen.getByText("error")).toBeInTheDocument();
    });

    it("shows error badge when toolCall.error is set", () => {
      render(
        <ReadWidget
          toolCall={makeReadCall({ error: "Permission denied" })}
        />,
      );
      expect(screen.getByText("error")).toBeInTheDocument();
    });
  });

  describe("pending state", () => {
    it("shows 'Reading...' when result is undefined", () => {
      render(
        <ReadWidget
          toolCall={makeReadCall({ result: undefined })}
        />,
      );
      expect(screen.getByText("Reading...")).toBeInTheDocument();
    });
  });

  describe("compact mode", () => {
    it("passes compact prop without crashing", () => {
      render(<ReadWidget toolCall={makeReadCall()} compact />);
      expect(screen.getByText("src/app.ts")).toBeInTheDocument();
    });
  });

  describe("MCP wrapper result", () => {
    it("handles MCP wrapper [{type: 'text', text: '...'}]", () => {
      render(
        <ReadWidget
          toolCall={makeReadCall({
            result: [{ type: "text", text: "     1→const a = 1;\n     2→const b = 2;" }],
          })}
        />,
      );
      expect(screen.getByText("const a = 1;")).toBeInTheDocument();
      expect(screen.getByText("const b = 2;")).toBeInTheDocument();
    });

    it("parses fs_read_file MCP payloads with metadata headers and pipe-prefixed lines", () => {
      render(
        <ReadWidget
          toolCall={makeReadCall({
            arguments: {
              path: "frontend/src/components/TaskGraph/hooks/useExecutionTimeline.ts",
              start_line: 1,
              end_line: 2,
            },
            result: [
              {
                type: "text",
                text: [
                  "FILE: /workspace/project/frontend/src/components/TaskGraph/hooks/useExecutionTimeline.ts",
                  "LINES: 1-2/206",
                  "TRUNCATED: false",
                  "",
                  "1| const a = 1;",
                  "2|   const b = 2;",
                ].join("\n"),
              },
            ],
          })}
        />,
      );

      expect(screen.getByText(/useExecutionTimeline\.ts/)).toBeInTheDocument();
      expect(screen.getByText("const a = 1;")).toBeInTheDocument();
      expect(screen.getByText("const b = 2;")).toBeInTheDocument();
      expect(screen.getByText("2 lines")).toBeInTheDocument();
      expect(screen.queryByText(/FILE:/)).not.toBeInTheDocument();
      expect(screen.queryByText(/LINES:/)).not.toBeInTheDocument();
      expect(screen.queryByText(/TRUNCATED:/)).not.toBeInTheDocument();
    });

    it("strips the unambiguous fs_read_file line range header", () => {
      render(
        <ReadWidget
          toolCall={makeReadCall({
            arguments: {
              path: "frontend/src/components/AgentsSidebar.tsx",
              start_line: 2699,
              end_line: 2730,
            },
            result: [
              {
                type: "text",
                text: [
                  "FILE: /workspace/project/frontend/src/components/AgentsSidebar.tsx",
                  "LINES: 2699-2730 of 4775",
                  "TRUNCATED: false",
                  "",
                  "2699| const first = true;",
                  "2700| const second = true;",
                ].join("\n"),
              },
            ],
          })}
        />,
      );

      expect(screen.getByText("const first = true;")).toBeInTheDocument();
      expect(screen.queryByText(/LINES:/)).not.toBeInTheDocument();
    });

    it("parses persisted full MCP wrapper results", () => {
      render(
        <ReadWidget
          toolCall={makeReadCall({
            arguments: { path: "CLAUDE.md" },
            result: {
              content: [
                {
                  type: "text",
                  text: [
                    "FILE: /workspace/project/CLAUDE.md",
                    "LINES: 1-2/2",
                    "TRUNCATED: false",
                    "",
                    "1| # Project Rules",
                    "2| Keep prompts clean.",
                  ].join("\n"),
                },
              ],
              structured_content: null,
            },
          })}
        />,
      );

      expect(screen.getByText("# Project Rules")).toBeInTheDocument();
      expect(screen.getByText("Keep prompts clean.")).toBeInTheDocument();
      expect(screen.queryByText("Reading...")).not.toBeInTheDocument();
    });

    it("surfaces fs_read_file ERROR payloads as widget errors", () => {
      render(
        <ReadWidget
          toolCall={makeReadCall({
            arguments: { path: "frontend/src/missing.ts" },
            result: [{ type: "text", text: "ERROR: ENOENT: no such file or directory" }],
          })}
        />,
      );

      expect(screen.getByText("error")).toBeInTheDocument();
      expect(
        screen.getByText("ERROR: ENOENT: no such file or directory"),
      ).toBeInTheDocument();
    });
  });
});
