import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import {
  MessageFileLinkContext,
  type MessageFileLinkContextValue,
} from "./MessageFileLinkContext";
import { DiffToolCallView } from "./DiffToolCallView";
import type { ToolCall } from "./ToolCallIndicator";

function makeEditCall(filePath: string): ToolCall {
  return {
    id: "tool-edit-1",
    name: "edit",
    arguments: {
      file_path: filePath,
      old_string: "old\n",
      new_string: "new\n",
    },
    result: { status: "completed" },
  };
}

function makeMultiHunkEditCall(): ToolCall {
  return {
    id: "tool-edit-multi",
    name: "edit",
    arguments: {
      file_path: "/tmp/ralphx/worktrees/conversation-1/src/example.ts",
      old_string: [
        "line 1",
        "line 2",
        "line 3",
        "line 4",
        "line 5",
        "line 6",
        "line 7",
        "line 8",
        "line 9",
        "line 10",
        "line 11",
        "line 12",
      ].join("\n"),
      new_string: [
        "line 1",
        "line 2 changed",
        "line 3",
        "line 4",
        "line 5",
        "line 6",
        "line 7",
        "line 8",
        "line 9",
        "line 10 changed",
        "line 11",
        "line 12",
      ].join("\n"),
    },
    result: { status: "completed" },
  };
}

function makeWhitespaceEditCall(): ToolCall {
  return {
    id: "tool-edit-whitespace",
    name: "edit",
    arguments: {
      file_path: "/tmp/ralphx/worktrees/conversation-1/src/whitespace.ts",
      old_string: [
        "function run() {",
        "  if (ready) {",
        "    return oldValue;",
        "  }",
        "}",
      ].join("\n"),
      new_string: [
        "function run() {",
        "  if (ready) {",
        "    return value;",
        "  }",
        "}",
      ].join("\n"),
    },
    result: { status: "completed" },
  };
}

function makeWriteCallWithoutBaseline(): ToolCall {
  return {
    id: "tool-write-1",
    name: "write",
    arguments: {
      file_path: "/tmp/outside/generated.txt",
      content: "first line\nsecond line\nthird line",
    },
    result: { status: "completed" },
  };
}

function makeNewFileWriteCall(): ToolCall {
  return {
    id: "tool-write-new-file",
    name: "write",
    arguments: {
      file_path: "/tmp/outside/new-file.txt",
      content: "first line\nsecond line",
    },
    diffContext: {
      filePath: "/tmp/outside/new-file.txt",
      oldFileExists: false,
    },
    result: { status: "completed" },
  };
}

function renderDiff(
  filePath: string,
  contextOverrides: Partial<MessageFileLinkContextValue> = {}
) {
  const context: MessageFileLinkContextValue = {
    workspaceRootPath: "/tmp/ralphx/worktrees/conversation-1",
    targets: [],
    preferredTargetId: null,
    openingPath: null,
    openingTargetId: null,
    onOpenPath: vi.fn(),
    ...contextOverrides,
  };

  return render(
    <TooltipProvider delayDuration={0}>
      <MessageFileLinkContext.Provider value={context}>
        <DiffToolCallView toolCall={makeEditCall(filePath)} />
      </MessageFileLinkContext.Provider>
    </TooltipProvider>
  );
}

describe("DiffToolCallView path header", () => {
  it("shows repo-relative paths for files inside the workspace root", () => {
    renderDiff(
      "/tmp/ralphx/worktrees/conversation-1/frontend/src/components/Chat/DiffToolCallView.tsx"
    );

    expect(screen.getByTestId("diff-tool-call-file-path")).toHaveTextContent(
      "frontend/src/components/Chat/DiffToolCallView.tsx"
    );
    expect(
      screen.queryByText(".../components/Chat/DiffToolCallView.tsx")
    ).not.toBeInTheDocument();
  });

  it("keeps full paths for files outside the workspace root", () => {
    renderDiff("/tmp/outside/frontend/src/components/Chat/DiffToolCallView.tsx");

    expect(screen.getByTestId("diff-tool-call-file-path")).toHaveTextContent(
      "/tmp/outside/frontend/src/components/Chat/DiffToolCallView.tsx"
    );
  });

  it("shows the full supplied path in the path tooltip", async () => {
    const user = userEvent.setup();
    const fullPath =
      "/tmp/ralphx/worktrees/conversation-1/frontend/src/components/Chat/DiffToolCallView.tsx";
    renderDiff(fullPath);

    await user.hover(screen.getByTestId("diff-tool-call-file-path"));

    expect(await screen.findByRole("tooltip")).toHaveTextContent(fullPath);
  });

  it("copies the full supplied path from the copy button", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const fullPath =
      "/tmp/ralphx/worktrees/conversation-1/frontend/src/components/Chat/DiffToolCallView.tsx";
    renderDiff(fullPath);

    await user.click(screen.getByRole("button", { name: "Copy file path" }));

    expect(writeText).toHaveBeenCalledWith(fullPath);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument()
    );
  });
});

describe("DiffToolCallView hunk rendering", () => {
  it("shows only the first hunk preview while collapsed", () => {
    render(
      <TooltipProvider delayDuration={0}>
        <DiffToolCallView toolCall={makeMultiHunkEditCall()} />
      </TooltipProvider>
    );

    expect(screen.getByTestId("diff-tool-call-preview-diff")).toBeInTheDocument();
    expect(screen.getByText("line 2 changed")).toBeInTheDocument();
    expect(screen.queryByText("line 10 changed")).not.toBeInTheDocument();
    expect(screen.queryByTestId("diff-tool-call-full-diff")).not.toBeInTheDocument();
  });

  it("hydrates the full hunk diff only after expansion", async () => {
    const user = userEvent.setup();
    render(
      <TooltipProvider delayDuration={0}>
        <DiffToolCallView toolCall={makeMultiHunkEditCall()} />
      </TooltipProvider>
    );

    expect(screen.queryByText("line 10 changed")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /edit .* click to expand/i }));

    const fullDiff = await screen.findByTestId("diff-tool-call-full-diff");
    expect(fullDiff).toBeInTheDocument();
    expect(fullDiff.querySelector('[data-density="compact"]')).toHaveClass(
      "text-[0.6875rem]",
      "leading-[18px]"
    );
    expect(fullDiff.querySelector("[data-wrap-lines='false']")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /wrap/i })).not.toBeInTheDocument();
    expect(screen.queryByText(/unchanged lines/i)).not.toBeInTheDocument();
    expect(await screen.findByText("line 10 changed")).toBeInTheDocument();
  });

  it("preserves leading whitespace consistently in preview and full diff rows", async () => {
    const user = userEvent.setup();
    render(
      <TooltipProvider delayDuration={0}>
        <DiffToolCallView toolCall={makeWhitespaceEditCall()} />
      </TooltipProvider>
    );

    const exactIndentedLine = (_content: string, element: Element | null) =>
      element?.textContent === "    return value;";

    const preview = screen.getByTestId("diff-tool-call-preview-diff");
    expect(within(preview).getByText(exactIndentedLine)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /edit .* click to expand/i }));

    const fullDiff = await screen.findByTestId("diff-tool-call-full-diff");
    expect(within(fullDiff).getByText(exactIndentedLine)).toBeInTheDocument();
  });

  it("labels Write content without a baseline as final file content", () => {
    render(
      <TooltipProvider delayDuration={0}>
        <DiffToolCallView toolCall={makeWriteCallWithoutBaseline()} />
      </TooltipProvider>
    );

    expect(screen.getByText("Baseline unavailable")).toBeInTheDocument();
    expect(screen.getByTestId("diff-tool-call-final-content")).toHaveTextContent(
      "first line"
    );
    expect(screen.queryByTestId("diff-tool-call-preview-diff")).not.toBeInTheDocument();
  });

  it("labels confirmed new-file Write calls as added-line diffs", () => {
    render(
      <TooltipProvider delayDuration={0}>
        <DiffToolCallView toolCall={makeNewFileWriteCall()} />
      </TooltipProvider>
    );

    expect(screen.getByText("New file")).toBeInTheDocument();
    expect(screen.queryByText("Baseline unavailable")).not.toBeInTheDocument();
    expect(screen.getByTestId("diff-tool-call-preview-diff")).toHaveTextContent(
      "first line"
    );
    expect(screen.queryByTestId("diff-tool-call-final-content")).not.toBeInTheDocument();
  });
});
