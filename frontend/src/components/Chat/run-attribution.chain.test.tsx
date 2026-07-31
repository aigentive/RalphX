import { useEffect } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { getConversationTimelinePage } from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useChatStore, selectAgentActivityLabel, selectAgentStatus } from "@/stores/chatStore";
import { EventProvider, useEventBus } from "@/providers/EventProvider";
import type { EventBus } from "@/lib/event-bus";

import { ChatMessageList, type ChatMessageData } from "./ChatMessageList";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn((filePath: string) => filePath),
  invoke: vi.fn(() => Promise.resolve(undefined)),
}));

const CONVERSATION_ID = "conversation-1";
const CONTEXT_KEY = "project:conversation-1";
const RUN_ID = "run-review";

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

function Transcript() {
  const timelineQuery = useQuery({
    queryKey: ["run-attribution-chain", CONVERSATION_ID],
    queryFn: () => getConversationTimelinePage(CONVERSATION_ID, 40),
  });
  const agentStatus = useChatStore(selectAgentStatus(CONTEXT_KEY));
  const typingIndicatorLabel = useChatStore(selectAgentActivityLabel(CONTEXT_KEY));
  const messages: ChatMessageData[] = timelineQuery.data?.items.map((item) => ({
    ...item.asMessage,
    finalizedAt: item.finalizedAt,
  })) ?? [];

  return (
    <ChatMessageList
      messages={messages}
      conversationId={CONVERSATION_ID}
      isSending={false}
      isAgentRunning={agentStatus === "generating"}
      typingIndicatorLabel={typingIndicatorLabel}
      streamingToolCalls={[]}
      contextKey={CONTEXT_KEY}
    />
  );
}

function renderTranscript(onEventBusReady: (eventBus: EventBus) => void) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <EventProvider>
          <EventBusCapture onReady={onEventBusReady} />
          <Transcript />
        </EventProvider>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

function timelinePage() {
  return {
    conversation: {
      id: CONVERSATION_ID,
      context_type: "project",
      context_id: "project-1",
      claude_session_id: null,
      provider_session_id: "provider-session-1",
      provider_harness: "codex",
      title: "Review workspace",
      message_count: 1,
      last_message_at: "2026-07-31T00:00:45Z",
      created_at: "2026-07-31T00:00:00Z",
      updated_at: "2026-07-31T00:00:45Z",
    },
    items: [{
      id: "timeline-provider-1",
      conversation_id: CONVERSATION_ID,
      message_id: "message-provider-1",
      run_id: RUN_ID,
      sequence: 1,
      block_index: 0,
      role: "assistant",
      kind: "message",
      status: "finalized",
      content: "The review is complete.",
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
      created_at: "2026-07-31T00:00:40Z",
      updated_at: "2026-07-31T00:00:45Z",
      finalized_at: "2026-07-31T00:00:45Z",
    }],
    limit: 40,
    before_sequence: null,
    total_item_count: 1,
    has_older: false,
    oldest_loaded_sequence: 1,
    newest_loaded_sequence: 1,
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

describe("run attribution lifecycle chain", () => {
  afterEach(() => {
    vi.useRealTimers();
    useChatStore.setState({
      activeConversationIds: {},
      activeAgentRunIds: {},
      activeAgentRunHarnesses: {},
      activeAgentRunMeta: {},
      agentActivityLabels: {},
      agentStatus: {},
    });
  });

  it("keeps the reviewer live through stale completion, then renders the settled batch attribution", async () => {
    let eventBus: EventBus | null = null;
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-31T00:01:06Z"));
    const pendingTimeline = deferred<ReturnType<typeof timelinePage>>();
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_agent_conversation_timeline_page") {
        return pendingTimeline.promise;
      }
      if (command === "get_agent_run_attributions") {
        return Promise.resolve(settledAttribution());
      }
      return Promise.resolve(undefined);
    });

    renderTranscript((bus) => {
      eventBus = bus;
    });
    expect(eventBus).not.toBeNull();

    await act(async () => {
      eventBus?.emit("agent:run_started", {
        run_id: RUN_ID,
        context_type: "project",
        context_id: "project-1",
        conversation_id: CONVERSATION_ID,
        started_at: "2026-07-31T00:01:00Z",
        agent_name: "ralphx-workspace-reviewer",
        launch_role: "workspace_reviewer",
      });
      pendingTimeline.resolve(timelinePage());
      await pendingTimeline.promise;
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(screen.getByText("The review is complete.")).toBeInTheDocument();
    expect(screen.getByTestId("chat-typing-indicator")).toHaveTextContent("Reviewer working for 6s");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(screen.getByTestId("chat-typing-indicator")).toHaveTextContent("Reviewer working for 7s");
    vi.useRealTimers();
    const user = userEvent.setup();
    expect(screen.queryByTestId("run-attribution-widget")).not.toBeInTheDocument();

    act(() => {
      eventBus?.emit("agent:run_completed", {
        run_id: "run-stale",
        context_type: "project",
        context_id: "project-1",
        conversation_id: CONVERSATION_ID,
        status: "completed",
      });
    });

    expect(screen.getByTestId("chat-typing-indicator")).toHaveTextContent("Reviewer working");
    expect(screen.queryByTestId("run-attribution-widget")).not.toBeInTheDocument();

    act(() => {
      eventBus?.emit("agent:run_completed", {
        run_id: RUN_ID,
        context_type: "project",
        context_id: "project-1",
        conversation_id: CONVERSATION_ID,
        status: "completed",
      });
    });

    const toggle = await screen.findByTestId("run-attribution-toggle");
    expect(screen.queryByTestId("chat-typing-indicator")).not.toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("get_agent_run_attributions", { runIds: [RUN_ID] });
    await waitFor(() =>
      expect(toggle).toHaveTextContent("Reviewer worked for 1m 30s"),
    );

    await user.click(toggle);
    const panel = await screen.findByTestId("run-attribution-panel");
    expect(within(panel).getByText("Agent").parentElement).toHaveTextContent("ralphx-workspace-reviewer");
    expect(within(panel).getByText("Role").parentElement).toHaveTextContent("workspace_reviewer");
    expect(within(panel).getByText("Trigger").parentElement).toHaveTextContent("workspace_review");
    expect(within(panel).getByText("Runtime source").parentElement).toHaveTextContent("Role default");
    expect(within(panel).getByText("Timing").parentElement).toHaveTextContent("1m 30s");
  });
});
