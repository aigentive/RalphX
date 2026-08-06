import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { GlobWidget } from "./GlobWidget";
import type { ToolCall } from "./shared.constants";

function makeGlobCall(overrides: Partial<ToolCall> = {}): ToolCall {
  return {
    id: "glob-1",
    name: "Glob",
    arguments: { pattern: "**/*.ts", path: "src/" },
    result: "src/app.ts\nsrc/utils.ts",
    ...overrides,
  };
}

describe("GlobWidget", () => {
  describe("rendering", () => {
    it("shows glob pattern in title with path", () => {
      render(<GlobWidget toolCall={makeGlobCall()} />);
      expect(screen.getByText("**/*.ts in src/")).toBeInTheDocument();
    });

    it("shows glob pattern only when no path", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({ arguments: { pattern: "*.json" } })}
        />
      );
      expect(screen.getByText("*.json")).toBeInTheDocument();
    });

    it("uses fs_glob aliases from MCP arguments", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({
            arguments: { file_pattern: "**/*.rs", base_path: "src-tauri/" },
          })}
        />,
      );
      expect(screen.getByText("**/*.rs in src-tauri/")).toBeInTheDocument();
    });

    it("shows match count badge", () => {
      render(<GlobWidget toolCall={makeGlobCall()} />);
      expect(screen.getByText("2 matches")).toBeInTheDocument();
    });

    it("shows singular match count", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({ result: "src/app.ts" })}
        />
      );
      expect(screen.getByText("1 match")).toBeInTheDocument();
    });

    it("shows matched file paths in body", () => {
      render(<GlobWidget toolCall={makeGlobCall()} />);
      expect(screen.getByText("src/app.ts")).toBeInTheDocument();
      expect(screen.getByText("src/utils.ts")).toBeInTheDocument();
    });
  });

  describe("inline vs collapsible", () => {
    it("renders inline (no chevron) for <=3 results", () => {
      const call = makeGlobCall({ result: "a.ts\nb.ts\nc.ts" });
      const { container } = render(<GlobWidget toolCall={call} />);
      expect(screen.getByText("a.ts")).toBeInTheDocument();
      expect(screen.getByText("b.ts")).toBeInTheDocument();
      expect(screen.getByText("c.ts")).toBeInTheDocument();
      const chevrons = container.querySelectorAll('[style*="rotate"]');
      expect(chevrons.length).toBe(0);
    });

    it("renders collapsible (with chevron) for >3 results", () => {
      const call = makeGlobCall({
        result: "a.ts\nb.ts\nc.ts\nd.ts",
      });
      const { container } = render(<GlobWidget toolCall={call} />);
      expect(screen.getByText("4 matches")).toBeInTheDocument();
      const chevrons = container.querySelectorAll('[style*="rotate"]');
      expect(chevrons.length).toBeGreaterThan(0);
    });
  });

  describe("pending state", () => {
    it("shows 'Searching...' when result is undefined", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({ result: undefined })}
        />
      );
      expect(screen.getByText("Searching...")).toBeInTheDocument();
      expect(screen.getByText("no matches")).toBeInTheDocument();
    });
  });

  describe("empty results", () => {
    it("shows 'No files matched' when result is empty string", () => {
      render(
        <GlobWidget toolCall={makeGlobCall({ result: "" })} />
      );
      expect(screen.getByText("No files matched")).toBeInTheDocument();
      expect(screen.getByText("no matches")).toBeInTheDocument();
    });
  });

  describe("result parsing", () => {
    it("parses plain string result", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({ result: "file1.ts\nfile2.ts" })}
        />
      );
      expect(screen.getByText("file1.ts")).toBeInTheDocument();
      expect(screen.getByText("file2.ts")).toBeInTheDocument();
    });

    it("parses MCP wrapper result", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({
            result: [{ type: "text", text: "mcp-file.ts\nmcp-other.ts" }],
          })}
        />
      );
      expect(screen.getByText("mcp-file.ts")).toBeInTheDocument();
      expect(screen.getByText("mcp-other.ts")).toBeInTheDocument();
    });

    it("parses persisted full MCP wrapper results", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({
            result: {
              content: [
                {
                  type: "text",
                  text: [
                    "ROOT: /workspace/project",
                    "PATTERN: **/*.rs",
                    "MATCHES: 2",
                    "",
                    "src-tauri/src/lib.rs",
                    "src-tauri/src/main.rs",
                  ].join("\n"),
                },
              ],
              structured_content: null,
            },
          })}
        />
      );
      expect(screen.getByText("src-tauri/src/lib.rs")).toBeInTheDocument();
      expect(screen.getByText("src-tauri/src/main.rs")).toBeInTheDocument();
    });

    it("parses string array result", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({
            result: ["arr-a.ts", "arr-b.ts"],
          })}
        />
      );
      expect(screen.getByText("arr-a.ts")).toBeInTheDocument();
      expect(screen.getByText("arr-b.ts")).toBeInTheDocument();
    });

    it("trims whitespace and filters empty lines", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({
            result: "  spaced.ts  \n\n  trimmed.ts  \n",
          })}
        />
      );
      expect(screen.getByText("spaced.ts")).toBeInTheDocument();
      expect(screen.getByText("trimmed.ts")).toBeInTheDocument();
    });
  });

  describe("parseSearchResult integration", () => {
    it("deduplicates repeated file paths", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({
            result: "src/app.ts\nsrc/app.ts\nsrc/utils.ts",
          })}
        />,
      );
      expect(screen.getByText("2 matches")).toBeInTheDocument();
    });

    it("skips metadata lines", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({
            result: "Found 2 files\nsrc/app.ts\nsrc/utils.ts",
          })}
        />,
      );
      expect(screen.getByText("2 matches")).toBeInTheDocument();
    });

    it("normalizes absolute paths to repo-relative", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({
            result: "/Users/dev/project/src/app.ts\n/Users/dev/project/tests/test.ts",
          })}
        />,
      );
      expect(screen.getByText("src/app.ts")).toBeInTheDocument();
      expect(screen.getByText("tests/test.ts")).toBeInTheDocument();
    });

    it("renders no-match note from search result", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({ result: "No files matched" })}
        />,
      );
      expect(screen.getByText("No files matched")).toBeInTheDocument();
      expect(screen.getByText("no matches")).toBeInTheDocument();
    });

    it("renders a zero-match exclusion diagnostic as the empty state", () => {
      render(
        <GlobWidget
          toolCall={makeGlobCall({
            result: [
              "ROOT: /workspace/project",
              "MATCHES: 0",
              "NOTE: 1 path excluded by .gitignore (node_modules). Set respect_gitignore=false to include it.",
            ].join("\n"),
          })}
        />,
      );

      expect(
        screen.getByText(/1 path excluded by \.gitignore \(node_modules\)/),
      ).toBeInTheDocument();
      expect(screen.queryByText(/^NOTE:/)).not.toBeInTheDocument();
    });
  });

  describe("compact mode", () => {
    it("passes compact prop without crashing", () => {
      render(<GlobWidget toolCall={makeGlobCall()} compact />);
      expect(screen.getByText(/\*\*\/\*.ts/)).toBeInTheDocument();
    });
  });

  describe("collapse interaction", () => {
    it("toggles body visibility on click for >3 results", async () => {
      const user = userEvent.setup();
      const call = makeGlobCall({
        result: "a.ts\nb.ts\nc.ts\nd.ts\ne.ts",
      });
      render(<GlobWidget toolCall={call} />);

      const toggle = screen.getByRole("button");
      expect(screen.getByText("a.ts")).toBeInTheDocument();

      await user.click(toggle);
      expect(screen.getByText("e.ts")).toBeInTheDocument();
    });
  });
});
