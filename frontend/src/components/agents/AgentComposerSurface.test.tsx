import { useState } from "react";

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@radix-ui/react-tooltip";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AtlassianIntegrationSettings } from "@/api/atlassian";
import type { GranolaIntegrationSettings } from "@/api/granola";
import type {
  TicketingProvider,
  TicketingProviderSummary,
} from "@/api/ticketing";
import { setRalphxTerminalDockDragActive } from "@/lib/internalDragTypes";
import {
  AgentComposerProjectLine,
  AgentComposerSurface,
} from "./AgentComposerSurface";
import { stageComposerExcerptReference } from "./artifact-selection/composerExcerptBridge";
import { chatKeys } from "@/hooks/useChat";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import { UNKNOWN_OUTCOME_MESSAGE } from "@/lib/remote/unknown-outcome";
import { LOCAL_ENVIRONMENT_ID } from "@/stores/environmentStore";
import { toast } from "sonner";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: () => Promise.resolve(() => {}),
  }),
}));

vi.mock("@tauri-apps/plugin-fs", () => ({
  readFile: vi.fn(),
  stat: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

vi.mock("sonner", () => ({
  toast: { info: vi.fn(), error: vi.fn(), success: vi.fn() },
}));

const featureFlags = vi.hoisted(() => ({ agentPersonas: false }));
vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: () => ({ data: featureFlags }),
}));

type ComposerProps = Parameters<typeof AgentComposerSurface>[0];

const INTEGRATION_UPDATED_AT = "2026-07-20T00:00:00Z";

function ticketingProvider(
  provider: TicketingProvider,
  overrides: Partial<TicketingProviderSummary> = {},
): TicketingProviderSummary {
  return {
    provider,
    label: provider === "clickup" ? "ClickUp" : `${provider[0]?.toUpperCase()}${provider.slice(1)}`,
    enabled: true,
    connectionStatus: "connected",
    capabilities: {
      supportsBoards: false,
      supportsKanban: false,
      kanbanWrite: false,
      statusWrite: false,
      assignmentWrite: false,
      commentWrite: false,
      labelWrite: false,
      freshness: "manual",
    },
    ...overrides,
  };
}

function atlassianSettings(
  overrides: Partial<AtlassianIntegrationSettings> = {},
): AtlassianIntegrationSettings {
  return {
    enabled: true,
    authMethod: "api_token",
    siteUrl: "https://example.atlassian.net",
    email: "dev@example.com",
    hasApiToken: true,
    hasOauthClientSecret: false,
    hasOauthToken: false,
    validationStatus: "valid",
    jiraAvailable: true,
    confluenceAvailable: true,
    updatedAt: INTEGRATION_UPDATED_AT,
    ...overrides,
  };
}

function granolaSettings(
  overrides: Partial<GranolaIntegrationSettings> = {},
): GranolaIntegrationSettings {
  return {
    enabled: true,
    hasApiToken: true,
    validationStatus: "valid",
    updatedAt: INTEGRATION_UPDATED_AT,
    ...overrides,
  };
}

function defaultComposerInvokeResponse(cmd: string): unknown {
  if (cmd === "list_conversation_folder_references") return [];
  if (cmd === "list_agent_composer_skills") return { skills: [] };
  if (cmd === "search_agent_composer_entries") {
    return { entries: [], truncated: false };
  }
  if (cmd === "search_agent_composer_plan_references") {
    return { plans: [], truncated: false };
  }
  if (cmd === "search_atlassian_resources") return { resources: [] };
  if (cmd === "resolve_atlassian_resource_urls") return { results: [] };
  return undefined;
}

function mockComposerIntegrationAvailability({
  providers = [
    ticketingProvider("jira"),
    ticketingProvider("linear"),
    ticketingProvider("clickup"),
  ],
  atlassian = atlassianSettings(),
  granola = granolaSettings(),
}: {
  providers?: TicketingProviderSummary[];
  atlassian?: AtlassianIntegrationSettings;
  granola?: GranolaIntegrationSettings;
} = {}) {
  vi.mocked(invoke).mockImplementation((cmd) => {
    if (cmd === "list_ticketing_providers") return Promise.resolve(providers);
    if (cmd === "get_atlassian_integration_settings") {
      return Promise.resolve(atlassian);
    }
    if (cmd === "get_granola_integration_settings") {
      return Promise.resolve(granola);
    }
    return Promise.resolve(defaultComposerInvokeResponse(cmd));
  });
}

function renderComposer(
  overrides: Partial<ComposerProps> = {},
  queryClient: QueryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  }),
) {
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
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

/** Intercepts `requestAnimationFrame` so paint-boundary-deferred work (like the
 * folder-reference hydration query) can be held and flushed explicitly, proving
 * it does not run in the same synchronous render as the composer shell. */
function holdDeferredFrames() {
  const originalRequestAnimationFrame = window.requestAnimationFrame;
  const originalCancelAnimationFrame = window.cancelAnimationFrame;
  const callbacks = new Map<number, FrameRequestCallback>();
  let nextId = 1;

  window.requestAnimationFrame = ((callback: FrameRequestCallback) => {
    const id = nextId;
    nextId += 1;
    callbacks.set(id, callback);
    return id;
  }) as typeof window.requestAnimationFrame;
  window.cancelAnimationFrame = ((id: number) => {
    callbacks.delete(id);
  }) as typeof window.cancelAnimationFrame;

  return {
    flush() {
      const queuedCallbacks = [...callbacks.values()];
      callbacks.clear();
      for (const callback of queuedCallbacks) {
        callback(performance.now());
      }
    },
    restore() {
      callbacks.clear();
      window.requestAnimationFrame = originalRequestAnimationFrame;
      window.cancelAnimationFrame = originalCancelAnimationFrame;
    },
  };
}

describe("AgentComposerSurface", () => {
  beforeEach(() => {
    vi.useRealTimers();
    setRalphxTerminalDockDragActive(false);
    mockComposerIntegrationAvailability();
  });

  it("shows Add folder directly after Add files for a Project conversation", async () => {
    const normal = renderComposer({
      conversationId: "conversation-1",
      enableAttachments: true,
      project: {
        value: "project-1",
        onValueChange: vi.fn(),
        options: [{ id: "project-1", label: "RalphX" }],
        placeholder: "Project",
      },
    });
    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    const addFiles = screen.getByRole("button", { name: "Add files" });
    const addFolder = screen.getByRole("button", { name: "Add folder" });
    expect(
      addFiles.compareDocumentPosition(addFolder) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      screen.queryByRole("button", { name: "New project" }),
    ).not.toBeInTheDocument();
    normal.unmount();

    // Persona mode remains tied to the separate Personas capability.
    renderComposer({
      conversationId: "conversation-2",
      enableAttachments: true,
      mode: { value: "persona_builder", onValueChange: vi.fn(), options: [] },
    });
    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    expect(screen.queryByRole("button", { name: "Add folder" })).not.toBeInTheDocument();
  });

  it("shows only integrations that are active in Settings", async () => {
    mockComposerIntegrationAvailability({
      providers: [
        ticketingProvider("jira"),
        ticketingProvider("linear", { enabled: false }),
        ticketingProvider("clickup"),
      ],
      atlassian: atlassianSettings({ confluenceAvailable: false }),
      granola: granolaSettings({ validationStatus: "invalid" }),
    });

    renderComposer();
    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));

    expect(await screen.findByRole("button", { name: "Jira" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ClickUp" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Confluence" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Linear" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Granola" })).not.toBeInTheDocument();
  });

  it("hides the Integrations section when no configured integration is active", async () => {
    mockComposerIntegrationAvailability({
      providers: [
        ticketingProvider("jira", {
          enabled: false,
          connectionStatus: "disconnected",
        }),
      ],
      atlassian: atlassianSettings({
        enabled: false,
        hasApiToken: false,
        validationStatus: "not_configured",
        jiraAvailable: false,
        confluenceAvailable: false,
      }),
      granola: granolaSettings({
        enabled: false,
        hasApiToken: false,
        validationStatus: "not_configured",
      }),
    });

    renderComposer();
    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "get_granola_integration_settings",
        {},
      ),
    );

    expect(screen.queryByText("Integrations")).not.toBeInTheDocument();
  });

  it("adds a picked folder and renders the hydrated chip after invalidation", async () => {
    const references = [] as Array<Record<string, string>>;
    vi.mocked(invoke).mockImplementation((cmd, args) => {
      if (cmd === "list_conversation_folder_references") return Promise.resolve(references);
      if (cmd === "add_conversation_folder_reference") {
        const input = (args as { input: Record<string, string> }).input;
        references.push({ id: "folder-1", ...input, createdAt: "2026-01-01T00:00:00Z" });
        return Promise.resolve(references[0]);
      }
      return Promise.resolve(undefined);
    });
    vi.mocked(openDialog).mockResolvedValue("/work/design-notes");
    renderComposer({ conversationId: "conversation-1", enableAttachments: true });
    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("add_conversation_folder_reference", { input: { conversationId: "conversation-1", folderPath: "/work/design-notes", displayName: "design-notes" } }));
    await waitFor(() => expect(screen.getByText("design-notes")).toBeInTheDocument());
  });

  it("hydrates folder chips and removes one with an accessible, tooltip-backed control", async () => {
    const references = [
      {
        id: "folder-1",
        conversationId: "conversation-1",
        folderPath: "/work/brand-kit",
        displayName: "brand-kit",
        createdAt: "2026-01-01T00:00:00Z",
      },
    ];
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_conversation_folder_references") return Promise.resolve(references);
      if (cmd === "remove_conversation_folder_reference") {
        references.splice(0, 1);
      }
      return Promise.resolve(undefined);
    });
    renderComposer({ conversationId: "conversation-1" });
    const folderChip = await screen.findByTestId(
      "agent-composer-reference-pill-folder:folder-1",
    );
    expect(folderChip).toHaveTextContent("Folder");
    expect(folderChip).toHaveTextContent("brand-kit");
    const remove = await screen.findByRole("button", { name: "Remove folder brand-kit" });
    expect(remove).toBeInTheDocument();
    fireEvent.click(remove);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("remove_conversation_folder_reference", { input: { conversationId: "conversation-1", folderReferenceId: "folder-1" } }));
    await waitFor(() => expect(screen.queryByText("brand-kit")).not.toBeInTheDocument());
  });

  it("snapshots hydrated folder references when sending", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_conversation_folder_references") {
        return Promise.resolve([
          {
            id: "folder-1",
            conversationId: "conversation-1",
            folderPath: "/work/brand-kit",
            displayName: "brand-kit",
            createdAt: "2026-01-01T00:00:00Z",
          },
        ]);
      }
      return Promise.resolve(undefined);
    });
    renderComposer({ conversationId: "conversation-1", onSend });

    await waitFor(() => expect(screen.getByText("brand-kit")).toBeInTheDocument());
    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "Review this folder" },
    });
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    await waitFor(() =>
      expect(onSend).toHaveBeenCalledWith("Review this folder", {
        folderReferences: [
          {
            id: "folder-1",
            folderPath: "/work/brand-kit",
            displayName: "brand-kit",
          },
        ],
      }),
    );
  });

  it("shows a retryable folder-reference warning instead of treating a failed list as empty", async () => {
    vi.mocked(invoke)
      .mockRejectedValueOnce(new Error("folder list unavailable"))
      .mockResolvedValueOnce([]);
    renderComposer({ conversationId: "conversation-folder-error" });

    expect(
      await screen.findByText(
        "Couldn't load folder references — previously attached folders may still be visible to the agent",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("folder-reference-chips")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry folder references" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledTimes(2),
    );
    await waitFor(() =>
      expect(
        screen.queryByText(
          "Couldn't load folder references — previously attached folders may still be visible to the agent",
        ),
      ).not.toBeInTheDocument(),
    );
  });

  it("says the file search failed instead of showing an empty project", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "search_agent_composer_entries") {
        return Promise.reject(new Error("git ls-files could not be run"));
      }
      return Promise.resolve(defaultComposerInvokeResponse(cmd as string));
    });
    renderComposer({ conversationId: "conversation-path-error" });

    const textarea = screen.getByLabelText(
      "Message input",
    ) as HTMLTextAreaElement;
    fireEvent.focus(textarea);
    fireEvent.change(textarea, { target: { value: "@src" } });
    textarea.setSelectionRange(4, 4);
    fireEvent.keyUp(textarea);

    // The whole point of the host-side fail-open repair: a failed search must not be
    // indistinguishable from a project with no matching files.
    expect(
      await screen.findByText(/File search failed: git ls-files could not be run/),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("No matching files or folders"),
    ).not.toBeInTheDocument();
  });

  it("shows the full folder path from the keyboard-focusable persisted chip", async () => {
    const references = [
      {
        id: "folder-1",
        conversationId: "conversation-1",
        folderPath: "/work/very/long/path/design-notes",
        displayName: "design-notes",
        createdAt: "2026-01-01T00:00:00Z",
      },
    ];
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_conversation_folder_references") return Promise.resolve(references);
      return Promise.resolve(undefined);
    });
    renderComposer({ conversationId: "conversation-1" });
    const chip = await screen.findByTestId(
      "agent-composer-reference-pill-folder:folder-1",
    );
    const pathTrigger = chip.querySelector<HTMLElement>('[tabindex="0"]');
    expect(pathTrigger).not.toBeNull();
    fireEvent.focus(pathTrigger!);
    const tooltip = await screen.findByRole("tooltip");
    expect(tooltip.textContent).toContain("/work/very/long/path/design-notes");
  });

  it("defers folder-reference hydration behind the first paint boundary", async () => {
    const deferredFrames = holdDeferredFrames();
    try {
      renderComposer({ conversationId: "conversation-1" });

      // Composer shell paints synchronously; the hydration query must not
      // have fired inside the same render/effect pass.
      expect(screen.getByLabelText("Message input")).toBeInTheDocument();
      expect(invoke).not.toHaveBeenCalledWith("list_conversation_folder_references", {
        conversationId: "conversation-1",
      });

      deferredFrames.flush();

      await waitFor(() =>
        expect(invoke).toHaveBeenCalledWith("list_conversation_folder_references", {
          conversationId: "conversation-1",
        }),
      );
    } finally {
      deferredFrames.restore();
    }
  });

  it("surfaces the folder cap rejection inline near the composer instead of a modal", async () => {
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "list_conversation_folder_references") return Promise.resolve([]);
      if (cmd === "add_conversation_folder_reference") {
        return Promise.reject(new Error("Maximum of 6 live folder references reached"));
      }
      return Promise.resolve(undefined);
    });
    vi.mocked(openDialog).mockResolvedValue("/work/one-too-many");
    renderComposer({ conversationId: "conversation-1", enableAttachments: true });
    fireEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    fireEvent.click(screen.getByRole("button", { name: "Add folder" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Maximum of 6 live folder references reached");
  });

  it("renders pre-send draft folder chips with an accessible, tooltip-backed remove control", async () => {
    const onRemoveFolder = vi.fn();
    const user = userEvent.setup();
    renderComposer({
      conversationId: null,
      folders: [
        { id: "draft-1", folderPath: "/work/draft-notes", displayName: "draft-notes" },
      ],
      onRemoveFolder,
    });

    expect(screen.getByText("draft-notes")).toBeInTheDocument();
    const remove = screen.getByRole("button", { name: "Remove folder draft-notes" });
    await user.hover(remove);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Remove folder");

    await user.click(remove);
    expect(onRemoveFolder).toHaveBeenCalledWith("draft-1");
  });

  it("does not render draft folder chips once removed (absence assertion)", () => {
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <TooltipProvider delayDuration={0}>
          <AgentComposerSurface
            project={{ value: "project-1", onValueChange: vi.fn(), options: [], placeholder: "Project" }}
            provider={{ value: "codex", onValueChange: vi.fn(), options: [] }}
            model={{ value: "gpt-5.5", onValueChange: vi.fn(), options: [] }}
            effort={{ value: "xhigh", onValueChange: vi.fn(), options: [] }}
            onSend={vi.fn()}
            folders={[{ id: "draft-1", folderPath: "/work/draft-notes", displayName: "draft-notes" }]}
            onRemoveFolder={vi.fn()}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    );
    expect(screen.getByText("draft-notes")).toBeInTheDocument();

    rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <TooltipProvider delayDuration={0}>
          <AgentComposerSurface
            project={{ value: "project-1", onValueChange: vi.fn(), options: [], placeholder: "Project" }}
            provider={{ value: "codex", onValueChange: vi.fn(), options: [] }}
            model={{ value: "gpt-5.5", onValueChange: vi.fn(), options: [] }}
            effort={{ value: "xhigh", onValueChange: vi.fn(), options: [] }}
            onSend={vi.fn()}
            folders={[]}
            onRemoveFolder={vi.fn()}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    );
    expect(screen.queryByText("draft-notes")).not.toBeInTheDocument();
    expect(screen.queryByTestId("draft-folder-reference-chips")).not.toBeInTheDocument();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("stages artifact selections as removable excerpt chips and sends them separately", async () => {
    const onSend = vi.fn();
    renderComposer({ conversationId: "conversation-1", onSend });

    act(() => {
      stageComposerExcerptReference("conversation-1", {
        sourceKind: "plan",
        sourceId: "artifact-1",
        sourceLabel: "Plan",
        title: "Release plan",
        excerpt: "Ship the native selection flow",
        artifactId: "artifact-1",
        version: 4,
      });
    });

    const chip = screen.getByTestId(
      "agent-composer-reference-pill-excerpt:plan:artifact-1",
    );
    expect(chip).toHaveTextContent("Plan excerpt");
    expect(chip).toHaveTextContent("Ship the native selection flow");

    fireEvent.change(screen.getByLabelText("Message input"), {
      target: { value: "Use this context" },
    });
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith("Use this context", {
      excerptReferences: [
        {
          sourceKind: "plan",
          sourceId: "artifact-1",
          sourceLabel: "Plan",
          title: "Release plan",
          excerpt: "Ship the native selection flow",
          artifactId: "artifact-1",
          version: 4,
        },
      ],
    });
    await waitFor(() => {
      expect(
        screen.queryByTestId(
          "agent-composer-reference-pill-excerpt:plan:artifact-1",
        ),
      ).not.toBeInTheDocument();
    });
  });

  it("retains staged excerpt references when sending fails", async () => {
    const onSend = vi.fn().mockRejectedValue(new Error("send failed"));
    renderComposer({ conversationId: "conversation-1", onSend });

    act(() => {
      stageComposerExcerptReference("conversation-1", {
        sourceKind: "task",
        sourceId: "clickup-task-1",
        sourceLabel: "ClickUp task",
        excerpt: "Preserve this acceptance detail",
      });
    });
    fireEvent.change(screen.getByLabelText("Message input"), {
      target: { value: "Use this context" },
    });
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    await waitFor(() => expect(onSend).toHaveBeenCalledTimes(1));
    expect(
      screen.getByTestId(
        "agent-composer-reference-pill-excerpt:task:clickup-task-1",
      ),
    ).toHaveTextContent("Preserve this acceptance detail");
  });

  it("sends staged excerpt references while answering an agent question", async () => {
    const onSend = vi.fn();
    renderComposer({
      conversationId: "conversation-1",
      onSend,
      questionMode: {
        optionCount: 1,
        multiSelect: false,
        onMatchedOptions: vi.fn(),
      },
    });

    act(() => {
      stageComposerExcerptReference("conversation-1", {
        sourceKind: "task",
        sourceId: "clickup-task-1",
        sourceLabel: "ClickUp task",
        excerpt: "Preserve this acceptance detail",
      });
    });
    fireEvent.change(screen.getByLabelText("Message input"), {
      target: { value: "Use this context" },
    });
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith("Use this context", {
      excerptReferences: [
        {
          sourceKind: "task",
          sourceId: "clickup-task-1",
          sourceLabel: "ClickUp task",
          excerpt: "Preserve this acceptance detail",
        },
      ],
    });
    await waitFor(() => {
      expect(
        screen.queryByTestId(
          "agent-composer-reference-pill-excerpt:task:clickup-task-1",
        ),
      ).not.toBeInTheDocument();
    });
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

  it("resets the complete runtime tuple from the quick dropdown", async () => {
    const onReset = vi.fn();
    renderComposer({
      runtimeDefault: {
        source: "project_ui",
        onReset,
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    const reset = screen.getByTestId("agent-composer-runtime-reset");
    expect(reset).toHaveAccessibleName(
      "Reset runtime to current role default",
    );
    fireEvent.click(reset);

    await waitFor(() => expect(onReset).toHaveBeenCalledTimes(1));
  });

  it("moves the optional persona control into the runtime selector", async () => {
    renderComposer({
      personaControl: <span data-testid="persona-control-slot">Persona</span>,
    });

    expect(screen.queryByTestId("persona-control-slot")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.click(
      await screen.findByTestId("agent-composer-runtime-persona-menu-trigger"),
    );
    expect(await screen.findByTestId("persona-control-slot")).toBeInTheDocument();
    expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveClass(
      "max-w-[34rem]",
    );
  });

  it("sends the selected capability as the provider-neutral intent", async () => {
    const onSend = vi.fn();
    const onValueChange = vi.fn();
    renderComposer({
      onSend,
      model: {
        value: "gpt-5.5",
        onValueChange: vi.fn(),
        options: [{ id: "gpt-5.5", label: "gpt-5.5" }],
        fastMode: {
          visible: true,
          value: false,
          onValueChange: vi.fn(),
        },
      },
      capability: {
        value: "rx_native_workflow",
        onValueChange,
        options: [
          { id: "solo", label: "Defaults" },
          { id: "rx_native_workflow", label: "Workflow" },
        ],
        testId: "agent-composer-capability",
      },
    });

    expect(
      screen.queryByRole("button", { name: /^Capabilities:/ }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    const orderedRows = [
      "agent-composer-runtime-provider-menu-trigger",
      "agent-composer-runtime-model-menu-trigger",
      "agent-composer-runtime-effort-menu-trigger",
      "agent-composer-runtime-capability-menu-trigger",
      "agent-composer-runtime-speed-menu-trigger",
    ].map((testId) => screen.getByTestId(testId));
    for (let index = 0; index < orderedRows.length - 1; index += 1) {
      expect(
        orderedRows[index]?.compareDocumentPosition(orderedRows[index + 1]!),
      ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    }
    fireEvent.click(screen.getByRole("button", { name: /^Capabilities,/ }));
    fireEvent.click(screen.getByTestId("agent-composer-capability-solo"));
    await waitFor(() => expect(onValueChange).toHaveBeenCalledWith("solo"));

    fireEvent.change(screen.getByLabelText("Message input"), {
      target: { value: "Run the migration workflow" },
    });
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith("Run the migration workflow", {
      capabilityIntent: { coordinationMode: "rx_native_workflow" },
    });
  });

  it("keeps a capability-only runtime menu reachable without optimistic selection", async () => {
    let settleSelection: (() => void) | undefined;
    const onValueChange = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          settleSelection = resolve;
        }),
    );
    renderComposer({
      provider: {
        value: "codex",
        onValueChange: vi.fn(),
        options: [{ id: "codex", label: "Codex" }],
        disabled: true,
      },
      model: {
        value: "",
        onValueChange: vi.fn(),
        options: [],
        disabled: true,
      },
      effort: {
        value: "",
        onValueChange: vi.fn(),
        options: [],
        disabled: true,
      },
      capability: {
        value: "solo",
        onValueChange,
        options: [
          { id: "solo", label: "Defaults" },
          { id: "rx_native_team", label: "Team" },
        ],
        testId: "agent-composer-capability",
      },
    });

    const runtimeTrigger = screen.getByTestId("agent-composer-runtime-pill");
    expect(runtimeTrigger).toHaveTextContent("Runtime settings");
    expect(runtimeTrigger).toHaveAccessibleName(/capabilities/i);
    fireEvent.click(runtimeTrigger);
    fireEvent.click(screen.getByRole("button", { name: /^Capabilities,/ }));
    fireEvent.click(
      screen.getByTestId("agent-composer-capability-rx_native_team"),
    );
    fireEvent.click(
      screen.getByTestId("agent-composer-capability-rx_native_team"),
    );

    expect(onValueChange).toHaveBeenCalledWith("rx_native_team");
    expect(onValueChange).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: /^Capabilities, Defaults/ })).toBeInTheDocument();
    expect(screen.getByTestId("agent-composer-runtime-capability-submenu")).toBeInTheDocument();

    settleSelection?.();
    await waitFor(() =>
      expect(
        screen.queryByTestId("agent-composer-runtime-capability-submenu"),
      ).not.toBeInTheDocument(),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /^Capabilities, Defaults/ }),
      ).toHaveFocus(),
    );
  });

  it("keeps a rejected capability selection open for retry", async () => {
    const onValueChange = vi.fn(() => Promise.reject(new Error("update failed")));
    renderComposer({
      capability: {
        value: "solo",
        onValueChange,
        options: [
          { id: "solo", label: "Defaults" },
          { id: "rx_native_team", label: "Team" },
          {
            id: "codex_native_ultra",
            label: "Ultra",
            disabled: true,
            disabledReason: "Ultra is unavailable for this model.",
          },
        ],
        testId: "agent-composer-capability",
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.click(screen.getByRole("button", { name: /^Capabilities,/ }));
    expect(
      screen.getByTestId("agent-composer-capability-codex_native_ultra"),
    ).toBeDisabled();
    expect(
      screen.getByText("Ultra is unavailable for this model."),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByTestId("agent-composer-capability-rx_native_team"),
    );

    await waitFor(() => expect(onValueChange).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("agent-composer-runtime-capability-submenu")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Capabilities, Defaults/ })).toBeInTheDocument();

    await waitFor(() =>
      expect(
        screen.getByTestId("agent-composer-capability-rx_native_team"),
      ).not.toBeDisabled(),
    );
    fireEvent.click(
      screen.getByTestId("agent-composer-capability-rx_native_team"),
    );
    await waitFor(() => expect(onValueChange).toHaveBeenCalledTimes(2));
  });

  it("returns from Persona after settlement and suppresses repeat selection", async () => {
    let settleSelection: (() => void) | undefined;
    const onValueChange = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          settleSelection = resolve;
        }),
    );
    renderComposer({
      persona: {
        value: "default",
        onValueChange,
        options: [
          { id: "default", label: "Default" },
          { id: "reviewer", label: "Reviewer" },
        ],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.click(screen.getByRole("button", { name: /^Persona, Default/ }));
    const reviewer = screen.getByTestId("agent-composer-runtime-persona-reviewer");
    fireEvent.click(reviewer);
    fireEvent.click(reviewer);

    expect(onValueChange).toHaveBeenCalledWith("reviewer");
    expect(onValueChange).toHaveBeenCalledTimes(1);
    expect(reviewer).toBeDisabled();
    expect(
      screen.getByTestId("agent-composer-runtime-persona-submenu"),
    ).toBeInTheDocument();

    settleSelection?.();
    await waitFor(() =>
      expect(
        screen.queryByTestId("agent-composer-runtime-persona-submenu"),
      ).not.toBeInTheDocument(),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /^Persona, Default/ }),
      ).toHaveFocus(),
    );
  });

  it("keeps a rejected Persona selection open for retry", async () => {
    const onValueChange = vi.fn(() =>
      Promise.reject(new Error("update failed")),
    );
    renderComposer({
      persona: {
        value: "default",
        onValueChange,
        options: [
          { id: "default", label: "Default" },
          { id: "reviewer", label: "Reviewer" },
        ],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.click(screen.getByRole("button", { name: /^Persona, Default/ }));
    fireEvent.click(
      screen.getByTestId("agent-composer-runtime-persona-reviewer"),
    );

    await waitFor(() => expect(onValueChange).toHaveBeenCalledTimes(1));
    expect(
      screen.getByTestId("agent-composer-runtime-persona-submenu"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByTestId("agent-composer-runtime-persona-reviewer"),
      ).not.toBeDisabled(),
    );
    fireEvent.click(
      screen.getByTestId("agent-composer-runtime-persona-reviewer"),
    );
    await waitFor(() => expect(onValueChange).toHaveBeenCalledTimes(2));
  });

  it("disables the integrated capability row while an update is pending", () => {
    renderComposer({
      capability: {
        value: "solo",
        onValueChange: vi.fn(),
        options: [
          { id: "solo", label: "Defaults" },
          { id: "rx_native_team", label: "Team" },
        ],
        pending: true,
        testId: "agent-composer-capability",
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));

    const capabilityRow = screen.getByRole("button", {
      name: /^Capabilities, Defaults/,
    });
    expect(capabilityRow).toBeDisabled();
    fireEvent.click(capabilityRow);
    expect(
      screen.queryByTestId("agent-composer-runtime-capability-submenu"),
    ).not.toBeInTheDocument();
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
    expect(screen.getByTestId("agent-composer-runtime-menu")).toHaveClass(
      "max-h-[min(38rem,var(--radix-popover-content-available-height))]",
    );
    expect(screen.getByTestId("agent-composer-runtime-menu-scroll")).toHaveClass(
      "overflow-y-auto",
      "overscroll-contain",
    );
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

  it("opens directly to the unified runtime rows and inline effort scale", () => {
    renderComposer({
      effort: {
        value: "high",
        onValueChange: vi.fn(),
        options: [
          { id: "low", label: "Low", description: "Fastest responses." },
          { id: "high", label: "High", description: "Greater depth." },
          { id: "max", label: "Maximum", description: "Deepest reasoning." },
        ],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));

    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();
    expect(screen.getByText("Advanced")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Provider,/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Model,/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Effort,/ })).toBeInTheDocument();
    expect(screen.getByRole("slider", { name: "Effort" })).toHaveAttribute(
      "aria-valuetext",
      "High",
    );
    expect(screen.getByText("Greater depth.")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agent-composer-runtime-model-gpt-5.5"),
    ).not.toBeInTheDocument();
  });

  it("returns successful wide runtime selections to Advanced", async () => {
    const onModelChange = vi.fn();
    const onEffortChange = vi.fn();
    const onFastModeChange = vi.fn();
    renderComposer({
      model: {
        value: "gpt-5.5",
        onValueChange: onModelChange,
        options: [
          { id: "gpt-5.5", label: "gpt-5.5", description: "Frontier model." },
          { id: "gpt-5.4", label: "gpt-5.4", description: "Strong model." },
        ],
        fastMode: {
          visible: true,
          value: false,
          onValueChange: onFastModeChange,
          description: "1.5x speed, more usage.",
        },
      },
      effort: {
        value: "high",
        onValueChange: onEffortChange,
        options: [
          { id: "low", label: "Low", description: "Lower latency." },
          { id: "high", label: "High", description: "Greater depth." },
        ],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();
    expect(screen.queryByText("Providers & models")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Provider,/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Model,/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Effort,/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Speed,/ })).toBeInTheDocument();
    expect(
      screen.queryByTestId("agent-composer-runtime-model-gpt-5.4"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^Back to/ }),
    ).not.toBeInTheDocument();

    fireEvent.pointerMove(screen.getByRole("button", { name: /^Provider,/ }));
    expect(
      screen.getByTestId("agent-composer-runtime-provider-submenu"),
    ).toBeInTheDocument();

    fireEvent.pointerMove(screen.getByRole("button", { name: /^Model,/ }));

    expect(
      screen.getByTestId("agent-composer-runtime-model-submenu"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByTestId("agent-composer-runtime-model-gpt-5.4"),
    );

    expect(onModelChange).toHaveBeenCalledWith("gpt-5.4");
    expect(
      screen.queryByTestId("agent-composer-runtime-model-submenu"),
    ).not.toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByTestId("agent-composer-runtime-model-menu-trigger"),
      ).toHaveFocus(),
    );

    fireEvent.pointerMove(screen.getByRole("button", { name: /^Effort,/ }));

    expect(
      screen.getByTestId("agent-composer-runtime-effort-submenu"),
    ).toBeInTheDocument();
    expect(screen.getByText("Lower latency.")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agent-composer-runtime-effort-low"));
    expect(onEffortChange).toHaveBeenCalledWith("low");
    expect(
      screen.queryByTestId("agent-composer-runtime-effort-submenu"),
    ).not.toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByTestId("agent-composer-runtime-effort-menu-trigger"),
      ).toHaveFocus(),
    );

    fireEvent.pointerMove(screen.getByRole("button", { name: /^Speed,/ }));
    expect(
      screen.getByTestId("agent-composer-runtime-speed-submenu"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agent-composer-runtime-speed-fast"));
    expect(onFastModeChange).toHaveBeenCalledWith(true);
    expect(
      screen.queryByTestId("agent-composer-runtime-speed-submenu"),
    ).not.toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByTestId("agent-composer-runtime-speed-menu-trigger"),
      ).toHaveFocus(),
    );

    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();
  });

  it("shows only the regular provider settings action in Advanced", () => {
    renderComposer({
      provider: {
        value: "codex",
        onValueChange: vi.fn(),
        options: [{ id: "codex", label: "Codex" }],
        footerAction: <button type="button">Open Provider Settings</button>,
        compactFooterAction: (
          <button type="button" aria-label="Compact Provider Settings" />
        ),
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.pointerMove(screen.getByRole("button", { name: /^Provider,/ }));

    expect(
      screen.getByRole("button", { name: "Open Provider Settings" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Compact Provider Settings" }),
    ).not.toBeInTheDocument();
  });

  it("uses a controlled icon-only Fast mode toggle in Quick view", () => {
    const onFastModeChange = vi.fn();
    renderComposer({
      model: {
        value: "gpt-5.5",
        onValueChange: vi.fn(),
        options: [{ id: "gpt-5.5", label: "gpt-5.5" }],
        fastMode: {
          visible: true,
          value: false,
          onValueChange: onFastModeChange,
          description: "Uses priority processing when available.",
          testId: "composer-codex-fast-mode",
        },
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    const fastToggle = screen.getByTestId("composer-codex-fast-mode");

    expect(fastToggle).toHaveAccessibleName("Turn Fast mode on");
    expect(fastToggle).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(fastToggle);
    expect(onFastModeChange).toHaveBeenCalledWith(true);
  });

  it("commits only option-backed effort values from slider keyboard actions", () => {
    const onEffortChange = vi.fn();
    renderComposer({
      effort: {
        value: "medium",
        onValueChange: onEffortChange,
        options: [
          { id: "quick", label: "Quick" },
          { id: "medium", label: "Balanced" },
          { id: "deep", label: "Deep" },
        ],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    const effortSlider = screen.getByRole("slider", { name: "Effort" });
    fireEvent.keyDown(effortSlider, { key: "ArrowRight" });
    fireEvent.keyDown(effortSlider, { key: "Home" });

    expect(onEffortChange.mock.calls).toEqual([["deep"], ["quick"]]);
  });

  it("previews disabled providers without mutating runtime or exposing effort controls", () => {
    const onProviderChange = vi.fn();
    renderComposer({
      provider: {
        value: "codex",
        onValueChange: onProviderChange,
        options: [
          { id: "codex", label: "Codex" },
          { id: "claude", label: "Claude", disabled: true },
        ],
        footerAction: <button type="button">Open provider Settings</button>,
      },
      effort: {
        value: "high",
        onValueChange: vi.fn(),
        options: [
          { id: "low", label: "Low" },
          { id: "high", label: "High" },
        ],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.pointerMove(screen.getByRole("button", { name: /^Provider,/ }));
    fireEvent.click(screen.getByTestId("agent-composer-runtime-provider-claude"));

    expect(onProviderChange).not.toHaveBeenCalled();
    expect(
      screen.getByTestId("agent-composer-runtime-provider-submenu"),
    ).toBeInTheDocument();
    expect(screen.getByText("Claude is not enabled")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^Effort/ }),
    ).not.toBeInTheDocument();
  });

  it("returns enabled provider selections to Advanced", async () => {
    const onProviderChange = vi.fn();
    renderComposer({
      provider: {
        value: "codex",
        onValueChange: onProviderChange,
        options: [
          { id: "codex", label: "Codex" },
          { id: "claude", label: "Claude" },
        ],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.pointerMove(screen.getByRole("button", { name: /^Provider,/ }));
    fireEvent.click(screen.getByTestId("agent-composer-runtime-provider-claude"));

    expect(onProviderChange).toHaveBeenCalledWith("claude");
    expect(
      screen.queryByTestId("agent-composer-runtime-provider-submenu"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();
    expect(
      screen.getByTestId("agent-composer-runtime-provider-menu-trigger"),
    ).toHaveTextContent("Codex");
    expect(
      screen.getByTestId("agent-composer-runtime-model-menu-trigger"),
    ).toHaveTextContent("gpt-5.5");
    await waitFor(() =>
      expect(
        screen.getByTestId("agent-composer-runtime-provider-menu-trigger"),
      ).toHaveFocus(),
    );
  });

  it("renders one coherent controlled runtime after an enabled provider commits", () => {
    function RuntimeHarness() {
      const [provider, setProvider] = useState<"claude" | "codex">("codex");
      const isClaude = provider === "claude";
      return (
        <AgentComposerSurface
          project={{
            value: "project-1",
            onValueChange: vi.fn(),
            options: [{ id: "project-1", label: "RalphX" }],
            placeholder: "Project",
          }}
          provider={{
            value: provider,
            onValueChange: setProvider,
            options: [
              { id: "codex", label: "Codex" },
              { id: "claude", label: "Claude" },
            ],
          }}
          model={{
            value: isClaude ? "sonnet" : "gpt-5.5",
            onValueChange: vi.fn(),
            options: isClaude
              ? [{ id: "sonnet", label: "Sonnet" }]
              : [{ id: "gpt-5.5", label: "gpt-5.5" }],
          }}
          effort={{
            value: isClaude ? "high" : "xhigh",
            onValueChange: vi.fn(),
            options: [
              { id: isClaude ? "high" : "xhigh", label: isClaude ? "High" : "Extra High" },
            ],
          }}
          mode={{
            value: "edit",
            onValueChange: vi.fn(),
            options: [{ id: "edit", label: "Agent" }],
          }}
          onSend={vi.fn()}
          actionTestId="agent-composer-submit"
        />
      );
    }

    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <TooltipProvider delayDuration={0}>
          <RuntimeHarness />
        </TooltipProvider>
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.pointerMove(screen.getByRole("button", { name: /^Provider,/ }));
    fireEvent.click(screen.getByTestId("agent-composer-runtime-provider-claude"));

    expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveTextContent("Sonnet");
    expect(screen.getByRole("button", { name: /^Provider,/ })).toHaveTextContent("Claude");
    expect(screen.getByRole("button", { name: /^Model,/ })).toHaveTextContent("Sonnet");
    fireEvent.pointerMove(screen.getByRole("button", { name: /^Model,/ }));
    expect(screen.getByTestId("agent-composer-runtime-model-sonnet")).toBeInTheDocument();
    expect(screen.queryByTestId("agent-composer-runtime-model-gpt-5.5")).not.toBeInTheDocument();
  });

  it("opens from the composer-scoped shortcut and resets nested state after closing", () => {
    renderComposer({
      model: {
        value: "gpt-5.5",
        onValueChange: vi.fn(),
        options: [
          { id: "gpt-5.5", label: "gpt-5.5" },
          { id: "gpt-5.4", label: "gpt-5.4" },
        ],
      },
    });

    const input = screen.getByLabelText("Message input");
    fireEvent.focus(input);
    fireEvent.keyDown(input, { key: "M", ctrlKey: true, shiftKey: true });
    fireEvent.click(screen.getByRole("button", { name: /^Model,/ }));
    expect(
      screen.getByTestId("agent-composer-runtime-model-gpt-5.4"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));

    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agent-composer-runtime-model-gpt-5.4"),
    ).not.toBeInTheDocument();
  });

  it("uses Back-based nested drill-ins only at the compact composer breakpoint", () => {
    const onModelChange = vi.fn();
    const rectSpy = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        const width = this.classList.contains("agent-composer-surface") ? 600 : 0;
        return {
          x: 0,
          y: 0,
          width,
          height: 0,
          top: 0,
          right: width,
          bottom: 0,
          left: 0,
          toJSON: () => ({}),
        };
      });
    renderComposer({
      model: {
        value: "gpt-5.5",
        onValueChange: onModelChange,
        options: [
          { id: "gpt-5.5", label: "gpt-5.5" },
          { id: "gpt-5.4", label: "gpt-5.4" },
        ],
        fastMode: {
          visible: true,
          value: false,
          onValueChange: vi.fn(),
        },
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));

    expect(screen.getByTestId("agent-composer-runtime-menu")).toHaveAttribute(
      "data-layout",
      "drill-in",
    );
    expect(screen.getByRole("button", { name: /^Model,/ })).toBeInTheDocument();
    expect(
      screen.queryByTestId("agent-composer-runtime-model-gpt-5.4"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Model,/ }));

    expect(
      screen.getByTestId("agent-composer-runtime-model-gpt-5.4"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agent-composer-runtime-model-gpt-5.4"));
    expect(onModelChange).toHaveBeenCalledWith("gpt-5.4");
    expect(
      screen.queryByTestId("agent-composer-runtime-model-submenu"),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Model,/ })).toBeInTheDocument();
    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Provider,/ }));
    expect(
      screen.getByTestId("agent-composer-runtime-provider-submenu"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Back to Advanced runtime settings" }),
    );
    expect(screen.getByRole("button", { name: /^Provider,/ })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Effort,/ }));
    expect(
      screen.getByTestId("agent-composer-runtime-effort-submenu"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Back to Advanced runtime settings" }),
    );
    expect(screen.getByRole("button", { name: /^Effort,/ })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Speed,/ }));
    expect(
      screen.getByTestId("agent-composer-runtime-speed-submenu"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Back to Advanced runtime settings" }),
    );
    expect(screen.getByRole("button", { name: /^Speed,/ })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Effort,/ }));
    fireEvent.keyDown(screen.getByTestId("agent-composer-runtime-effort-submenu"), {
      key: "Escape",
    });
    expect(screen.getByRole("button", { name: /^Model,/ })).toBeInTheDocument();

    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();
    rectSpy.mockRestore();
  });

  it("dismisses a wide child flyout before the unified root selector", () => {
    renderComposer({
      model: {
        value: "gpt-5.5",
        onValueChange: vi.fn(),
        options: [
          { id: "gpt-5.5", label: "gpt-5.5" },
          { id: "gpt-5.4", label: "gpt-5.4" },
        ],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.click(screen.getByRole("button", { name: /^Model,/ }));
    fireEvent.keyDown(screen.getByTestId("agent-composer-runtime-model-submenu"), {
      key: "Escape",
    });

    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agent-composer-runtime-model-submenu"),
    ).not.toBeInTheDocument();

    fireEvent.keyDown(screen.getByTestId("agent-composer-runtime-menu"), {
      key: "Escape",
    });

    expect(
      screen.queryByTestId("agent-composer-runtime-menu"),
    ).not.toBeInTheDocument();
  });

  it("omits an empty scale and explains the empty Effort state", () => {
    renderComposer({
      effort: {
        value: "",
        onValueChange: vi.fn(),
        options: [],
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    expect(screen.queryByRole("slider", { name: "Effort" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Effort,/ }));
    expect(
      screen.getByText("No effort options for this model"),
    ).toBeInTheDocument();
  });

  it("preserves custom model entry through the existing model callback", () => {
    const onModelChange = vi.fn();
    renderComposer({
      model: {
        value: "gpt-5.5",
        onValueChange: onModelChange,
        options: [{ id: "gpt-5.5", label: "gpt-5.5" }],
        allowCustomValue: true,
        customPlaceholder: "Custom model ID",
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.click(screen.getByRole("button", { name: /^Model,/ }));
    fireEvent.change(screen.getByPlaceholderText("Custom model ID"), {
      target: { value: "future-model" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Use" }));

    expect(onModelChange).toHaveBeenCalledWith("future-model");
    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agent-composer-runtime-model-submenu"),
    ).not.toBeInTheDocument();
  });

  it("hydrates custom model values and commits them from Enter", () => {
    const onModelChange = vi.fn();
    renderComposer({
      model: {
        value: "custom-current",
        onValueChange: onModelChange,
        options: [{ id: "gpt-5.5", label: "gpt-5.5" }],
        allowCustomValue: true,
        customPlaceholder: "Custom model ID",
      },
    });

    fireEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    fireEvent.click(screen.getByRole("button", { name: /^Model,/ }));

    const customInput = screen.getByPlaceholderText("Custom model ID");
    expect(customInput).toHaveValue("custom-current");
    fireEvent.change(customInput, { target: { value: "next-custom" } });
    fireEvent.keyDown(customInput, { key: "Enter" });

    expect(onModelChange).toHaveBeenCalledWith("next-custom");
    expect(
      screen.queryByTestId("agent-composer-runtime-model-submenu"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("agent-composer-runtime-menu")).toBeInTheDocument();
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

  it("keeps specialized modes behind the existing mode-menu disclosure", async () => {
    const user = userEvent.setup();
    renderComposer({
      mode: {
        value: "edit",
        onValueChange: vi.fn(),
        options: [
          { id: "plan", label: "Plan" },
          { id: "edit", label: "Agent" },
          { id: "review_pr", label: "Review PR" },
          { id: "chat", label: "Ask" },
          { id: "automation", label: "Automation" },
          { id: "persona_builder", label: "Persona" },
        ],
        secondaryOptionIds: ["automation", "persona_builder"],
        testId: "agent-mode",
      },
    });

    await user.click(screen.getByTestId("agent-mode-chip"));

    expect(screen.getByTestId("agent-mode-plan")).toBeInTheDocument();
    expect(screen.getByTestId("agent-mode-chat")).toHaveTextContent("Ask");
    expect(screen.queryByTestId("agent-mode-automation")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agent-mode-persona_builder")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Show more modes" }));

    expect(screen.getByTestId("agent-mode-automation")).toBeInTheDocument();
    expect(screen.getByTestId("agent-mode-persona_builder")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show fewer modes" })).toBeInTheDocument();
  });

  it("keeps a selected specialized mode visible before disclosure", async () => {
    const user = userEvent.setup();
    renderComposer({
      mode: {
        value: "automation",
        onValueChange: vi.fn(),
        options: [
          { id: "plan", label: "Plan" },
          { id: "edit", label: "Agent" },
          { id: "automation", label: "Automation" },
        ],
        secondaryOptionIds: ["automation"],
        testId: "agent-mode",
      },
    });

    await user.click(screen.getByTestId("agent-mode-chip"));

    expect(screen.getByTestId("agent-mode-automation")).toBeInTheDocument();
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

  /**
   * The Stop button used to `void onStop?.()`, discarding the promise: a rejected stop —
   * including a remote host answering `REMOTE_COMMAND_UNAVAILABLE` — produced no UI at all
   * while the agent kept running.
   */
  it("surfaces a failed stop instead of discarding the promise", async () => {
    const onStop = vi
      .fn()
      .mockRejectedValue(
        new Error("This action runs only on the host — it is not available remotely."),
      );
    renderComposer({ agentStatus: "generating", onStop });

    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(
        "Couldn't stop the agent",
        expect.objectContaining({
          description:
            "This action runs only on the host — it is not available remotely.",
        }),
      );
    });
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  /**
   * First paint wins: the pending state must flip in the click commit, BEFORE the await
   * settles, so the button reads as acted-on immediately.
   */
  it("flips the stop button to pending synchronously, before the stop settles", async () => {
    let settle: (() => void) | undefined;
    const onStop = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          settle = resolve;
        }),
    );
    renderComposer({ agentStatus: "generating", onStop });

    const button = screen.getByTestId("agent-composer-submit");
    fireEvent.click(button);
    expect(button).toBeDisabled();

    await act(async () => {
      settle?.();
    });
    await waitFor(() => expect(button).not.toBeDisabled());
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("does not toast when the stop succeeds", async () => {
    const onStop = vi.fn().mockResolvedValue(undefined);
    renderComposer({ agentStatus: "generating", onStop });

    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    await waitFor(() => expect(onStop).toHaveBeenCalledTimes(1));
    expect(toast.error).not.toHaveBeenCalled();
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
      fireEvent.click(await screen.findByRole("button", { name: label }));

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
    fireEvent.click(await screen.findByRole("button", { name: "ClickUp" }));

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

  it("appends internal skill directives for typed slash skill tokens", async () => {
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
    fireEvent.change(textarea, { target: { value: "Use /workspace-swe" } });
    textarea.setSelectionRange(
      "Use /workspace-swe".length,
      "Use /workspace-swe".length,
    );
    fireEvent.keyUp(textarea);

    await screen.findByTestId(
      "agent-composer-menu-item-skill:internal:workspace-swe",
    );
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    expect(onSend).toHaveBeenCalledWith(
      "Use /workspace-swe\n\n<!-- ralphx_internal_skill=workspace-swe -->",
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
    fireEvent.change(textarea, { target: { value: "Use /rev" } });
    textarea.setSelectionRange("Use /rev".length, "Use /rev".length);
    fireEvent.keyUp(textarea);

    const item = await screen.findByTestId(
      "agent-composer-menu-item-skill:claude:project:review",
    );
    fireEvent.mouseDown(item);
    fireEvent.click(item);

    expect(textarea.value).toBe("Use /review ");
  });

  it("does not open skill suggestions for dollar tokens", async () => {
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

    await waitFor(() => {
      expect(
        screen.queryByTestId("agent-composer-command-menu"),
      ).not.toBeInTheDocument();
    });
    expect(
      screen.queryByTestId(
        "agent-composer-menu-item-skill:claude:project:review",
      ),
    ).not.toBeInTheDocument();
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

    const item = await screen.findByTestId(
      "agent-composer-menu-item-skill:claude:project:review",
    );
    expect(item).toHaveTextContent("/review");
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(textarea.value).toBe("/review ");
  });

  it("invokes Codex-native skills from the slash menu with provider-native insertion", async () => {
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

    const item = await screen.findByTestId(
      "agent-composer-menu-item-skill:codex:global:plugin-creator",
    );
    expect(item).toHaveTextContent("/plugin-creator");
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
    it("uses the presented queue prop for ArrowUp editing and queue-caused expansion", () => {
      const onEditLastQueued = vi.fn();
      // With no queued message, ArrowUp falls through to persisted input
      // history. Remove that independent source of composer activity so this
      // test exercises the queue prop's effect on edit/expansion behavior.
      localStorage.removeItem("ralphx:composer-input-history");
      const { rerender } = renderComposer({
        dataTestId: "agent-composer",
        collapsible: true,
        hasQueuedMessages: false,
        onEditLastQueued,
      });

      const textarea = screen.getByLabelText("Message input");
      fireEvent.keyDown(textarea, { key: "ArrowUp" });
      expect(onEditLastQueued).not.toHaveBeenCalled();
      expect(screen.getByTestId("agent-composer")).toHaveAttribute(
        "data-collapsed",
        "true",
      );

      rerender(
        <QueryClientProvider client={new QueryClient()}>
          <TooltipProvider delayDuration={0}>
            <AgentComposerSurface
              project={{
                value: "project-1",
                onValueChange: vi.fn(),
                options: [],
                placeholder: "Project",
              }}
              provider={{ value: "codex", onValueChange: vi.fn(), options: [] }}
              model={{ value: "gpt-5.5", onValueChange: vi.fn(), options: [] }}
              effort={{ value: "xhigh", onValueChange: vi.fn(), options: [] }}
              mode={{ value: "edit", onValueChange: vi.fn(), options: [] }}
              onSend={vi.fn()}
              actionTestId="agent-composer-submit"
              dataTestId="agent-composer"
              collapsible
              hasQueuedMessages
              onEditLastQueued={onEditLastQueued}
            />
          </TooltipProvider>
        </QueryClientProvider>,
      );

      fireEvent.keyDown(screen.getByLabelText("Message input"), {
        key: "ArrowUp",
      });
      expect(onEditLastQueued).toHaveBeenCalledTimes(1);
      expect(screen.getByTestId("agent-composer")).toHaveAttribute(
        "data-collapsed",
        "false",
      );
    });

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

    it("resizes the visible composer without emitting a transcript layout signal", () => {
      let measuredScrollHeight = 96;
      const scrollHeightSpy = vi
        .spyOn(HTMLTextAreaElement.prototype, "scrollHeight", "get")
        .mockImplementation(() => measuredScrollHeight);
      try {
        renderComposer({
          dataTestId: "agent-composer",
          collapsible: false,
        });

        const textarea = screen.getByLabelText(
          "Message input",
        ) as HTMLTextAreaElement;
        expect(textarea.style.height).toBe("96px");

        measuredScrollHeight = 132;
        fireEvent.change(textarea, {
          target: { value: "line one\nline two\nline three" },
        });

        expect(textarea.style.height).toBe("132px");
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
      renderComposer({ dataTestId: "agent-composer", collapsible: true });
      const surface = screen.getByTestId("agent-composer");
      const textarea = screen.getByLabelText(
        "Message input",
      ) as HTMLTextAreaElement;

      fireEvent.focus(textarea);
      expect(surface).toHaveAttribute("data-collapsed", "false");

      // Blur with no text returns to the minimal resting state.
      fireEvent.blur(textarea);
      expect(surface).toHaveAttribute("data-collapsed", "true");
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

// ---------------------------------------------------------------------------
// P-20: unknown send outcome — refetch the transcript, never re-send
// ---------------------------------------------------------------------------

describe("unknown send outcome", () => {
  beforeEach(() => {
    vi.useRealTimers();
    mockComposerIntegrationAvailability();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  for (const code of ["REMOTE_TIMEOUT_UNKNOWN", "REMOTE_REQUEST_IN_PROGRESS"] as const) {
    it(`reconciles ${code} with one transcript refetch and zero re-sends`, async () => {
      const onSend = vi.fn().mockRejectedValue(
        new RemoteTransportError({
          code,
          message: "no answer",
          environmentId: LOCAL_ENVIRONMENT_ID,
        }),
      );
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const invalidate = vi.spyOn(queryClient, "invalidateQueries");

      renderComposer({ conversationId: "conversation-1", onSend }, queryClient);

      fireEvent.change(screen.getByRole("textbox"), {
        target: { value: "Maybe delivered" },
      });
      fireEvent.click(screen.getByTestId("agent-composer-submit"));

      await waitFor(() => expect(onSend).toHaveBeenCalledTimes(1));
      await waitFor(() =>
        expect(invalidate).toHaveBeenCalledWith({
          queryKey: chatKeys.conversation("conversation-1"),
        }),
      );
      expect(vi.mocked(toast.info)).toHaveBeenCalledWith(UNKNOWN_OUTCOME_MESSAGE);
      // The message stays sent-once: the host's transcript, not a second send, is
      // what settles whether it landed.
      expect(onSend).toHaveBeenCalledTimes(1);
    });
  }

  it("leaves other send failures to the existing inline banner", async () => {
    const onSend = vi.fn().mockRejectedValue(
      new RemoteTransportError({
        code: "REMOTE_FORBIDDEN",
        message: "scope narrowed",
        environmentId: LOCAL_ENVIRONMENT_ID,
      }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    renderComposer({ conversationId: "conversation-1", onSend }, queryClient);

    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "Refused" },
    });
    fireEvent.click(screen.getByTestId("agent-composer-submit"));

    await waitFor(() => expect(onSend).toHaveBeenCalledTimes(1));
    expect(
      await screen.findByTestId("agent-composer-remote-error"),
    ).toBeInTheDocument();
    expect(invalidate).not.toHaveBeenCalled();
    expect(vi.mocked(toast.info)).not.toHaveBeenCalled();
  });
});
