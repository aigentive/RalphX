import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@radix-ui/react-tooltip";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setRalphxTerminalDockDragActive } from "@/lib/internalDragTypes";
import {
  AgentComposerProjectLine,
  AgentComposerSurface,
} from "./AgentComposerSurface";

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
    </QueryClientProvider>,
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
  const file = new File(["content"], "terminal-drag.txt", {
    type: "text/plain",
  });
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
    vi.useRealTimers();
    setRalphxTerminalDockDragActive(false);
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({ skills: [] });
      }
      if (cmd === "search_agent_composer_entries") {
        return Promise.resolve({ entries: [], truncated: false });
      }
      if (cmd === "search_agent_composer_plan_references") {
        return Promise.resolve({ plans: [], truncated: false });
      }
      if (cmd === "search_atlassian_resources") {
        return Promise.resolve({ resources: [] });
      }
      if (cmd === "resolve_atlassian_resource_urls") {
        return Promise.resolve({ results: [] });
      }
      return Promise.resolve(undefined);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps the runtime selector content-sized instead of filling the footer row", () => {
    renderComposer();

    expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveClass(
      "max-w-[34rem]",
    );
    expect(screen.getByTestId("agent-composer-runtime-pill")).not.toHaveClass(
      "flex-1",
    );
    expect(screen.getByTestId("agent-composer-submit")).toHaveClass("ml-auto");
  });

  it("hides the runtime pill when no model is available to show or select", () => {
    renderComposer({
      model: {
        value: "",
        onValueChange: vi.fn(),
        options: [],
        disabled: true,
      },
    });

    expect(
      screen.queryByTestId("agent-composer-runtime-pill"),
    ).not.toBeInTheDocument();
  });

  it("keeps the runtime pill when a model is selectable even with no current value", () => {
    renderComposer({
      model: {
        value: "",
        onValueChange: vi.fn(),
        options: [{ id: "gpt-5.5", label: "gpt-5.5" }],
      },
    });

    expect(
      screen.getByTestId("agent-composer-runtime-pill"),
    ).toBeInTheDocument();
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

  it("bounds the runtime selector popover to available height with internal scrolling", () => {
    renderComposer({
      model: {
        value: "gpt-5.5",
        onValueChange: vi.fn(),
        options: [
          { id: "gpt-5.5", label: "gpt-5.5", description: "Frontier model." },
          {
            id: "gpt-5.4",
            label: "gpt-5.4",
            description: "Strong model for coding.",
          },
          {
            id: "gpt-5.4-mini",
            label: "gpt-5.4-mini",
            description: "Small and fast.",
          },
          {
            id: "gpt-5.3-codex",
            label: "gpt-5.3-codex",
            description: "Coding optimized.",
          },
          {
            id: "gpt-5.3-codex-spark",
            label: "gpt-5.3-codex-spark",
            description: "Ultra fast.",
          },
        ],
        onOpenModelSettings: vi.fn(),
      },
      effort: {
        value: "xhigh",
        onValueChange: vi.fn(),
        options: [
          { id: "low", label: "Low", description: "Fastest responses." },
          { id: "medium", label: "Medium", description: "Balanced depth." },
          { id: "high", label: "High", description: "Greater depth." },
          {
            id: "xhigh",
            label: "Extra High",
            description: "Long-horizon work.",
          },
        ],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));

    const selectedModel = screen.getByTestId(
      "agent-composer-runtime-model-gpt-5.5",
    );
    const runtimePopover = selectedModel.closest("[data-side='top']");

    expect(runtimePopover).toHaveClass(
      "max-h-[var(--radix-popover-content-available-height)]",
    );
    expect(runtimePopover).toHaveClass("overflow-y-auto");
    expect(runtimePopover).toHaveClass("overscroll-contain");
    expect(runtimePopover).not.toHaveClass("overflow-hidden");
  });

  it("shows disabled Codex Fast mode reason in the runtime selector", () => {
    renderComposer({
      model: {
        value: "gpt-5.4-mini",
        onValueChange: vi.fn(),
        options: [{ id: "gpt-5.4-mini", label: "gpt-5.4-mini" }],
        fastMode: {
          visible: true,
          value: false,
          disabled: true,
          description: "Fast mode is not available for gpt-5.4-mini.",
          onValueChange: vi.fn(),
          testId: "composer-codex-fast-mode",
        },
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));

    expect(screen.getByText("Fast mode")).toBeInTheDocument();
    expect(
      screen.getByText("Fast mode is not available for gpt-5.4-mini."),
    ).toBeInTheDocument();
    expect(screen.getByTestId("composer-codex-fast-mode")).toBeDisabled();
  });

  it("filters and selects projects from the compact project line", () => {
    const onValueChange = vi.fn();
    render(
      <TooltipProvider>
        <AgentComposerProjectLine
          value="project-1"
          onValueChange={onValueChange}
          placeholder="Project"
          testId="agent-composer-project-line"
          options={[
            {
              id: "project-1",
              label: "RalphX",
              description: "/work/ralphx",
            },
            {
              id: "project-2",
              label: "PrintSpeak",
              description: "/work/printspeak",
            },
          ]}
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByTestId("agent-composer-project-line"));
    fireEvent.change(screen.getByPlaceholderText("Search projects..."), {
      target: { value: "print" },
    });

    expect(screen.getByText("PrintSpeak")).toBeInTheDocument();
    fireEvent.click(screen.getByText("PrintSpeak"));

    expect(onValueChange).toHaveBeenCalledWith("project-2");
  });

  it("shows an empty state when no compact project line results match", () => {
    render(
      <TooltipProvider>
        <AgentComposerProjectLine
          value=""
          onValueChange={vi.fn()}
          placeholder="Choose project"
          testId="agent-composer-project-line-empty"
          options={[
            {
              id: "project-1",
              label: "RalphX",
              description: "/work/ralphx",
            },
          ]}
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByTestId("agent-composer-project-line-empty"));
    fireEvent.change(screen.getByPlaceholderText("Search projects..."), {
      target: { value: "missing" },
    });

    expect(screen.getByText("No projects found")).toBeInTheDocument();
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

    expect(
      screen.getByText("Type / for commands and skills"),
    ).toBeInTheDocument();
    expect(screen.getByText("@ for references")).toBeInTheDocument();
    expect(screen.queryByText("$ for skills")).not.toBeInTheDocument();
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
    expect(
      screen.getByText("Plan execution is still active"),
    ).toBeInTheDocument();
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
          { id: "plan", label: "Plan" },
        ],
      },
    });

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/ch" } });
    textarea.setSelectionRange(3, 3);
    fireEvent.keyUp(textarea);
    await screen.findByTestId("agent-composer-menu-item-command:mode:chat");
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onValueChange).toHaveBeenCalledWith("chat");
    expect(textarea.value).toBe("");
  });

  it("runs the plan slash mode command from the composer menu", async () => {
    const onValueChange = vi.fn();
    renderComposer({
      mode: {
        value: "edit",
        onValueChange,
        options: [
          { id: "edit", label: "Agent" },
          { id: "plan", label: "Plan" },
          { id: "chat", label: "Chat" },
        ],
      },
    });

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/pl" } });
    textarea.setSelectionRange(3, 3);
    fireEvent.keyUp(textarea);
    await screen.findByTestId("agent-composer-menu-item-command:mode:plan");
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onValueChange).toHaveBeenCalledWith("plan");
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/fo" } });
    textarea.setSelectionRange(3, 3);
    fireEvent.keyUp(textarea);
    await screen.findByTestId("agent-composer-menu-item-command:custom:fork");
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => expect(onFork).toHaveBeenCalledTimes(1));
    expect(textarea.value).toBe("");
  });

  it("runs the refine slash command from Plan mode", async () => {
    const onSend = vi.fn();
    renderComposer({
      mode: {
        value: "plan",
        onValueChange: vi.fn(),
        options: [
          { id: "plan", label: "Plan" },
          { id: "edit", label: "Agent" },
        ],
      },
      onSend,
    });

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/ref" } });
    textarea.setSelectionRange(4, 4);
    fireEvent.keyUp(textarea);
    await screen.findByTestId("agent-composer-menu-item-command:plan:refine");
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => {
      expect(onSend).toHaveBeenCalledWith(
        "Please verify and refine the current plan.",
      );
    });
    expect(textarea.value).toBe("");
  });

  it("submits question-mode answers while the agent is generating", () => {
    const onSend = vi.fn();
    const onMatchedOptions = vi.fn();
    renderComposer({
      agentStatus: "generating",
      isSubmitting: true,
      onSend,
      questionMode: {
        optionCount: 2,
        multiSelect: false,
        onMatchedOptions,
      },
    });

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "1" } });
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onMatchedOptions).toHaveBeenLastCalledWith([0]);
    expect(onSend).toHaveBeenCalledWith("1");
  });

  it("submits the configured empty message when the textarea is blank", () => {
    const onSend = vi.fn();
    renderComposer({
      onSend,
      emptySubmitMessage: "Review this PR.",
    });

    const action = screen.getByTestId("agent-composer-submit");
    expect(action).toBeEnabled();

    fireEvent.click(action);

    expect(onSend).toHaveBeenCalledWith("Review this PR.");
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
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

    expect(onSend).toHaveBeenCalledWith("Read", {
      projectReferences: [{ path: "src/main.ts", kind: "file" }],
    });
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Read @" } });
    textarea.setSelectionRange("Read @".length, "Read @".length);
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId("agent-composer-menu-item-path:src");
    fireEvent.mouseDown(item);
    fireEvent.click(item);

    expect(
      screen.getByTestId("agent-composer-reference-pill-project:src"),
    ).toHaveTextContent("Folder");
    fireEvent.click(screen.getByLabelText("Remove folder reference src"));
    expect(
      screen.queryByTestId("agent-composer-reference-pill-project:src"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith("Read");
  });

  it("does not store free-form @ tokens as references without menu selection", () => {
    const onSend = vi.fn();
    renderComposer({ onSend });

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, {
      target: { value: "Check @invalid-reference and @jira:RX-404" },
    });
    textarea.setSelectionRange(textarea.value.length, textarea.value.length);
    fireEvent.keyUp(textarea);
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith(
      "Check @invalid-reference and @jira:RX-404",
    );
  });

  it("surfaces Atlassian search failures instead of showing an empty result state", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "search_atlassian_resources") {
        return Promise.reject(
          new Error("Atlassian integration is not enabled"),
        );
      }
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({ skills: [] });
      }
      if (cmd === "search_agent_composer_entries") {
        return Promise.resolve({ entries: [], truncated: false });
      }
      return Promise.resolve(undefined);
    });
    renderComposer();

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Work on @jira:PDM-81" } });
    textarea.setSelectionRange(textarea.value.length, textarea.value.length);
    fireEvent.keyUp(textarea);

    expect(
      await screen.findByText(
        "Jira search failed: Atlassian integration is not enabled",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("No matching integration items"),
    ).not.toBeInTheDocument();
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Work on @jira:RX" } });
    textarea.setSelectionRange(
      "Work on @jira:RX".length,
      "Work on @jira:RX".length,
    );
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId(
      "agent-composer-menu-item-integration:jira:RX-42",
    );
    fireEvent.mouseDown(item);
    fireEvent.click(item);
    expect(textarea).toHaveValue("Work on ");
    expect(
      screen.getByTestId(
        "agent-composer-reference-pill-integration:jira:RX-42",
      ),
    ).toHaveTextContent("Jira");
    expect(
      screen.getByTestId(
        "agent-composer-reference-pill-integration:jira:RX-42",
      ),
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

  it("turns resolved pasted Atlassian URLs into structured integration references", async () => {
    const onSend = vi.fn();
    const pastedText =
      "Please check https://example.atlassian.net/browse/RX-42 and https://other.atlassian.net/browse/RX-99";
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "resolve_atlassian_resource_urls") {
        return Promise.resolve({
          results: [
            {
              inputUrl: "https://example.atlassian.net/browse/RX-42",
              resource: {
                kind: "jira",
                id: "RX-42",
                key: "RX-42",
                title: "Fix composer paste",
                url: "https://example.atlassian.net/browse/RX-42",
                excerpt: null,
              },
            },
            {
              inputUrl: "https://other.atlassian.net/browse/RX-99",
              resource: null,
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.paste(textarea, {
      clipboardData: {
        getData: () => pastedText,
      },
    });

    expect(textarea).toHaveValue(pastedText);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("resolve_atlassian_resource_urls", {
        input: {
          urls: [
            "https://example.atlassian.net/browse/RX-42",
            "https://other.atlassian.net/browse/RX-99",
          ],
        },
      }),
    );
    expect(
      await screen.findByTestId(
        "agent-composer-reference-pill-integration:jira:RX-42",
      ),
    ).toHaveTextContent("Fix composer paste");
    await waitFor(() =>
      expect(textarea).toHaveValue(
        "Please check and https://other.atlassian.net/browse/RX-99",
      ),
    );

    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith(
      "Please check and https://other.atlassian.net/browse/RX-99",
      {
        integrationReferences: [
          {
            provider: "atlassian",
            kind: "jira",
            id: "RX-42",
            key: "RX-42",
            title: "Fix composer paste",
            url: "https://example.atlassian.net/browse/RX-42",
          },
        ],
      },
    );
  });

  it("leaves pasted Atlassian URLs intact when no pasted URL resolves", async () => {
    const pastedText =
      "Please check https://example.atlassian.net/browse/RX-404";
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "resolve_atlassian_resource_urls") {
        return Promise.resolve({
          results: [
            {
              inputUrl: "https://example.atlassian.net/browse/RX-404",
              resource: null,
            },
          ],
        });
      }
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({ skills: [] });
      }
      return Promise.resolve({ entries: [], truncated: false });
    });
    renderComposer();

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.paste(textarea, {
      clipboardData: {
        getData: () => pastedText,
      },
    });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("resolve_atlassian_resource_urls", {
        input: {
          urls: ["https://example.atlassian.net/browse/RX-404"],
        },
      }),
    );
    expect(textarea).toHaveValue(pastedText);
    expect(
      screen.queryByTestId("agent-composer-reference-pill-integration:jira:RX-404"),
    ).not.toBeInTheDocument();
  });

  it("keeps pasted text when the resolved backend URL is no longer present", async () => {
    const pastedText =
      "Please check https://example.atlassian.net/browse/RX-42";
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "resolve_atlassian_resource_urls") {
        return Promise.resolve({
          results: [
            {
              inputUrl: "https://example.atlassian.net/browse/RX-43",
              resource: {
                kind: "jira",
                id: "RX-43",
                key: "RX-43",
                title: "Stale resolver result",
                url: "https://example.atlassian.net/browse/RX-43",
                excerpt: null,
              },
            },
          ],
        });
      }
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({ skills: [] });
      }
      return Promise.resolve({ entries: [], truncated: false });
    });
    renderComposer();

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.paste(textarea, {
      clipboardData: {
        getData: () => pastedText,
      },
    });

    expect(
      await screen.findByTestId(
        "agent-composer-reference-pill-integration:jira:RX-43",
      ),
    ).toHaveTextContent("Stale resolver result");
    expect(textarea).toHaveValue(pastedText);
  });

  it("does not resolve pasted Atlassian URLs while the composer is read-only", () => {
    renderComposer({ isReadOnly: true });

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    vi.mocked(invoke).mockClear();
    fireEvent.paste(textarea, {
      clipboardData: {
        getData: () => "https://example.atlassian.net/browse/RX-42",
      },
    });

    expect(invoke).not.toHaveBeenCalledWith(
      "resolve_atlassian_resource_urls",
      expect.anything(),
    );
  });

  it("does not invoke Atlassian resolution for non-URL pasted text", () => {
    renderComposer();

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    vi.mocked(invoke).mockClear();
    fireEvent.paste(textarea, {
      clipboardData: {
        getData: () => "plain text only",
      },
    });

    expect(invoke).not.toHaveBeenCalledWith(
      "resolve_atlassian_resource_urls",
      expect.anything(),
    );
  });

  it("hydrates initial ticket references and waits for the user prompt before sending", async () => {
    const onSend = vi.fn();
    const view = renderComposer({
      onSend,
      initialIntegrationReferences: [
        {
          provider: "clickup",
          kind: "clickup",
          id: "TASK-123",
          key: "TASK-123",
          title: "Demo task",
          url: "https://app.clickup.com/t/workspace-1/TASK-123",
        },
      ],
    });

    const pill = await screen.findByTestId(
      "agent-composer-reference-pill-integration:clickup:TASK-123",
    );
    expect(pill).toHaveTextContent("ClickUp");
    expect(pill).toHaveTextContent("Demo task");

    fireEvent.click(
      screen.getByRole("button", {
        name: "Remove ClickUp reference TASK-123",
      }),
    );
    expect(
      screen.queryByTestId(
        "agent-composer-reference-pill-integration:clickup:TASK-123",
      ),
    ).not.toBeInTheDocument();

    view.unmount();
    renderComposer({
      onSend,
      initialIntegrationReferences: [
        {
          provider: "clickup",
          kind: "clickup",
          id: "TASK-123",
          key: "TASK-123",
          title: "Demo task",
          url: "https://app.clickup.com/t/workspace-1/TASK-123",
        },
      ],
    });
    await screen.findByTestId(
      "agent-composer-reference-pill-integration:clickup:TASK-123",
    );

    fireEvent.click(screen.getByTestId("agent-composer-submit"));
    expect(onSend).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("Message input"), {
      target: { value: "Please scope this ticket" },
    });
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith("Please scope this ticket", {
      integrationReferences: [
        {
          provider: "clickup",
          kind: "clickup",
          id: "TASK-123",
          key: "TASK-123",
          title: "Demo task",
          url: "https://app.clickup.com/t/workspace-1/TASK-123",
        },
      ],
    });
  });

  it("sends selected plans as structured artifact references", async () => {
    const onSend = vi.fn();
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "search_agent_composer_plan_references") {
        return Promise.resolve({
          plans: [
            {
              sessionId: "session-1",
              artifactId: "artifact-1",
              title: "Checkout Plan",
              status: "approved",
              artifactVersion: 2,
              updatedAt: "2026-05-23T10:00:00Z",
              approvedAt: "2026-05-23T10:01:00Z",
            },
          ],
          truncated: false,
        });
      }
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({ skills: [] });
      }
      if (cmd === "search_atlassian_resources") {
        return Promise.resolve({ resources: [] });
      }
      return Promise.resolve({ entries: [], truncated: false });
    });
    renderComposer({ onSend });

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Use @plan:checkout" } });
    textarea.setSelectionRange(
      "Use @plan:checkout".length,
      "Use @plan:checkout".length,
    );
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId(
      "agent-composer-menu-item-plan:artifact-1",
    );
    fireEvent.mouseDown(item);
    fireEvent.click(item);
    expect(textarea).toHaveValue("Use ");
    expect(
      screen.getByTestId(
        "agent-composer-reference-pill-artifact:plan:artifact-1",
      ),
    ).toHaveTextContent("Plan");
    expect(
      screen.getByTestId(
        "agent-composer-reference-pill-artifact:plan:artifact-1",
      ),
    ).toHaveTextContent("Checkout Plan");
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith("Use", {
      artifactReferences: [
        {
          kind: "plan",
          artifactId: "artifact-1",
          title: "Checkout Plan",
          sessionId: "session-1",
          version: 2,
          status: "approved",
        },
      ],
    });
  });

  it("extracts typed @plan references when sent without selecting a menu item", async () => {
    const onSend = vi.fn();
    renderComposer({ onSend });

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "Use @plan:artifact-2" } });
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith("Use @plan:artifact-2", {
      artifactReferences: [{ kind: "plan", artifactId: "artifact-2" }],
    });
  });

  it.each([
    ["Jira", "@jira:", "jira"],
    ["Confluence", "@confluence:", "confluence"],
  ])(
    "inserts %s triggers from the plus menu and opens search",
    async (label, expectedValue, kind) => {
      renderComposer();

      fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
      fireEvent.click(screen.getByText(label));

      const textarea = screen.getByLabelText("Message input");
      expect(textarea).toHaveValue(expectedValue);
      await waitFor(() => expect(textarea).toHaveFocus());
      expect(
        await screen.findByTestId("agent-composer-command-menu"),
      ).toBeInTheDocument();
      await waitFor(() =>
        expect(invoke).toHaveBeenCalledWith("search_atlassian_resources", {
          input: { kind, query: "", limit: 12 },
        }),
      );
    },
  );

  it("inserts ClickUp triggers from the plus menu and opens task search", async () => {
    renderComposer();

    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    fireEvent.click(screen.getByText("ClickUp"));

    const textarea = screen.getByLabelText("Message input");
    expect(textarea).toHaveValue("@clickup:");
    await waitFor(() => expect(textarea).toHaveFocus());
    expect(
      await screen.findByTestId("agent-composer-command-menu"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("search_clickup_tasks", {
        input: { spaceIds: [], query: "", limit: 10 },
      }),
    );
  });

  it("inserts plan triggers from the plus menu and opens plan search", async () => {
    renderComposer();

    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    fireEvent.click(screen.getByText("Plan"));

    const textarea = screen.getByLabelText("Message input");
    expect(textarea).toHaveValue("@plan:");
    await waitFor(() => expect(textarea).toHaveFocus());
    expect(
      await screen.findByTestId("agent-composer-command-menu"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "search_agent_composer_plan_references",
        {
          input: { projectId: "project-1", query: "", limit: 12 },
        },
      ),
    );
  });

  it("runs fork session from the plus menu", async () => {
    const onForkSession = vi.fn().mockResolvedValue(undefined);
    renderComposer({ onForkSession });

    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    fireEvent.click(screen.getByText("Fork session"));

    await waitFor(() => expect(onForkSession).toHaveBeenCalledTimes(1));
  });

  it("appends internal skill directives for selected slash skills", async () => {
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/work" } });
    textarea.setSelectionRange("/work".length, "/work".length);
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId(
      "agent-composer-menu-item-skill:internal:workspace-swe",
    );
    fireEvent.mouseDown(item);
    fireEvent.click(item);
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith(
      "workspace-swe\n\n<!-- ralphx_internal_skill=workspace-swe -->",
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
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

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/rev" } });
    textarea.setSelectionRange("/rev".length, "/rev".length);
    fireEvent.keyUp(textarea);

    await screen.findByTestId(
      "agent-composer-menu-item-skill:claude:project:review",
    );
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(textarea.value).toBe("/review ");
  });

  it("includes Codex-native dollar skills in the slash command menu", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_agent_composer_skills") {
        return Promise.resolve({
          skills: [
            {
              id: "codex:global:plugin-creator",
              name: "plugin-creator",
              displayName: null,
              description: "Create Codex plugins.",
              source: "harness-native",
              providerHarness: "codex",
              scope: "global",
              invocationKind: "harness-native-token",
              invocationValue: "$plugin-creator",
              enabled: true,
              sourcePath: ".codex/skills/plugin-creator/SKILL.md",
            },
          ],
        });
      }
      return Promise.resolve({ entries: [], truncated: false });
    });
    renderComposer();

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "/plug" } });
    textarea.setSelectionRange("/plug".length, "/plug".length);
    fireEvent.keyUp(textarea);

    await screen.findByTestId(
      "agent-composer-menu-item-skill:codex:global:plugin-creator",
    );
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(textarea.value).toBe("$plugin-creator ");
  });

  it("treats dropped markdown files as normal chat attachments", async () => {
    const onFilesSelected = vi.fn();
    renderComposer({
      dataTestId: "agent-composer",
      enableAttachments: true,
      onFilesSelected,
    });
    const file = new File(["content"], "notes.md", { type: "text/markdown" });
    const composer = screen.getByTestId("agent-composer");

    fireEvent.dragEnter(composer, makeDropEvent([file]));

    expect(
      screen.getByTestId("chat-composer-drop-overlay"),
    ).toBeInTheDocument();

    fireEvent.drop(composer, makeDropEvent([file]));

    await waitFor(() => {
      expect(onFilesSelected).toHaveBeenCalledWith([file]);
    });
    expect(invoke).not.toHaveBeenCalledWith(
      "import_agent_conversation_plan",
      expect.anything(),
    );
    expect(
      screen.queryByTestId("chat-composer-drop-overlay"),
    ).not.toBeInTheDocument();
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

    expect(
      screen.queryByTestId("chat-composer-drop-overlay"),
    ).not.toBeInTheDocument();
    expect(onFilesSelected).not.toHaveBeenCalled();
  });

  it("ignores active terminal panel drags when WebKit only reports file types", () => {
    const onFilesSelected = vi.fn();
    const file = new File(["content"], "terminal-drag.txt", {
      type: "text/plain",
    });
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

    expect(
      screen.queryByTestId("chat-composer-drop-overlay"),
    ).not.toBeInTheDocument();
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
    expect(
      screen.queryByTestId("chat-composer-drop-overlay"),
    ).not.toBeInTheDocument();
  });

  it("orders the footer controls mode → model → chat focus", () => {
    renderComposer({
      chatFocus: {
        value: "workspace",
        onValueChange: vi.fn(),
        options: [
          { id: "workspace", label: "Workspace" },
          { id: "verification", label: "Verification" },
        ],
      },
    });

    const modeChip = screen.getByTestId("agent-composer-mode-chip");
    const runtimePill = screen.getByTestId("agent-composer-runtime-pill");
    const chatPill = screen.getByTestId("agent-composer-chat-focus-pill");

    // mode precedes model
    expect(
      modeChip.compareDocumentPosition(runtimePill) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    // model precedes chat focus
    expect(
      runtimePill.compareDocumentPosition(chatPill) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  describe("collapsible resting state", () => {
    it("rests in a minimal one-row state when idle and empty", () => {
      renderComposer({ dataTestId: "agent-composer", collapsible: true });

      const surface = screen.getByTestId("agent-composer");
      expect(surface).toHaveAttribute("data-collapsed", "true");

      // Helper line is hidden (reveals on focus) so the resting bar is compact.
      expect(
        screen.getByTestId("agent-composer-helper-reveal"),
      ).toHaveAttribute("data-visible", "false");

      // Runtime ("GPT") + Mode chips drop to the compact height, and the mode
      // chip sheds its "Mode" eyebrow label (eyebrows show only when expanded).
      expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveClass(
        "h-8",
      );
      const modeChip = screen.getByTestId("agent-composer-mode-chip");
      expect(modeChip).toHaveClass("h-8");
      expect(modeChip.textContent).toBe("Agent");

      const textarea = screen.getByLabelText(
        "Message input",
      ) as HTMLTextAreaElement;
      expect(textarea.style.height).toBe("38px");
    });

    it("expands when text is entered and reveals the helper + full chips", () => {
      renderComposer({ dataTestId: "agent-composer", collapsible: true });

      const textarea = screen.getByLabelText(
        "Message input",
      ) as HTMLTextAreaElement;
      fireEvent.change(textarea, { target: { value: "hello" } });

      const surface = screen.getByTestId("agent-composer");
      expect(surface).toHaveAttribute("data-collapsed", "false");
      expect(
        screen.getByTestId("agent-composer-helper-reveal"),
      ).toHaveAttribute("data-visible", "true");
      expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveClass(
        "h-10",
      );
      const modeChip = screen.getByTestId("agent-composer-mode-chip");
      expect(modeChip).toHaveClass("h-10");
      expect(modeChip.textContent).toBe("ModeAgent");
      expect(textarea.style.height).toBe("92px");
    });

    it("notifies layout changes when textarea content resizes the visible composer", () => {
      let measuredScrollHeight = 96;
      const scrollHeightSpy = vi
        .spyOn(HTMLTextAreaElement.prototype, "scrollHeight", "get")
        .mockImplementation(() => measuredScrollHeight);
      const onLayoutChange = vi.fn();

      try {
        renderComposer({
          dataTestId: "agent-composer",
          collapsible: false,
          onLayoutChange,
        });

        const textarea = screen.getByLabelText(
          "Message input",
        ) as HTMLTextAreaElement;
        expect(textarea.style.height).toBe("96px");

        onLayoutChange.mockClear();
        measuredScrollHeight = 132;
        fireEvent.change(textarea, {
          target: { value: "line one\nline two\nline three" },
        });

        expect(textarea.style.height).toBe("132px");
        expect(onLayoutChange).toHaveBeenCalledTimes(1);
      } finally {
        scrollHeightSpy.mockRestore();
      }
    });

    it("stays expanded after blur while the prompt has content", () => {
      renderComposer({ dataTestId: "agent-composer", collapsible: true });

      const textarea = screen.getByLabelText(
        "Message input",
      ) as HTMLTextAreaElement;
      fireEvent.focus(textarea);
      fireEvent.change(textarea, { target: { value: "draft message" } });
      fireEvent.blur(textarea);

      expect(screen.getByTestId("agent-composer")).toHaveAttribute(
        "data-collapsed",
        "false",
      );
    });

    it("expands when the textarea is focused even with no text", () => {
      const onLayoutChange = vi.fn();
      renderComposer({ dataTestId: "agent-composer", collapsible: true, onLayoutChange });
      const surface = screen.getByTestId("agent-composer");
      const textarea = screen.getByLabelText(
        "Message input",
      ) as HTMLTextAreaElement;

      onLayoutChange.mockClear();
      fireEvent.focus(textarea);
      expect(surface).toHaveAttribute("data-collapsed", "false");
      expect(onLayoutChange).toHaveBeenCalled();

      // Blur with no text returns to the minimal resting state.
      onLayoutChange.mockClear();
      fireEvent.blur(textarea);
      expect(surface).toHaveAttribute("data-collapsed", "true");
      expect(onLayoutChange).toHaveBeenCalled();
    });

    it("stays minimal when a popover opens on an unfocused composer (no flicker)", () => {
      renderComposer({ dataTestId: "agent-composer", collapsible: true });
      const surface = screen.getByTestId("agent-composer");

      // Opening the "+" action menu without focusing the textarea must not
      // expand the composer.
      fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
      expect(surface).toHaveAttribute("data-collapsed", "true");
    });

    it("returns to the minimal state after blur while the agent is generating", () => {
      renderComposer({
        dataTestId: "agent-composer",
        collapsible: true,
        agentStatus: "generating",
        onStop: vi.fn(),
      });

      const surface = screen.getByTestId("agent-composer");
      const textarea = screen.getByLabelText(
        "Message input",
      ) as HTMLTextAreaElement;

      expect(surface).toHaveAttribute("data-collapsed", "true");
      fireEvent.focus(textarea);
      expect(surface).toHaveAttribute("data-collapsed", "false");
      fireEvent.blur(textarea);
      expect(surface).toHaveAttribute("data-collapsed", "true");

      const stopButton = screen.getByTestId("agent-composer-submit");
      expect(stopButton).toHaveAccessibleName("Stop agent");
      expect(stopButton).toBeEnabled();
    });

    it("still sends on Enter from the collapsible composer", () => {
      const onSend = vi.fn();
      renderComposer({
        dataTestId: "agent-composer",
        collapsible: true,
        onSend,
      });

      const textarea = screen.getByLabelText(
        "Message input",
      ) as HTMLTextAreaElement;
      fireEvent.focus(textarea);
      fireEvent.change(textarea, { target: { value: "ship it" } });
      fireEvent.keyDown(textarea, { key: "Enter" });

      expect(onSend).toHaveBeenCalledWith("ship it");
    });

    it("never collapses when collapsible is not opted in (start composer)", () => {
      renderComposer({ dataTestId: "agent-composer" });

      const surface = screen.getByTestId("agent-composer");
      expect(surface).toHaveAttribute("data-collapsible", "false");
      expect(surface).toHaveAttribute("data-collapsed", "false");
      expect(
        screen.getByTestId("agent-composer-helper-reveal"),
      ).toHaveAttribute("data-visible", "true");
      expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveClass(
        "h-10",
      );
    });

    it("loads minimal: does not auto-focus or expand on mount even with autoFocus", () => {
      renderComposer({
        dataTestId: "agent-composer",
        collapsible: true,
        autoFocus: true,
      });

      expect(screen.getByTestId("agent-composer")).toHaveAttribute(
        "data-collapsed",
        "true",
      );
      expect(screen.getByLabelText("Message input")).not.toHaveFocus();
    });

    it("still auto-focuses a non-collapsible composer on mount", () => {
      renderComposer({ dataTestId: "agent-composer", autoFocus: true });

      expect(screen.getByLabelText("Message input")).toHaveFocus();
    });
  });
});
