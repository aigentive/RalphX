import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { GrepWidget } from "./GrepWidget";
import type { ToolCall } from "./shared.constants";

function makeGrepCall(overrides: Partial<ToolCall> = {}): ToolCall {
  return {
    id: "grep-1",
    name: "Grep",
    arguments: { pattern: "TODO", path: "src/" },
    result: "src/app.ts\nsrc/utils.ts",
    ...overrides,
  };
}

describe("GrepWidget", () => {
  describe("rendering", () => {
    it("shows pattern in title", () => {
      render(<GrepWidget toolCall={makeGrepCall()} />);
      expect(screen.getByText(/"TODO" in src\//)).toBeInTheDocument();
    });

    it("shows pattern only when no path", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({ arguments: { pattern: "fixme" } })}
        />
      );
      expect(screen.getByText(/"fixme"/)).toBeInTheDocument();
    });

    it("shows file count badge", () => {
      render(<GrepWidget toolCall={makeGrepCall()} />);
      expect(screen.getByText("2 files")).toBeInTheDocument();
    });

    it("shows singular file count", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({ result: "src/app.ts" })}
        />
      );
      expect(screen.getByText("1 file")).toBeInTheDocument();
    });

    it("shows file paths in body", () => {
      render(<GrepWidget toolCall={makeGrepCall()} />);
      expect(screen.getByText("src/app.ts")).toBeInTheDocument();
      expect(screen.getByText("src/utils.ts")).toBeInTheDocument();
    });
  });

  describe("inline vs collapsible", () => {
    it("renders inline (no chevron) for <=3 results", () => {
      const call = makeGrepCall({ result: "a.ts\nb.ts\nc.ts" });
      const { container } = render(<GrepWidget toolCall={call} />);
      // alwaysExpanded hides the ChevronRight — verify all 3 files visible
      expect(screen.getByText("a.ts")).toBeInTheDocument();
      expect(screen.getByText("b.ts")).toBeInTheDocument();
      expect(screen.getByText("c.ts")).toBeInTheDocument();
      // No chevron svg when alwaysExpanded
      const chevrons = container.querySelectorAll('[style*="rotate"]');
      expect(chevrons.length).toBe(0);
    });

    it("renders collapsible (with chevron) for >3 results", () => {
      const call = makeGrepCall({
        result: "a.ts\nb.ts\nc.ts\nd.ts",
      });
      const { container } = render(<GrepWidget toolCall={call} />);
      expect(screen.getByText("4 files")).toBeInTheDocument();
      // Should have a chevron for collapse
      const chevrons = container.querySelectorAll('[style*="rotate"]');
      expect(chevrons.length).toBeGreaterThan(0);
    });
  });

  describe("pending state", () => {
    it("shows 'Searching...' when result is undefined", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({ result: undefined })}
        />
      );
      expect(screen.getByText("Searching...")).toBeInTheDocument();
      expect(screen.getByText("no results")).toBeInTheDocument();
    });
  });

  describe("empty results", () => {
    it("shows 'No matches found' when result is empty string", () => {
      render(
        <GrepWidget toolCall={makeGrepCall({ result: "" })} />
      );
      expect(screen.getByText("No matches found")).toBeInTheDocument();
      expect(screen.getByText("no results")).toBeInTheDocument();
    });
  });

  describe("result parsing", () => {
    it("parses plain string result", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({ result: "file1.ts\nfile2.ts" })}
        />
      );
      expect(screen.getByText("file1.ts")).toBeInTheDocument();
      expect(screen.getByText("file2.ts")).toBeInTheDocument();
    });

    it("parses MCP wrapper result", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            result: [{ type: "text", text: "mcp-file.ts\nmcp-other.ts" }],
          })}
        />
      );
      expect(screen.getByText("mcp-file.ts")).toBeInTheDocument();
      expect(screen.getByText("mcp-other.ts")).toBeInTheDocument();
    });

    it("parses string array result", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            result: ["arr-a.ts", "arr-b.ts"],
          })}
        />
      );
      expect(screen.getByText("arr-a.ts")).toBeInTheDocument();
      expect(screen.getByText("arr-b.ts")).toBeInTheDocument();
    });

    it("trims whitespace and filters empty lines", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            result: "  spaced.ts  \n\n  trimmed.ts  \n",
          })}
        />
      );
      expect(screen.getByText("spaced.ts")).toBeInTheDocument();
      expect(screen.getByText("trimmed.ts")).toBeInTheDocument();
    });

    it("parses fs_grep MCP payloads with metadata headers", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            arguments: {
              pattern: "getTimelineEvents",
              file_pattern: "**/api/task-graph.ts",
              max_results: 10,
            },
            result: [
              {
                type: "text",
                text: [
                  "ROOT: /workspace/project",
                  "PATTERN: getTimelineEvents",
                  "FILE_PATTERN: **/api/task-graph.ts",
                  "MATCHES: 1",
                  "INCLUDE_HIDDEN: false",
                  "RESPECT_GITIGNORE: true",
                  "",
                  "frontend/src/api/task-graph.ts:103:   getTimelineEvents: (",
                ].join("\n"),
              },
            ],
          })}
        />,
      );

      expect(screen.getByText(/"getTimelineEvents" in \*\*\/api\/task-graph\.ts/)).toBeInTheDocument();
      expect(screen.getByText("1 file")).toBeInTheDocument();
      expect(screen.getByText("frontend/src/api/task-graph.ts")).toBeInTheDocument();
      expect(screen.queryByText(/^ROOT:/)).not.toBeInTheDocument();
      expect(screen.queryByText(/^PATTERN:/)).not.toBeInTheDocument();
      expect(screen.queryByText(/^MATCHES:/)).not.toBeInTheDocument();
    });

    it("parses persisted full MCP wrapper results", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            result: {
              content: [
                {
                  type: "text",
                  text: [
                    "ROOT: /workspace/project",
                    "PATTERN: workspace",
                    "MATCHES: 1",
                    "",
                    "src-tauri/src/application/chat_service/mod.rs:42:workspace",
                  ].join("\n"),
                },
              ],
              structured_content: null,
            },
          })}
        />,
      );

      expect(screen.getByText("src-tauri/src/application/chat_service/mod.rs")).toBeInTheDocument();
    });
  });

  describe("parseSearchResult integration", () => {
    it("deduplicates paths from grep content output", () => {
      // Grep content mode: path:line:match — same file multiple times
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            result: "src/app.ts:10:import React\nsrc/app.ts:20:export default\nsrc/utils.ts:5:export function",
          })}
        />,
      );
      expect(screen.getByText("2 files")).toBeInTheDocument();
      expect(screen.getByText("src/app.ts")).toBeInTheDocument();
      expect(screen.getByText("src/utils.ts")).toBeInTheDocument();
    });

    it("skips metadata lines like 'Found N files'", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            result: "Found 2 files\nsrc/app.ts\nsrc/utils.ts",
          })}
        />,
      );
      expect(screen.getByText("2 files")).toBeInTheDocument();
    });

    it("normalizes absolute paths to repo-relative", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            result: "/Users/dev/project/src/app.ts\n/Users/dev/project/src/utils.ts",
          })}
        />,
      );
      expect(screen.getByText("src/app.ts")).toBeInTheDocument();
      expect(screen.getByText("src/utils.ts")).toBeInTheDocument();
    });

    it("renders no-match note from search result", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({ result: "No matches found" })}
        />,
      );
      expect(screen.getByText("No matches found")).toBeInTheDocument();
      expect(screen.getByText("no results")).toBeInTheDocument();
    });

    it("surfaces fs_grep ERROR payloads as notes instead of fake file paths", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            result: [{ type: "text", text: "ERROR: ENOENT: no such file or directory, realpath '/workspace/project/.'" }],
          })}
        />,
      );

      expect(
        screen.getByText(/ERROR: ENOENT: no such file or directory/),
      ).toBeInTheDocument();
      expect(screen.getByText("no results")).toBeInTheDocument();
      expect(screen.queryByText(/realpath/)).toBeInTheDocument();
    });

    it("renders diagnostics below non-empty results instead of as file paths", () => {
      render(
        <GrepWidget
          toolCall={makeGrepCall({
            result: [
              "ROOT: /workspace/project",
              "MATCHES: 1",
              "NOTE: result cap reached (max_results=1); more matches exist.",
              "src/app.ts:10: needle",
            ].join("\n"),
          })}
        />,
      );

      expect(screen.getByText("src/app.ts")).toBeInTheDocument();
      expect(
        screen.getByText("result cap reached (max_results=1); more matches exist."),
      ).toBeInTheDocument();
      expect(screen.queryByText(/^NOTE:/)).not.toBeInTheDocument();
    });
  });

  describe("compact mode", () => {
    it("passes compact prop without crashing", () => {
      render(<GrepWidget toolCall={makeGrepCall()} compact />);
      expect(screen.getByText(/"TODO"/)).toBeInTheDocument();
    });
  });

  describe("collapse interaction", () => {
    it("toggles body visibility on click for >3 results", async () => {
      const user = userEvent.setup();
      const call = makeGrepCall({
        result: "a.ts\nb.ts\nc.ts\nd.ts\ne.ts",
      });
      render(<GrepWidget toolCall={call} />);

      // The widget card header is clickable — find the role=button
      const toggle = screen.getByRole("button");
      // Initially collapsed — body is present but max-height limited
      expect(screen.getByText("a.ts")).toBeInTheDocument();

      // Click to expand
      await user.click(toggle);
      // All files still visible
      expect(screen.getByText("e.ts")).toBeInTheDocument();
    });
  });
});
