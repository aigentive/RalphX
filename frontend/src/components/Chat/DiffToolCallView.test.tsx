import { render, screen, waitFor } from "@testing-library/react";
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
