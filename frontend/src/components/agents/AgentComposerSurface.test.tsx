import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@radix-ui/react-tooltip";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { setRalphxTerminalDockDragActive } from "@/lib/internalDragTypes";
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
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
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
          options: [{ id: "gpt-5.5", label: "gpt-5.5" }],
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
      </TooltipProvider>
    </QueryClientProvider>
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

function makeTerminalDragEvent() {
  const file = new File(["content"], "terminal-drag.txt", { type: "text/plain" });
  return {
    dataTransfer: {
      files: [file],
      items: [
        {
          kind: "file",
          type: file.type,
          getAsFile: () => file,
        },
      ],
      types: ["application/x-ralphx-terminal-dock", "Files"],
      dropEffect: "none",
    },
  };
}

describe("AgentComposerSurface", () => {
  beforeEach(() => {
    setRalphxTerminalDockDragActive(false);
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({ skills: [] });
      }
      if (cmd === "search_agent_composer_entries") {
        return Promise.resolve({ entries: [], truncated: false });
      }
      if (cmd === "search_atlassian_resources") {
        return Promise.resolve({ resources: [] });
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

  it("keeps Send as the primary action while the agent is waiting for input", () => {
    const onStop = vi.fn();
    renderComposer({
      agentStatus: "waiting_for_input",
      onStop,
    });

    const action = screen.getByTestId("agent-composer-submit");
    expect(action).toHaveAccessibleName("Send");
    expect(action).toHaveTextContent("Send");
    expect(action).not.toHaveTextContent("Stop");
    expect(action).toBeDisabled();

    fireEvent.click(action);
    expect(onStop).not.toHaveBeenCalled();
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
    expect(screen.getByText("@ for references")).toBeInTheDocument();
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

  it("runs custom slash commands from the composer menu", async () => {
    const onFork = vi.fn();
    renderComposer({
      slashCommands: [
        {
          id: "fork",
          label: "/fork",
          description: "Fork this agent conversation",
          onSelect: onFork,
        },
      ],
    });

    const textarea = screen.getByLabelText("Message input") as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/fo" } });
    textarea.setSelectionRange(3, 3);
    fireEvent.keyUp(textarea);
    await screen.findByTestId("agent-composer-menu-item-command:custom:fork");
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => expect(onFork).toHaveBeenCalledTimes(1));
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

    expect(textarea.value).toBe("Open ");
    expect(
      screen.getByTestId("agent-composer-reference-pill-project:src/main.ts"),
    ).toHaveTextContent("File");
    expect(
      screen.getByTestId("agent-composer-reference-pill-project:src/main.ts"),
    ).toHaveTextContent("src/main.ts");
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
    expect(textarea).toHaveValue("Read ");
    expect(
      screen.getByTestId("agent-composer-reference-pill-project:src/main.ts"),
    ).toHaveTextContent("File");
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith(
      "Read",
      {
        projectReferences: [{ path: "src/main.ts", kind: "file" }],
      },
    );
  });

  it("removes selected project reference pills before sending", async () => {
    const onSend = vi.fn();
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "search_agent_composer_entries") {
        return Promise.resolve({
          entries: [{ path: "src", kind: "directory", parentPath: null }],
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

    const item = await screen.findByTestId("agent-composer-menu-item-path:src");
    fireEvent.mouseDown(item);
    fireEvent.click(item);

    expect(screen.getByTestId("agent-composer-reference-pill-project:src")).toHaveTextContent(
      "Folder",
    );
    fireEvent.click(screen.getByLabelText("Remove folder reference src"));
    expect(screen.queryByTestId("agent-composer-reference-pill-project:src")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith("Read");
  });

  it("sends selected Jira items as structured integration references", async () => {
    const onSend = vi.fn();
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "search_atlassian_resources") {
        return Promise.resolve({
          resources: [
            {
              kind: "jira",
              id: "RX-42",
              key: "RX-42",
              title: "Fix composer search",
              url: "https://example.atlassian.net/browse/RX-42",
              excerpt: null,
            },
          ],
        });
      }
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({ skills: [] });
      }
      return Promise.resolve({ entries: [], truncated: false });
    });
    renderComposer({ onSend });

    const textarea = screen.getByLabelText("Message input") as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Work on @jira:RX" } });
    textarea.setSelectionRange("Work on @jira:RX".length, "Work on @jira:RX".length);
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId(
      "agent-composer-menu-item-integration:jira:RX-42",
    );
    fireEvent.mouseDown(item);
    fireEvent.click(item);
    expect(textarea).toHaveValue("Work on ");
    expect(
      screen.getByTestId("agent-composer-reference-pill-integration:jira:RX-42"),
    ).toHaveTextContent("Jira");
    expect(
      screen.getByTestId("agent-composer-reference-pill-integration:jira:RX-42"),
    ).toHaveTextContent("RX-42");
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith("Work on", {
      integrationReferences: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "RX-42",
          key: "RX-42",
          title: "Fix composer search",
          url: "https://example.atlassian.net/browse/RX-42",
        },
      ],
    });
  });

  it.each([
    ["Jira", "@jira:", "jira"],
    ["Confluence", "@confluence:", "confluence"],
  ])("inserts %s triggers from the plus menu and opens search", async (label, expectedValue, kind) => {
    renderComposer();

    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    fireEvent.click(screen.getByText(label));

    const textarea = screen.getByLabelText("Message input");
    expect(textarea).toHaveValue(expectedValue);
    await waitFor(() => expect(textarea).toHaveFocus());
    expect(await screen.findByTestId("agent-composer-command-menu")).toBeInTheDocument();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("search_atlassian_resources", {
        input: { kind, query: "", limit: 12 },
      }),
    );
  });

  it("runs fork session from the plus menu", async () => {
    const onForkSession = vi.fn().mockResolvedValue(undefined);
    renderComposer({ onForkSession });

    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    fireEvent.click(screen.getByText("Fork session"));

    await waitFor(() => expect(onForkSession).toHaveBeenCalledTimes(1));
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

  it("ignores terminal panel drags even when the drag event advertises file types", () => {
    const onFilesSelected = vi.fn();
    renderComposer({
      dataTestId: "agent-composer",
      enableAttachments: true,
      onFilesSelected,
    });
    const composer = screen.getByTestId("agent-composer");

    fireEvent.dragEnter(composer, makeTerminalDragEvent());
    fireEvent.dragOver(composer, makeTerminalDragEvent());
    fireEvent.drop(composer, makeTerminalDragEvent());

    expect(screen.queryByTestId("chat-composer-drop-overlay")).not.toBeInTheDocument();
    expect(onFilesSelected).not.toHaveBeenCalled();
  });

  it("ignores active terminal panel drags when WebKit only reports file types", () => {
    const onFilesSelected = vi.fn();
    const file = new File(["content"], "terminal-drag.txt", { type: "text/plain" });
    setRalphxTerminalDockDragActive(true);
    renderComposer({
      dataTestId: "agent-composer",
      enableAttachments: true,
      onFilesSelected,
    });
    const composer = screen.getByTestId("agent-composer");

    fireEvent.dragEnter(composer, makeDropEvent([file]));
    fireEvent.dragOver(composer, makeDropEvent([file]));
    fireEvent.drop(composer, makeDropEvent([file]));

    expect(screen.queryByTestId("chat-composer-drop-overlay")).not.toBeInTheDocument();
    expect(onFilesSelected).not.toHaveBeenCalled();
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
