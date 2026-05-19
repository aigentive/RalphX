import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AgentComposerSurface } from "./AgentComposerSurface";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: () => Promise.resolve(() => {}),
  }),
}));

vi.mock("@tauri-apps/plugin-fs", () => ({
  readFile: vi.fn(),
  stat: vi.fn(),
}));

type ComposerProps = Parameters<typeof AgentComposerSurface>[0];

function renderComposer(overrides: Partial<ComposerProps> = {}) {
  return render(
    <AgentComposerSurface
      project={{
        value: "project-1",
        onValueChange: vi.fn(),
        options: [{ id: "project-1", label: "RalphX" }],
        placeholder: "Project",
      }}
      provider={{
        value: "codex",
        onValueChange: vi.fn(),
        options: [{ id: "codex", label: "Codex" }],
      }}
      model={{
        value: "gpt-5.5",
        onValueChange: vi.fn(),
        options: [{ id: "gpt-5.5", label: "gpt-5.5 (Current)" }],
      }}
      effort={{
        value: "xhigh",
        onValueChange: vi.fn(),
        options: [{ id: "xhigh", label: "Extra High" }],
      }}
      mode={{
        value: "agent",
        onValueChange: vi.fn(),
        options: [{ id: "agent", label: "Agent" }],
      }}
      onSend={vi.fn()}
      actionTestId="agent-composer-submit"
      {...overrides}
    />
  );
}

function makeDropEvent(files: File[]) {
  return {
    dataTransfer: {
      files,
      items: files.map((file) => ({
        kind: "file",
        type: file.type,
        getAsFile: () => file,
      })),
      types: ["Files"],
      dropEffect: "none",
    },
  };
}

describe("AgentComposerSurface", () => {
  it("keeps the runtime selector content-sized instead of filling the footer row", () => {
    renderComposer();

    expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveClass(
      "max-w-[34rem]"
    );
    expect(screen.getByTestId("agent-composer-runtime-pill")).not.toHaveClass("flex-1");
    expect(screen.getByTestId("agent-composer-submit")).toHaveClass("ml-auto");
  });

  it("refreshes mode state when the mode menu opens", () => {
    const onOpen = vi.fn();
    renderComposer({
      mode: {
        value: "ideation",
        onOpen,
        onValueChange: vi.fn(),
        options: [{ id: "ideation", label: "Ideation" }],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-mode-chip"));

    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("shows disabled mode option reasons without firing the change handler", () => {
    const onValueChange = vi.fn();
    renderComposer({
      mode: {
        value: "ideation",
        onValueChange,
        options: [
          { id: "ideation", label: "Ideation" },
          {
            id: "chat",
            label: "Chat",
            disabled: true,
            disabledReason: "Plan execution is still active",
          },
        ],
        testId: "agent-mode",
      },
    });

    fireEvent.click(screen.getByTestId("agent-mode-chip"));
    const chatOption = screen.getByTestId("agent-mode-chat");
    fireEvent.click(chatOption);

    expect(chatOption).toBeDisabled();
    expect(screen.getByText("Plan execution is still active")).toBeInTheDocument();
    expect(onValueChange).not.toHaveBeenCalled();
  });

  it("accepts dropped files across the whole composer surface", async () => {
    const onFilesSelected = vi.fn();
    renderComposer({
      dataTestId: "agent-composer",
      enableAttachments: true,
      onFilesSelected,
    });
    const file = new File(["content"], "notes.md", { type: "text/markdown" });
    const composer = screen.getByTestId("agent-composer");

    fireEvent.dragEnter(composer, makeDropEvent([file]));

    expect(screen.getByTestId("chat-composer-drop-overlay")).toBeInTheDocument();

    fireEvent.drop(composer, makeDropEvent([file]));

    await waitFor(() => {
      expect(onFilesSelected).toHaveBeenCalledWith([file]);
    });
    expect(screen.queryByTestId("chat-composer-drop-overlay")).not.toBeInTheDocument();
  });

  it("does not accept dropped files when attachments are disabled", () => {
    const onFilesSelected = vi.fn();
    renderComposer({
      dataTestId: "agent-composer",
      enableAttachments: false,
      onFilesSelected,
    });
    const file = new File(["content"], "notes.md", { type: "text/markdown" });

    fireEvent.drop(screen.getByTestId("agent-composer"), makeDropEvent([file]));

    expect(onFilesSelected).not.toHaveBeenCalled();
    expect(screen.queryByTestId("chat-composer-drop-overlay")).not.toBeInTheDocument();
  });
});
