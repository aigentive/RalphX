import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AgentComposerSurface } from "./AgentComposerSurface";

type ComposerProps = Parameters<typeof AgentComposerSurface>[0];

function renderComposer(overrides: Partial<ComposerProps> = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
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
          value: "edit",
          onValueChange: vi.fn(),
          options: [{ id: "edit", label: "Agent" }],
        }}
        onSend={vi.fn()}
        actionTestId="agent-composer-submit"
        {...overrides}
      />
    </QueryClientProvider>
  );
}

describe("AgentComposerSurface", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({ skills: [] });
      }
      if (cmd === "search_agent_composer_entries") {
        return Promise.resolve({ entries: [], truncated: false });
      }
      return Promise.resolve(undefined);
    });
  });

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

  it("shows trigger hints in the helper text", () => {
    renderComposer();

    expect(screen.getByText("Type / for commands")).toBeInTheDocument();
    expect(screen.getByText("@ for files")).toBeInTheDocument();
    expect(screen.getByText("$ for skills")).toBeInTheDocument();
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

  it("runs slash mode commands from the composer menu", async () => {
    const onValueChange = vi.fn();
    renderComposer({
      mode: {
        value: "edit",
        onValueChange,
        options: [
          { id: "edit", label: "Agent" },
          { id: "chat", label: "Chat" },
        ],
      },
    });

    const textarea = screen.getByLabelText("Message input") as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/ch" } });
    textarea.setSelectionRange(3, 3);
    fireEvent.keyUp(textarea);
    await screen.findByTestId("agent-composer-menu-item-command:mode:chat");
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onValueChange).toHaveBeenCalledWith("chat");
    expect(textarea.value).toBe("");
  });

  it("bounds slash command suggestions to five visible rows", async () => {
    renderComposer({
      mode: {
        value: "edit",
        onValueChange: vi.fn(),
        options: [
          { id: "edit", label: "Agent" },
          { id: "chat", label: "Chat" },
          { id: "ideation", label: "Ideation" },
          { id: "review", label: "Review" },
          { id: "debug", label: "Debug" },
          { id: "plan", label: "Plan" },
        ],
      },
    });

    const textarea = screen.getByLabelText("Message input") as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/" } });
    textarea.setSelectionRange(1, 1);
    fireEvent.keyUp(textarea);

    const scrollRegion = await screen.findByTestId(
      "agent-composer-command-menu-scroll",
    );
    expect(scrollRegion).toHaveStyle({ maxHeight: "260px" });
    expect(scrollRegion).toHaveClass("overflow-y-auto");
  });

  it("opens initial path suggestions for a bare @ trigger", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "search_agent_composer_entries") {
        return Promise.resolve({
          entries: [{ path: "src/main.ts", kind: "file", parentPath: "src" }],
          truncated: false,
        });
      }
      return Promise.resolve({ skills: [] });
    });
    renderComposer();

    const textarea = screen.getByLabelText("Message input") as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Open @" } });
    textarea.setSelectionRange("Open @".length, "Open @".length);
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId(
      "agent-composer-menu-item-path:src/main.ts",
    );
    fireEvent.mouseDown(item);
    fireEvent.click(item);

    expect(textarea.value).toBe("Open @src/main.ts ");
  });

  it("sends selected @ paths as structured project references", async () => {
    const onSend = vi.fn();
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "search_agent_composer_entries") {
        return Promise.resolve({
          entries: [{ path: "src/main.ts", kind: "file", parentPath: "src" }],
          truncated: false,
        });
      }
      return Promise.resolve({ skills: [] });
    });
    renderComposer({ onSend });

    const textarea = screen.getByLabelText("Message input") as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Read @" } });
    textarea.setSelectionRange("Read @".length, "Read @".length);
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId(
      "agent-composer-menu-item-path:src/main.ts",
    );
    fireEvent.mouseDown(item);
    fireEvent.click(item);
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith(
      "Read @src/main.ts",
      {
        projectReferences: [{ path: "src/main.ts", kind: "file" }],
      },
    );
  });

  it("appends internal skill directives for selected $ skills", async () => {
    const onSend = vi.fn();
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({
          skills: [
            {
              id: "internal:workspace-swe",
              name: "workspace-swe",
              displayName: null,
              description: "Workspace skill",
              source: "ralphx-internal",
              providerHarness: null,
              scope: "RalphX",
              invocationKind: "internal-directive",
              invocationValue: "workspace-swe",
              enabled: true,
              sourcePath: "plugins/app/skills/workspace-swe/SKILL.md",
            },
          ],
        });
      }
      return Promise.resolve({ entries: [], truncated: false });
    });
    renderComposer({ onSend });

    const textarea = screen.getByLabelText("Message input") as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Use $work" } });
    textarea.setSelectionRange("Use $work".length, "Use $work".length);
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId(
      "agent-composer-menu-item-skill:internal:workspace-swe",
    );
    fireEvent.mouseDown(item);
    fireEvent.click(item);
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith(
      "Use $workspace-swe\n\n<!-- ralphx_internal_skill=workspace-swe -->",
    );
  });

  it("uses provider-native invocation values for selected harness skills", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({
          skills: [
            {
              id: "claude:project:review",
              name: "review",
              displayName: null,
              description: "Claude project review skill.",
              source: "harness-native",
              providerHarness: "claude",
              scope: "project",
              invocationKind: "harness-native-token",
              invocationValue: "/review",
              enabled: true,
              sourcePath: ".claude/skills/review/SKILL.md",
            },
          ],
        });
      }
      return Promise.resolve({ entries: [], truncated: false });
    });
    renderComposer({
      provider: {
        value: "claude",
        onValueChange: vi.fn(),
        options: [{ id: "claude", label: "Claude" }],
      },
    });

    const textarea = screen.getByLabelText("Message input") as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Use $rev" } });
    textarea.setSelectionRange("Use $rev".length, "Use $rev".length);
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId(
      "agent-composer-menu-item-skill:claude:project:review",
    );
    fireEvent.mouseDown(item);
    fireEvent.click(item);

    expect(textarea.value).toBe("Use /review ");
  });

  it("includes provider-native slash skills in the slash command menu", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({
          skills: [
            {
              id: "claude:project:review",
              name: "review",
              displayName: null,
              description: "Claude project review skill.",
              source: "harness-native",
              providerHarness: "claude",
              scope: "project",
              invocationKind: "harness-native-token",
              invocationValue: "/review",
              enabled: true,
              sourcePath: ".claude/skills/review/SKILL.md",
            },
          ],
        });
      }
      return Promise.resolve({ entries: [], truncated: false });
    });
    renderComposer({
      provider: {
        value: "claude",
        onValueChange: vi.fn(),
        options: [{ id: "claude", label: "Claude" }],
      },
    });

    const textarea = screen.getByLabelText("Message input") as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/rev" } });
    textarea.setSelectionRange("/rev".length, "/rev".length);
    fireEvent.keyUp(textarea);

    await screen.findByTestId("agent-composer-menu-item-command:skill:claude:project:review");
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(textarea.value).toBe("/review ");
  });
});
