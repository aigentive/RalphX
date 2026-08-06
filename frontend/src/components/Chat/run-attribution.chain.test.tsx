/**
 * Run-attribution lifecycle chain.
 *
 * This mounts the real panel composition — IntegratedChatPanel →
 * useChatPanelContext → useChat/timeline queries → ChatMessageList — against
 * the real `@/api/chat` timeline transform and the real EventProvider bus.
 * Only peripheral panel controls, the virtualizer, and non-timeline transport
 * are replaced, so `run_id` / `finalized_at` reach the widget the same way the
 * backend delivers them: raw timeline row → transformTimelineItem → asMessage
 * → ChatMessageList anchor. Nothing here fabricates `runId`, `finalizedAt`, or
 * the store context key.
 */

import { act, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import userEvent from "@testing-library/user-event";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { EventProvider, useEventBus } from "@/providers/EventProvider";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import type { EventBus } from "@/lib/event-bus";

import { IntegratedChatPanel } from "./IntegratedChatPanel";

const CONVERSATION_ID = "conversation-1";
const PROJECT_ID = "project-1";
/** Production Agents panels key project conversations by conversation id. */
const STORE_CONTEXT_KEY = `project:${CONVERSATION_ID}`;
const RUN_ID = "run-review";

const { transport } = vi.hoisted(() => ({
  transport: {
    listConversations: vi.fn(),
    getConversationMessagesPage: vi.fn(),
    getAgentRunStatus: vi.fn(),
    isAgentRunning: vi.fn(),
    getConversationActiveState: vi.fn(),
    getConversationStats: vi.fn(),
    listMessageAttachments: vi.fn(),
  },
}));

// getConversationTimelinePage is deliberately NOT stubbed: the production
// invoke → Zod → transform path is what this chain test exists to cover.
vi.mock("@/api/chat", async (importActual) => {
  const actual = await importActual<typeof import("@/api/chat")>();
  return { ...actual, chatApi: { ...actual.chatApi, ...transport } };
});

vi.mock("@/hooks/useTasks", () => ({
  useTasks: () => ({ data: [] }),
  taskKeys: {
    list: (projectId: string) => ["tasks", projectId],
    detail: (taskId: string) => ["task", taskId],
  },
}));
vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: () => ({ data: { agentPersonas: false, activityPage: false } }),
}));
vi.mock("@/hooks/usePersonas", () => ({
  usePersonas: () => ({ data: [] }),
  useSwitchConversationPersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
}));
vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({ confirm: vi.fn(), confirmationDialogProps: {}, ConfirmationDialog: () => null }),
}));
vi.mock("@/hooks/usePersonaRunEvents", () => ({ usePersonaRunEvents: vi.fn() }));
vi.mock("@/hooks/useQueuedMessagesHydration", () => ({ useQueuedMessagesHydration: vi.fn() }));
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
vi.mock("@/hooks/useMessageAttachments", () => ({ useMessageAttachments: () => ({ data: new Map() }) }));
vi.mock("@/components/recovery/RecoveryPromptDialog", () => ({ RecoveryPromptDialog: () => null }));
vi.mock("./ChildSessionTranscriptModal", () => ({ ChildSessionTranscriptModal: () => null }));

vi.mock("react-virtuoso", async () => {
  const React = await import("react");
  return {
    Virtuoso: React.forwardRef(function ChainVirtuoso(
      props: {
        data?: unknown[];
        firstItemIndex?: number;
        itemContent?: (index: number, item: unknown) => React.ReactNode;
        rangeChanged?: (range: { startIndex: number; endIndex: number }) => void;
        scrollerRef?: (element: HTMLElement | null) => void;
        components?: { Header?: React.ComponentType };
      },
      ref,
    ) {
      const rootRef = React.useRef<HTMLDivElement | null>(null);
      React.useImperativeHandle(ref, () => ({ scrollToIndex: vi.fn() }));
      React.useEffect(() => {
        props.scrollerRef?.(rootRef.current);
        const length = props.data?.length ?? 0;
        if (length > 0) props.rangeChanged?.({ startIndex: 0, endIndex: length - 1 });
        return () => props.scrollerRef?.(null);
      }, [props]);
      const Header = props.components?.Header;
      return (
        <div ref={rootRef} data-testid="chain-virtuoso">
          {Header ? <Header /> : null}
          {(props.data ?? []).map((item, index) => (
            <div key={index}>{props.itemContent?.((props.firstItemIndex ?? 0) + index, item)}</div>
          ))}
        </div>
      );
    }),
  };
});

const conversation = {
  id: CONVERSATION_ID,
  contextType: "project" as const,
  contextId: PROJECT_ID,
  claudeSessionId: null,
  providerSessionId: "provider-session-1",
  providerHarness: "codex",
  coordinationMode: "solo" as const,
  title: "Review workspace",
  messageCount: 2,
  lastMessageAt: "2026-07-31T00:00:45Z",
  createdAt: "2026-07-31T00:00:00Z",
  updatedAt: "2026-07-31T00:00:45Z",
};

/** Backend wire shape: snake_case rows carrying run_id and finalized_at. */
function rawTimelineRow(overrides: Record<string, unknown>) {
  return {
    conversation_id: CONVERSATION_ID,
    run_id: RUN_ID,
    role: "assistant",
    kind: "message",
    status: "finalized",
    content_blocks: [],
    tool_call: null,
    metadata: null,
    provider_harness: "codex",
    provider_session_id: "provider-session-1",
    upstream_provider: "openai",
    provider_profile: null,
    logical_model: "gpt-5.6",
    effective_model_id: "gpt-5.6",
    logical_effort: "high",
    effective_effort: "high",
    input_tokens: null,
    output_tokens: null,
    cache_creation_tokens: null,
    cache_read_tokens: null,
    estimated_usd: null,
    usage_provenance: null,
    ...overrides,
  };
}

function rawTimelinePage() {
  return {
    conversation: {
      id: CONVERSATION_ID,
      context_type: "project",
      context_id: PROJECT_ID,
      claude_session_id: null,
      provider_session_id: "provider-session-1",
      provider_harness: "codex",
      title: "Review workspace",
      message_count: 2,
      last_message_at: "2026-07-31T00:00:45Z",
      created_at: "2026-07-31T00:00:00Z",
      updated_at: "2026-07-31T00:00:45Z",
    },
    items: [
      rawTimelineRow({
        id: "timeline-provider-1",
        message_id: "message-provider-1",
        sequence: 1,
        block_index: 0,
        content: "Starting the workspace review.",
        created_at: "2026-07-31T00:00:40Z",
        updated_at: "2026-07-31T00:00:44Z",
        finalized_at: "2026-07-31T00:00:44Z",
      }),
      rawTimelineRow({
        id: "timeline-provider-2",
        message_id: "message-provider-2",
        sequence: 2,
        block_index: 0,
        content: "The review is complete.",
        created_at: "2026-07-31T00:00:42Z",
        updated_at: "2026-07-31T00:00:45Z",
        finalized_at: "2026-07-31T00:00:45Z",
      }),
    ],
    limit: 40,
    before_sequence: null,
    total_item_count: 2,
    has_older: false,
    oldest_loaded_sequence: 1,
    newest_loaded_sequence: 2,
  };
}

function settledAttribution() {
  return [{
    id: RUN_ID,
    conversation_id: CONVERSATION_ID,
    status: "completed",
    started_at: "2026-07-31T00:01:00Z",
    completed_at: "2026-07-31T00:02:30Z",
    harness: "codex",
    upstream_provider: "openai",
    provider_profile: null,
    provider_session_id: "provider-session-1",
    logical_model: "gpt-5.6",
    effective_model_id: "gpt-5.6",
    logical_effort: "high",
    effective_effort: "high",
    service_tier: "fast",
    approval_policy: "never",
    sandbox_mode: "danger-full-access",
    input_tokens: 21,
    output_tokens: 34,
    cache_creation_tokens: null,
    cache_read_tokens: null,
    estimated_usd: 0.0123,
    run_chain_id: "chain-1",
    action_kind: "workspace_review",
    persona_slug: null,
    agent_name: "ralphx-workspace-reviewer",
    launch_role: "workspace_reviewer",
    runtime_source: "role_default",
  }];
}

/**
 * Process truth for the liveness reconciler. Production clears the active run
 * when `is_agent_running` disagrees with local state, so the chain test has to
 * move this the same way a real run does.
 */
function setProcessLiveness(running: boolean) {
  transport.isAgentRunning.mockResolvedValue(running);
  transport.getAgentRunStatus.mockResolvedValue({
    id: RUN_ID,
    conversationId: CONVERSATION_ID,
    status: running ? "running" : "completed",
    startedAt: "2026-07-31T00:01:00Z",
    completedAt: running ? null : "2026-07-31T00:02:30Z",
    errorMessage: null,
    modelId: null,
    modelLabel: null,
  });
}

function deferred<T>() {
  let resolve: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve: (value: T) => resolve(value) };
}

function EventBusCapture({ onReady }: { onReady: (eventBus: EventBus) => void }) {
  const eventBus = useEventBus();
  useEffect(() => {
    onReady(eventBus);
  }, [eventBus, onReady]);
  return null;
}

function renderPanel(onEventBusReady: (eventBus: EventBus) => void) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: Infinity },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <EventProvider>
          <EventBusCapture onReady={onEventBusReady} />
          <IntegratedChatPanel
            projectId={PROJECT_ID}
            contextTypeOverride="project"
            contextIdOverride={PROJECT_ID}
            conversationIdOverride={CONVERSATION_ID}
            storeContextKeyOverride={STORE_CONTEXT_KEY}
            hideHeaderSessionControls
            hideSessionToolbar
            renderComposer={() => null}
          />
        </EventProvider>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("run attribution lifecycle chain", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    transport.listConversations.mockResolvedValue([conversation]);
    transport.getConversationMessagesPage.mockResolvedValue({
      conversation,
      messages: [],
      totalMessageCount: 0,
      offset: 0,
      limit: 40,
      hasOlder: false,
    });
    transport.getAgentRunStatus.mockResolvedValue(null);
    transport.isAgentRunning.mockResolvedValue(false);
    transport.getConversationActiveState.mockResolvedValue({
      is_active: false,
      tool_calls: [],
      streaming_tasks: [],
      partial_text: "",
    });
    transport.getConversationStats.mockResolvedValue(null);
    transport.listMessageAttachments.mockResolvedValue([]);

    act(() => {
      useChatStore.setState({
        activeConversationIds: {},
        agentStatus: {},
        activeAgentRunIds: {},
        activeAgentRunHarnesses: {},
        activeAgentRunMeta: {},
        agentActivityLabels: {},
        isSending: {},
        queuedMessages: {},
        effectiveModel: {},
        delegateExpansionByConversation: {},
      });
      useUiStore.setState({ selectedTaskId: null, taskHistoryState: null });
    });
  });

  afterEach(() => {
    act(() => {
      useChatStore.setState({
        activeConversationIds: {},
        agentStatus: {},
        activeAgentRunIds: {},
        activeAgentRunHarnesses: {},
        activeAgentRunMeta: {},
        agentActivityLabels: {},
      });
    });
  });

  it("suppresses the widget while the run is live, then renders persisted timing and the settled attribution", async () => {
    let eventBus: EventBus | null = null;
    const pendingAttribution = deferred<ReturnType<typeof settledAttribution>>();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_agent_conversation_timeline_page") {
        return Promise.resolve(rawTimelinePage());
      }
      if (command === "get_agent_run_attributions") {
        return pendingAttribution.promise;
      }
      return Promise.resolve(undefined);
    });

    setProcessLiveness(true);
    renderPanel((bus) => {
      eventBus = bus;
    });
    await waitFor(() => expect(eventBus).not.toBeNull());

    // The run is live: production keys it by conversation id, the same slot the
    // panel derives, so the settled-run widget must stay hidden.
    await act(async () => {
      eventBus?.emit("agent:run_started", {
        run_id: RUN_ID,
        context_type: "project",
        context_id: PROJECT_ID,
        conversation_id: CONVERSATION_ID,
        // Real-clock timestamp: the typing indicator's elapsed counter runs off
        // the actual system clock (no fake timers), so the anchor must be close
        // to "now" for the `/\d+s/` assertion below to hold.
        started_at: new Date().toISOString(),
        agent_name: "ralphx-workspace-reviewer",
        launch_role: "workspace_reviewer",
      });
    });
    await waitFor(() => {
      expect(useChatStore.getState().activeAgentRunIds[STORE_CONTEXT_KEY]).toBe(RUN_ID);
      expect(screen.getByText("The review is complete.")).toBeInTheDocument();
    });
    expect(screen.queryByTestId("run-attribution-widget")).not.toBeInTheDocument();

    // The live typing indicator proves `TypingIndicator storeKey={contextKey}`
    // is actually wired through the real panel, with a role verb sourced from
    // the emitted `agent:run_started` payload, not a hand-passed storeKey.
    const typingIndicator = await screen.findByTestId("chat-typing-indicator");
    expect(typingIndicator).toHaveTextContent(/Reviewer working/);
    await waitFor(() => {
      expect(screen.getByTestId("chat-typing-indicator")).toHaveTextContent(/Reviewer working for \d+s/);
    });

    // A completion for an unrelated run must not settle this one.
    await act(async () => {
      eventBus?.emit("agent:run_completed", {
        run_id: "run-stale",
        context_type: "project",
        context_id: PROJECT_ID,
        conversation_id: CONVERSATION_ID,
        status: "completed",
      });
    });
    expect(useChatStore.getState().activeAgentRunIds[STORE_CONTEXT_KEY]).toBe(RUN_ID);
    expect(screen.queryByTestId("run-attribution-widget")).not.toBeInTheDocument();

    setProcessLiveness(false);
    await act(async () => {
      eventBus?.emit("agent:run_completed", {
        run_id: RUN_ID,
        context_type: "project",
        context_id: PROJECT_ID,
        conversation_id: CONVERSATION_ID,
        status: "completed",
      });
    });

    // Exactly one widget, anchored on the run's last provider row. Before the
    // attribution batch resolves the label comes from persisted timeline
    // timing: created_at of the first row → finalized_at of the last row (5s).
    const toggle = await screen.findByTestId("run-attribution-toggle");
    expect(screen.getAllByTestId("run-attribution-widget")).toHaveLength(1);
    expect(toggle).toHaveAttribute("data-chat-run-attribution-key", RUN_ID);
    expect(toggle).toHaveTextContent("Agent worked for 5s");
    expect(invoke).toHaveBeenCalledWith("get_agent_run_attributions", { runIds: [RUN_ID] });
    // Settling pins the live-versus-settled handoff: the typing indicator is
    // gone once the widget takes over.
    expect(screen.queryByTestId("chat-typing-indicator")).not.toBeInTheDocument();

    await act(async () => {
      pendingAttribution.resolve(settledAttribution());
      await pendingAttribution.promise;
    });

    await waitFor(() => expect(toggle).toHaveTextContent("Reviewer worked for 1m 30s"));

    const user = userEvent.setup();
    await user.click(toggle);
    const panel = await screen.findByTestId("run-attribution-panel");
    expect(within(panel).getByText("Agent").parentElement).toHaveTextContent("ralphx-workspace-reviewer");
    expect(within(panel).getByText("Role").parentElement).toHaveTextContent("workspace_reviewer");
    expect(within(panel).getByText("Trigger").parentElement).toHaveTextContent("workspace_review");
    expect(within(panel).getByText("Runtime source").parentElement).toHaveTextContent("Role default");
    expect(within(panel).getByText("Timing").parentElement).toHaveTextContent("1m 30s");
  });

  it("renders nothing and fetches no attribution for legacy rows without a run id", async () => {
    let eventBus: EventBus | null = null;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_agent_conversation_timeline_page") {
        const page = rawTimelinePage();
        return Promise.resolve({
          ...page,
          items: page.items.map((item) => ({ ...item, run_id: null })),
        });
      }
      return Promise.resolve(undefined);
    });

    renderPanel((bus) => {
      eventBus = bus;
    });
    await waitFor(() => expect(eventBus).not.toBeNull());
    await waitFor(() => {
      expect(screen.getByText("The review is complete.")).toBeInTheDocument();
    });

    expect(screen.queryByTestId("run-attribution-widget")).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("get_agent_run_attributions", expect.anything());
  });
});
