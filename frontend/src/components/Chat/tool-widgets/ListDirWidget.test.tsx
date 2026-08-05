import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ToolCallIndicator } from "../ToolCallIndicator";
import { ListDirWidget } from "./ListDirWidget";
import type { ToolCall } from "./shared.constants";

function makeListDirCall(overrides: Partial<ToolCall> = {}): ToolCall {
  return {
    id: "list-1",
    name: "ralphx::fs_list_dir",
    arguments: { path: ".artifacts/specs/ralphx-cli" },
    result: {
      content: [
        {
          type: "text",
          text: [
            "DIRECTORY: /workspace/project/.artifacts/specs/ralphx-cli",
            "ENTRIES: 2",
            "DIRECTORIES_ONLY: false",
            "INCLUDE_HIDDEN: true",
            "RESPECT_GITIGNORE: false",
            "",
            "DIR  drafts/",
            "FILE cli-surface.md",
          ].join("\n"),
        },
      ],
      structured_content: null,
    },
    ...overrides,
  };
}

describe("ListDirWidget", () => {
  it("renders directory entries from full MCP wrapper results", () => {
    render(<ListDirWidget toolCall={makeListDirCall()} />);

    expect(screen.getByText(".artifacts/specs/ralphx-cli")).toBeInTheDocument();
    expect(screen.getByText("2 entries")).toBeInTheDocument();
    expect(screen.getByText("drafts/")).toBeInTheDocument();
    expect(screen.getByText("cli-surface.md")).toBeInTheDocument();
    expect(screen.queryByText(/^DIRECTORY:/)).not.toBeInTheDocument();
  });

  it("surfaces fs_list_dir error payloads", () => {
    render(
      <ListDirWidget
        toolCall={makeListDirCall({
          result: {
            content: [
              {
                type: "text",
                text: "ERROR: Path \"/outside\" resolves outside the allowed filesystem roots.",
              },
            ],
            structured_content: null,
          },
        })}
      />,
    );

    expect(screen.getByText("error")).toBeInTheDocument();
    expect(screen.getByText(/outside the allowed filesystem roots/)).toBeInTheDocument();
  });

  it("renders pending listings from base_path arguments", () => {
    render(
      <ListDirWidget
        toolCall={makeListDirCall({
          arguments: { base_path: "src-tauri" },
          result: undefined,
        })}
      />,
    );

    expect(screen.getByText("src-tauri")).toBeInTheDocument();
    expect(screen.getByText("Listing...")).toBeInTheDocument();
  });

  it("renders unclassified entries with the default directory label", () => {
    render(
      <ListDirWidget
        toolCall={makeListDirCall({
          arguments: null,
          result: {
            content: [{ type: "text", text: "README.md" }],
            structured_content: null,
          },
        })}
      />,
    );

    expect(screen.getByText("directory")).toBeInTheDocument();
    expect(screen.getByText("README.md")).toBeInTheDocument();
  });

  it("renders exclusion notes without listing them as directory entries", () => {
    render(
      <ListDirWidget
        toolCall={makeListDirCall({
          result: {
            content: [
              {
                type: "text",
                text: [
                  "DIRECTORY: /workspace/project",
                  "ENTRIES: 1",
                  "NOTE: 2 hidden paths skipped. Set include_hidden=true to include them.",
                  "",
                  "FILE visible.ts (12 B)",
                ].join("\n"),
              },
            ],
            structured_content: null,
          },
        })}
      />,
    );

    expect(screen.getByText("visible.ts (12 B)")).toBeInTheDocument();
    expect(
      screen.getByText("2 hidden paths skipped. Set include_hidden=true to include them."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/^NOTE:/)).not.toBeInTheDocument();
  });

  it("routes prefixed fs_list_dir through ToolCallIndicator instead of generic Calling", async () => {
    render(<ToolCallIndicator toolCall={makeListDirCall()} />);

    expect(await screen.findByText("cli-surface.md")).toBeInTheDocument();
    expect(screen.queryByText("Calling")).not.toBeInTheDocument();
  });
});
