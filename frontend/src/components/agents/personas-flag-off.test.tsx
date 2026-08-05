import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PersonaChip } from "@/components/Chat/PersonaChip";
import { IntegratedChatPanel } from "@/components/Chat/IntegratedChatPanel";
import { TooltipProvider } from "@/components/ui/tooltip";
import SettingsDialog from "@/components/settings/SettingsDialog";
import { PersonasSection } from "@/components/settings/PersonasSection";
import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";

import { AgentsStartComposer } from "./AgentsStartComposer";
import { PersonaPickerControl } from "./PersonaPickerControl";

const featureFlags = vi.hoisted(() => ({ agentPersonas: false }));
const composerProps = vi.hoisted(() => ({
  hasPersonaControl: false,
  modeValue: null as string | null,
}));
const personaChipRendered = vi.hoisted(() => vi.fn());

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: () => ({ data: featureFlags }),
  useUpdateFeatureFlags: () => ({ mutate: vi.fn(), isPending: false }),
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: () => ({
    settings: { defaultProvider: "codex" },
    providers: [
      {
        provider: "codex",
        enabled: true,
        available: true,
        binaryFound: true,
        model: "gpt-5.5",
        effort: "xhigh",
        missingCoreExecFeatures: [],
        supportsFastMode: false,
        fastModeSupportedModels: [],
      },
    ],
    isLoading: false,
    isPlaceholderData: false,
  }),
}));

vi.mock("@/components/shared/BranchBasePicker", () => ({
  BranchBasePicker: () => <div data-testid="branch-base-picker" />,
}));

vi.mock("@/components/shared/branchBaseOptions", () => ({
  fallbackBranchBaseOptions: () => ({ options: [], selectedKey: "" }),
  loadBranchBaseOptions: vi.fn().mockResolvedValue([]),
  loadPullRequestBaseOptions: vi.fn().mockResolvedValue([]),
}));

vi.mock("./AgentComposerSurface", () => ({
  AgentComposerProjectLine: () => null,
  AgentComposerSurface: ({
    mode,
    onSend,
    persona,
  }: {
    mode?: { value: string };
    onSend: (message: string) => Promise<void>;
    persona?: {
      onValueChange: (value: string) => void;
      options: Array<{ id: string; label: string }>;
    };
  }) => {
    composerProps.hasPersonaControl = persona !== undefined;
    composerProps.modeValue = mode?.value ?? null;
    return (
      <div>
        {persona && (
          <>
            <button type="button" aria-label="Choose persona">
              Choose persona
            </button>
            {persona.options.map((option) => (
              <button
                key={option.id}
                type="button"
                role="menuitemradio"
                onClick={() => persona.onValueChange(option.id)}
              >
                {option.label}
              </button>
            ))}
          </>
        )}
        <button type="button" onClick={() => void onSend("flag-off submission")}>
          Start Agent
        </button>
      </div>
    );
  },
}));

vi.mock("./AgentProviderSettingsButton", () => ({
  AgentProviderSettingsButton: () => null,
}));

vi.mock("./PersonaChip", () => ({
  PersonaChip: () => {
    personaChipRendered();
    return <div data-testid="persona-chip" />;
  },
}));

const chatState = vi.hoisted(() => ({
  conversation: {
    id: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    agentMode: "chat",
    providerHarness: null,
    providerSessionId: null,
  },
}));

vi.mock("@/hooks/useChat", () => ({
  useChat: () => ({
    messages: { data: { messages: [], conversation: chatState.conversation }, isLoading: false },
    sendMessage: { mutateAsync: vi.fn(), isPending: false },
    conversations: { data: [], isLoading: false },
    switchConversation: vi.fn(),
    createConversation: vi.fn(),
  }),
  useConversation: () => ({ data: undefined, isLoading: false, error: null }),
  useConversationHistoryWindow: () => ({
    data: undefined,
    isLoading: false,
    isFetchingOlderMessages: false,
    hasOlderMessages: false,
    loadedStartIndex: 0,
    fetchOlderMessages: vi.fn(),
  }),
  useConversationTimelineWindow: () => ({
    data: undefined,
    isLoading: false,
    isFetchingOlderMessages: false,
    hasOlderMessages: false,
    loadedStartIndex: 0,
    fetchOlderMessages: vi.fn(),
  }),
  isOptimisticConversationId: () => false,
  getCachedConversationMessages: () => [],
  chatKeys: {
    all: ["chat"],
    conversationList: (type: string, id: string) => ["chat", "conversations", type, id],
    conversation: (id: string) => ["chat", "conversation", id],
    conversationHistory: (id: string) => ["chat", "conversation", id, "history"],
    conversationTimeline: (id: string) => ["chat", "conversation", id, "timeline"],
    agentRun: (id: string) => ["chat", "agentRun", id],
  },
}));

vi.mock("@/hooks/useTasks", () => ({
  useTasks: () => ({ data: [] }),
  taskKeys: {
    list: (projectId: string) => ["tasks", projectId],
    detail: (taskId: string) => ["task", taskId],
  },
}));

vi.mock("@/hooks/useChatPanelContext", () => ({
  useChatPanelContext: () => ({
    chatContext: { view: "kanban", projectId: "project-1" },
    storeContextKey: "project:project-1",
    currentContextType: "project",
    currentContextId: "project-1",
    activeConversationId: "conversation-1",
    streamingToolCalls: [],
    setStreamingToolCalls: vi.fn(),
    streamingContentBlocks: [],
    setStreamingContentBlocks: vi.fn(),
    streamingTasks: new Map(),
    setStreamingTasks: vi.fn(),
    isFinalizing: false,
    setIsFinalizing: vi.fn(),
    autoSelectConversation: vi.fn(),
  }),
}));

vi.mock("@/hooks/useChatActions", () => ({
  useChatActions: () => ({
    handleSend: vi.fn(),
    handleEditLastQueued: vi.fn(),
    handleDeleteQueuedMessage: vi.fn(),
    handleEditQueuedMessage: vi.fn(),
    handleStopAgent: vi.fn(),
  }),
}));

vi.mock("@/hooks/useChatEvents", () => ({ useChatEvents: vi.fn() }));
vi.mock("@/hooks/useChatRecovery", () => ({
  useChatRecovery: () => ({ isStreamingHydrated: true }),
}));
vi.mock("@/hooks/useAgentEvents", () => ({ useAgentEvents: vi.fn() }));
vi.mock("@/hooks/useAskUserQuestion", () => ({
  useAskUserQuestion: () => ({
    activeQuestion: null,
    answeredQuestion: undefined,
    submitAnswer: vi.fn(),
    dismissQuestion: vi.fn(),
    clearAnswered: vi.fn(),
    isLoading: false,
  }),
}));
vi.mock("@/hooks/useQuestionInput", () => ({
  useQuestionInput: () => ({
    selectedOptions: new Set(),
    questionInputValue: "",
    setQuestionInputValue: vi.fn(),
    handleChipClick: vi.fn(),
    handleMatchedOptions: vi.fn(),
    handleQuestionSend: vi.fn(),
    handleQuestionSkip: vi.fn(),
    handleQuestionOptionSubmit: vi.fn(),
  }),
}));
vi.mock("@/hooks/useChatAttachments", () => ({
  useChatAttachments: () => ({
    attachments: [],
    uploadFiles: vi.fn(),
    removeAttachment: vi.fn(),
    clearAttachments: vi.fn(),
    uploading: false,
    uploadProgress: [],
  }),
}));
vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({ subscribe: vi.fn(() => vi.fn()), emit: vi.fn() }),
}));
vi.mock("@/api/chat", () => ({
  chatApi: {
    listConversations: vi.fn().mockResolvedValue([]),
    getConversationStats: vi.fn().mockResolvedValue(null),
    getAgentRunStatus: vi.fn().mockResolvedValue(null),
    // Wave B3 added queue hydration; keep this flag-off test on the local queue path.
    getQueuedAgentMessages: vi.fn().mockResolvedValue([]),
    sendAgentMessage: vi.fn(),
  },
  stopAgent: vi.fn(),
}));
vi.mock("@/components/recovery/RecoveryPromptDialog", () => ({
  RecoveryPromptDialog: () => null,
}));

const personas = vi.hoisted(() => [
  {
    id: "reviewer",
    slug: "reviewer-voice",
    name: "Reviewer Voice",
    description: "Careful reviews",
    content: "# Reviewer",
    status: "active" as const,
    version: 1,
    projectId: null,
    contentHash: "hash-reviewer",
    sourceSessionId: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  },
  {
    id: "draft",
    slug: "draft-voice",
    name: "Draft Voice",
    description: "Draft",
    content: "# Draft",
    status: "draft" as const,
    version: 1,
    projectId: null,
    contentHash: "hash-draft",
    sourceSessionId: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  },
]);

vi.mock("@/hooks/usePersonas", () => ({
  personaKeys: { list: () => ["personas", "list"] },
  fetchPersonas: vi.fn().mockResolvedValue(personas),
  usePersonas: () => ({ data: personas, isLoading: false }),
  usePersona: () => ({ data: personas[1], isLoading: false }),
  useCreatePersonaDraft: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useUpdatePersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useApprovePersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useArchivePersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useDeletePersonaDraft: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useSwitchConversationPersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
  usePersonaOverlayPreview: () => ({
    isPending: true,
    isError: false,
    data: undefined,
    error: null,
  }),
  usePersonaUsage: () => ({ data: [], isLoading: false, isError: false }),
  useUnarchivePersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useReseedPersonaDraft: () => ({
    mutateAsync: vi.fn(),
    isPending: false,
    error: null,
  }),
}));

vi.mock("@/hooks/usePersonaDraftEvents", () => ({
  usePersonaDraftEvents: () => "draft",
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: vi.fn(),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

const PERSONA_COMMANDS = new Set([
  "list_personas",
  "get_persona",
  "create_persona_draft",
  "update_persona",
  "approve_persona",
  "archive_persona",
  "delete_persona_draft",
  "switch_agent_conversation_persona",
]);

function createQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
}

function renderWithProviders(ui: React.ReactElement) {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider delayDuration={0}>{ui}</TooltipProvider>
    </QueryClientProvider>,
  );
}

function expectNoPersonaInvokes() {
  const personaInvokes = vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => PERSONA_COMMANDS.has(command));
  expect(personaInvokes).toEqual([]);
}

describe("agent personas flag-off sweep", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    featureFlags.agentPersonas = false;
    composerProps.hasPersonaControl = false;
    composerProps.modeValue = null;
    localStorage.clear();
    act(() => {
      useUiStore.setState({ activeModal: "settings", modalContext: { section: "personas" } });
      useAgentSessionStore.setState({ startConversationFailure: null });
      useChatStore.setState({ composerDraftsByKey: {} });
    });
  });

  it("keeps every persona entry point inert and omits personaId when the flag is off", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const settings = renderWithProviders(
      <SettingsDialog
        executionSettings={DEFAULT_PROJECT_SETTINGS}
        isLoadingSettings={false}
        isSavingSettings={false}
        settingsError={null}
        onSettingsChange={vi.fn()}
      />,
    );

    // Personas is a leaf tab under the Agents nav entry.
    await userEvent.setup().click(screen.getByRole("button", { name: "Agents" }));
    expect(screen.getByRole("tab", { name: "Personas" })).toBeInTheDocument();
    expect(screen.queryByText("Build with agent")).not.toBeInTheDocument();
    settings.unmount();

    renderWithProviders(
      <AgentsStartComposer
        projects={[{ id: "project-1", name: "Project", workingDirectory: "/tmp/project" }]}
        defaultProjectId="project-1"
        defaultRuntime={{ provider: "codex", modelId: "gpt-5.5", effort: "xhigh" }}
        isLoadingProjects={false}
        isSubmitting={false}
        modelRegistry={{
          claude: [],
          codex: [{ id: "gpt-5.5", label: "gpt-5.5", menuLabel: "gpt-5.5", defaultEffort: "xhigh", supportedEfforts: ["xhigh"] }],
        }}
        onSubmit={onSubmit}
      />,
    );

    expect(screen.queryByRole("button", { name: "Choose persona" })).not.toBeInTheDocument();
    expect(composerProps.hasPersonaControl).toBe(false);

    await userEvent.setup().click(screen.getByRole("button", { name: "Start Agent" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onSubmit.mock.calls[0]?.[0]).not.toHaveProperty("personaId");

    renderWithProviders(<IntegratedChatPanel projectId="project-1" selectedTaskIdOverride={null} />);
    expect(screen.queryByTestId("persona-chip")).not.toBeInTheDocument();
    expect(personaChipRendered).not.toHaveBeenCalled();
    expectNoPersonaInvokes();
  });

  it("keeps project mode while selecting the first available project", async () => {
    renderWithProviders(
      <AgentsStartComposer
        projects={[
          {
            id: "project-1",
            name: "Project",
            workingDirectory: "/tmp/project",
          },
        ]}
        defaultProjectId={null}
        defaultRuntime={{
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        }}
        isLoadingProjects={false}
        isSubmitting={false}
        modelRegistry={{
          claude: [],
          codex: [
            {
              id: "gpt-5.5",
              label: "gpt-5.5",
              menuLabel: "gpt-5.5",
              defaultEffort: "xhigh",
              supportedEfforts: ["xhigh"],
            },
          ],
        }}
        onSubmit={vi.fn()}
      />,
    );

    await waitFor(() => expect(composerProps.modeValue).toBe("edit"));
    expect(
      screen.queryByText(
        /Project-requiring modes are unavailable without a project/,
      ),
    ).not.toBeInTheDocument();
  });
});

describe("agent personas A18 icon-only controls", () => {
  beforeEach(() => {
    featureFlags.agentPersonas = true;
  });

  it("gives every persona icon-only control an aria-label and an app tooltip", async () => {
    const user = userEvent.setup();
    const picker = renderWithProviders(
      <PersonaPickerControl
        currentProjectId="project-1"
        currentProjectName="Project One"
        personaId="reviewer"
        onValueChange={vi.fn()}
        onOpenPersonas={vi.fn()}
      />,
    );
    const pickerTrigger = screen.getByRole("button", { name: "Choose persona" });
    expect(pickerTrigger).toHaveAttribute("aria-label", "Choose persona");
    await user.hover(pickerTrigger);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Persona: Reviewer Voice");
    picker.unmount();

    const chip = renderWithProviders(
      <PersonaChip conversationId="conversation-1" personaId="reviewer" isAgentRunning={false} />,
    );
    const chipTrigger = screen.getByRole("button", { name: "Switch conversation persona" });
    expect(chipTrigger).toHaveAttribute("aria-label", "Switch conversation persona");
    await user.hover(chipTrigger);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Applies to this conversation only");
    chip.unmount();

    const rows = renderWithProviders(<PersonasSection />);
    const archive = screen.getByRole("button", { name: "Archive Reviewer Voice" });
    const removeDraft = screen.getByRole("button", { name: "Delete Draft Voice" });
    expect(archive).toHaveAttribute("aria-label", "Archive Reviewer Voice");
    expect(removeDraft).toHaveAttribute("aria-label", "Delete Draft Voice");
    await user.hover(removeDraft);
    expect(await screen.findByRole("tooltip", { name: "Delete Draft Voice" })).toBeInTheDocument();
    rows.unmount();

    const archiveRows = renderWithProviders(<PersonasSection />);
    const archiveAction = screen.getByRole("button", { name: "Archive Reviewer Voice" });
    await user.hover(archiveAction);
    expect(await screen.findByRole("tooltip", { name: "Archive Reviewer Voice" })).toBeInTheDocument();
    archiveRows.unmount();

  });
});

describe("agent persona start retries", () => {
  beforeEach(() => {
    featureFlags.agentPersonas = true;
  });

  it("removes an unavailable persona before retrying and shows the retry failure", async () => {
    const onSubmit = vi.fn()
      .mockRejectedValueOnce(new Error("[Persona unavailable: Reviewer Voice is archived]"))
      .mockRejectedValueOnce(new Error("Retry could not start the agent"));
    renderWithProviders(
      <AgentsStartComposer
        projects={[{ id: "project-1", name: "Project", workingDirectory: "/tmp/project" }]}
        defaultProjectId="project-1"
        defaultRuntime={{ provider: "codex", modelId: "gpt-5.5", effort: "xhigh" }}
        isLoadingProjects={false}
        isSubmitting={false}
        modelRegistry={{
          claude: [],
          codex: [{ id: "gpt-5.5", label: "gpt-5.5", menuLabel: "gpt-5.5", defaultEffort: "xhigh", supportedEfforts: ["xhigh"] }],
        }}
        onSubmit={onSubmit}
      />,
    );

    await userEvent.setup().click(screen.getByRole("button", { name: "Choose persona" }));
    await userEvent.setup().click(screen.getByRole("menuitemradio", { name: /^Reviewer Voice/ }));
    await userEvent.setup().click(screen.getByRole("button", { name: "Start Agent" }));

    expect(await screen.findByTestId("persona-unavailable-notice")).toHaveTextContent(
      "Reviewer Voice is archived",
    );
    await userEvent.setup().click(screen.getByRole("button", { name: "Remove persona and retry" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(2));
    expect(onSubmit.mock.calls[1]?.[0]).toMatchObject({ personaId: null });
    expect(await screen.findByText("Retry could not start the agent")).toBeInTheDocument();
  });
});
