/**
 * Replay/recovery parity harness.
 *
 * This mounts the real panel composition: useChatPanelContext → useChat /
 * timeline queries → useChatRecovery + useChatEvents → ChatMessageList. Only
 * transport, the event bus, virtualizer, and peripheral panel controls are
 * replaced, so P1/P2/P3/P4 exercise the production recovery boundary.
 */

import { act, fireEvent, render, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { IntegratedChatPanel } from "./IntegratedChatPanel";
import {
  createDeltaReplayConversationFixture,
  createReplayConversationFixture,
  type ReplayLiveEvent,
  type ReplayStepId,
} from "./__tests__/replayConversationFixture";
import {
  captureTranscriptSnapshot,
  expectPrefixExact,
  expectSameTranscript,
  textOnlyExposure,
} from "./__tests__/transcriptSnapshot";

const { eventBus, transport } = vi.hoisted(() => {
  const handlers = new Map<string, Set<(payload: unknown) => void>>();
  return {
    eventBus: {
      subscribe: vi.fn((event: string, handler: (payload: unknown) => void) => {
        const listeners = handlers.get(event) ?? new Set<(payload: unknown) => void>();
        listeners.add(handler);
        handlers.set(event, listeners);
        return () => listeners.delete(handler);
      }),
      emit: vi.fn((event: string, payload: unknown) => {
        handlers.get(event)?.forEach((handler) => handler(payload));
      }),
      clear: () => handlers.clear(),
    },
    transport: {
      listConversations: vi.fn(),
      getConversationTimelinePage: vi.fn(),
      getConversationMessagesPage: vi.fn(),
      getAgentRunStatus: vi.fn(),
      isAgentRunning: vi.fn(),
      getConversationActiveState: vi.fn(),
      getConversationStats: vi.fn(),
      listMessageAttachments: vi.fn(),
    },
  };
});

vi.mock("@/api/chat", async (importActual) => {
  const actual = await importActual<typeof import("@/api/chat")>();
  return { ...actual, chatApi: { ...actual.chatApi, ...transport } };
});

vi.mock("@/providers/EventProvider", () => ({ useEventBus: () => eventBus }));
vi.mock("@/hooks/useTasks", () => ({
  useTasks: () => ({ data: [] }),
  taskKeys: { list: (projectId: string) => ["tasks", projectId], detail: (taskId: string) => ["task", taskId] },
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
    Virtuoso: React.forwardRef(function ReplayVirtuoso(
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
        <div ref={rootRef} data-testid="replay-virtuoso">
          {Header ? <Header /> : null}
          {(props.data ?? []).map((item, index) => (
            <div key={index}>{props.itemContent?.((props.firstItemIndex ?? 0) + index, item)}</div>
          ))}
        </div>
      );
    }),
  };
});

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: Infinity },
      mutations: { retry: false },
    },
  });
}

function renderReplayPanel() {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <IntegratedChatPanel
          projectId="project-replay"
          contextTypeOverride="project"
          contextIdOverride="project-replay"
          conversationIdOverride="conversation-replay"
          hideHeaderSessionControls
          hideSessionToolbar
          renderComposer={() => null}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

async function emit(events: readonly ReplayLiveEvent[]): Promise<void> {
  for (const event of events) {
    // The native event bus delivers these one callback at a time. Separate React
    // commits let the production hook refresh its refs between lifecycle events.
    await act(async () => {
      eventBus.emit(event.name, event.payload);
    });
  }
}

/**
 * Expands every currently COLLAPSED tool-call group in `container`.
 *
 * Idempotent by construction — only toggles reporting `aria-expanded="false"` are
 * clicked — which is what lets callers re-run it inside a `waitFor`. The panel
 * hydrates its transcript behind a paint boundary, so the set of rendered groups grows
 * across commits: a single up-front pass expands whatever happens to exist at that
 * instant and leaves any group that arrives afterwards collapsed forever, no matter
 * how long the following wait is. The unconditional-click form could not be retried,
 * because a second pass would collapse the groups the first one opened.
 */
function expandVisibleToolGroups(container: HTMLElement): void {
  act(() => {
    within(container)
      .queryAllByTestId("tool-call-group-toggle")
      .filter((toggle) => toggle.getAttribute("aria-expanded") !== "true")
      .forEach((toggle) => fireEvent.click(toggle));
  });
}

/**
 * Expands tool-call groups until none is left collapsed.
 *
 * Used where a transcript SNAPSHOT follows immediately: a snapshot taken while a group
 * is still collapsed hides that group's rows, and the parity comparison would then be
 * between two differently-hydrated transcripts rather than between the transcripts.
 */
async function expandAllToolGroups(container: HTMLElement): Promise<void> {
  await waitFor(() => {
    expandVisibleToolGroups(container);
    expect(
      within(container)
        .queryAllByTestId("tool-call-group-toggle")
        .filter((toggle) => toggle.getAttribute("aria-expanded") !== "true"),
    ).toHaveLength(0);
  });
}

/**
 * Hydration reaches the transcript across separate commits — the timeline page
 * lands before active state resolves — so a single expansion pass can miss a
 * group that has not rendered yet. Expansion is idempotent (already-expanded
 * groups are skipped), so retrying it alongside the assertion keeps the
 * production expectation intact without depending on query resolution order.
 */
async function expandToolGroupsUntil(
  container: HTMLElement,
  assertion: () => void,
): Promise<void> {
  await waitFor(() => {
    expandVisibleToolGroups(container);
    assertion();
  });
}

function liveRowKeys(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll<HTMLElement>("[data-chat-live-row-key]"))
    .map((element) => element.dataset.chatLiveRowKey ?? "");
}

describe("IntegratedChatPanel live replay/recovery parity", () => {
  const fixture = createReplayConversationFixture();
  let backendStep: ReplayStepId;

  beforeEach(() => {
    backendStep = "turn-2-user";
    eventBus.clear();
    vi.clearAllMocks();
    transport.listConversations.mockResolvedValue([fixture.conversation]);
    transport.getConversationTimelinePage.mockImplementation(async () => fixture.timelinePage(backendStep));
    transport.getConversationMessagesPage.mockResolvedValue({
      conversation: fixture.conversation,
      messages: [],
      totalMessageCount: 0,
      offset: 0,
      limit: 40,
      hasOlder: false,
    });
    transport.getAgentRunStatus.mockResolvedValue({
      id: "run-replay",
      conversationId: "conversation-replay",
      status: "running",
      startedAt: "2026-04-10T07:00:00.000Z",
      completedAt: null,
      errorMessage: null,
      modelId: null,
      modelLabel: null,
    });
    transport.isAgentRunning.mockResolvedValue(true);
    transport.getConversationActiveState.mockImplementation(async () => fixture.activeState(backendStep));
    transport.getConversationStats.mockResolvedValue(null);
    transport.listMessageAttachments.mockResolvedValue([]);

    act(() => {
      useChatStore.setState({
        activeConversationIds: {},
        agentStatus: {},
        activeAgentRunIds: {},
        activeAgentRunHarnesses: {},
        isSending: {},
        queuedMessages: {},
        effectiveModel: {},
        delegateExpansionByConversation: {},
      });
      useUiStore.setState({ selectedTaskId: null, taskHistoryState: null });
    });
  });

  it("keeps one two-turn transcript intact across live streaming, recovery hydration, and final persistence", async () => {
    const editCheckpoint = fixture.timelinePage("turn-2-edit-started");
    expect(editCheckpoint.items.some((item) => item.status === "streaming")).toBe(true);
    expect(editCheckpoint.items.some((item) => item.status === "finalized")).toBe(true);
    expect(fixture.activeState("turn-2-edit-started").tool_calls).toHaveLength(3);
    expect(fixture.events("turn-2-user", "turn-2-edit-started").map((event) => event.name)).toContain(
      "agent:task_completed",
    );
    expect(fixture.timelinePage("turn-2-finalized").items.every(
      (item) => item.status === "finalized",
    )).toBe(true);

    // P1: stay continuously mounted while the original live event stream reaches
    // the in-flight Edit checkpoint.
    const p1 = renderReplayPanel();
    await waitFor(() => {
      expect(p1.container.querySelectorAll('[data-chat-message-item="true"]')).not.toHaveLength(0);
      expect(eventBus.subscribe).toHaveBeenCalledWith("agent:tool_call", expect.any(Function));
      expect(eventBus.subscribe).toHaveBeenCalledWith("agent:chunk", expect.any(Function));
    });
    await emit(fixture.events("turn-2-user", "turn-2-edit-started"));
    await waitFor(() => {
      expect(within(p1.container).getAllByTestId("tool-call-group-toggle").length).toBeGreaterThan(0);
    });
    await expandToolGroupsUntil(p1.container, () => {
      expect(within(p1.container).getByTestId("diff-tool-call-view")).toBeInTheDocument();
      expect(within(p1.container).getByTestId("task-subagent-card")).toBeInTheDocument();
    });
    const p1AtEdit = captureTranscriptSnapshot(p1.container);
    const p1LiveTail = liveRowKeys(p1.container);
    expect(p1LiveTail).toContain("streaming-text:seq-8");

    // The user leaves, destroying the panel-local stream state.
    const departed = renderReplayPanel();
    await waitFor(() => {
      expect(departed.container.textContent).toContain(
        "Now prove live recovery without losing turn one.",
      );
    });
    departed.unmount();

    // P2: a remount at the identical backend step. It obtains durable
    // siblings from the timeline and the in-flight Edit exclusively via active state.
    backendStep = "turn-2-edit-started";
    transport.getConversationActiveState.mockClear();
    const p2 = renderReplayPanel();
    await waitFor(() => {
      expect(transport.getConversationActiveState).toHaveBeenCalledWith("conversation-replay");
      expect(within(p2.container).getAllByTestId("tool-call-group-toggle").length).toBeGreaterThan(0);
    });
    await expandToolGroupsUntil(p2.container, () => {
      expect(within(p2.container).getByTestId("diff-tool-call-view")).toBeInTheDocument();
      expect(captureTranscriptSnapshot(p2.container).filter(
        (row) => row.key === "text:Live turn two begins before remount.",
      )).toHaveLength(1);
    });
    const p2AtHydration = captureTranscriptSnapshot(p2.container);
    const p2LiveTail = liveRowKeys(p2.container);
    expect(p2LiveTail.length).toBeGreaterThan(0);
    expect(p2LiveTail).not.toEqual(p1LiveTail);
    expectSameTranscript(p2AtHydration, p1AtEdit);
    expect(p2AtHydration.filter((row) => row.kind === "delegate")).toHaveLength(2);
    expect(p2AtHydration.some((row) => row.key === "user:Replay the full two-turn recovery path.")).toBe(true);

    // A late live block reaches both mounted panels. P2 must keep every hydrated
    // turn-one sibling rather than replacing its recovered prefix with the tail.
    backendStep = "turn-2-final-text";
    await emit(fixture.events("turn-2-edit-started", "turn-2-final-text"));
    await waitFor(() => {
      expect(p2.container.textContent).toContain("Live turn two is ready to finalize.");
    });
    const p2AfterLateLive = captureTranscriptSnapshot(p2.container);
    const turnTwoStart = p2AtHydration.findIndex(
      (row) => row.key === "user:Now prove live recovery without losing turn one.",
    );
    const p2TurnOne = p2AtHydration.slice(0, turnTwoStart);
    const turnOneKeys = new Set(p2TurnOne.map((row) => row.key));
    expectSameTranscript(
      p2AfterLateLive.filter((row) => turnOneKeys.has(row.key)),
      p2TurnOne,
    );
    expect(p2AfterLateLive.some((row) => row.key === "delegate:replay-delegate-turn-1")).toBe(true);
    p2.unmount();

    // P3: resume the original P1 event stream through message-created finalization.
    backendStep = "turn-2-finalized";
    await emit(fixture.events("turn-2-final-text", "turn-2-finalized"));
    await waitFor(() => {
      expect(p1.container.querySelectorAll("[data-chat-live-row-key]")).toHaveLength(0);
      expect(p1.container.textContent).toContain("Live turn two is ready to finalize.");
    });
    await expandAllToolGroups(p1.container);
    const p3Finalized = captureTranscriptSnapshot(p1.container);

    // P4: no live state survives this fresh mount; it sees final persistence only.
    transport.getAgentRunStatus.mockResolvedValue({
      id: "run-replay",
      conversationId: "conversation-replay",
      status: "completed",
      startedAt: "2026-04-10T07:00:00.000Z",
      completedAt: "2026-04-10T07:00:16.000Z",
      errorMessage: null,
      modelId: null,
      modelLabel: null,
    });
    transport.isAgentRunning.mockResolvedValue(false);
    const p4 = renderReplayPanel();
    await waitFor(() => {
      expect(p4.container.textContent).toContain("Live turn two is ready to finalize.");
    });
    await expandAllToolGroups(p4.container);
    const p4Persisted = captureTranscriptSnapshot(p4.container);

    expectSameTranscript(p3Finalized, p4Persisted);
    expectPrefixExact(p1AtEdit, p4Persisted);
    expectPrefixExact(p2AtHydration, p4Persisted);
    expect(p4Persisted.filter((row) => row.kind === "delegate")).toHaveLength(2);
    expect(textOnlyExposure(p4.container)).not.toContain("frontend/src/hooks");
    expect(textOnlyExposure(p4.container)).not.toContain("old_string");
  });

  it("restores a text-only in-flight turn from the streaming timeline without duplication", async () => {
    backendStep = "turn-2-text-delta";
    const reentered = renderReplayPanel();

    await waitFor(() => {
      expect(transport.getConversationActiveState).toHaveBeenCalledWith("conversation-replay");
      expect(captureTranscriptSnapshot(reentered.container).filter(
        (row) => row.key === "text:Live turn two begins before remount.",
      )).toHaveLength(1);
    });

    const transcript = captureTranscriptSnapshot(reentered.container);
    const turnTwoStart = transcript.findIndex(
      (row) => row.key === "user:Now prove live recovery without losing turn one.",
    );
    expect(transcript.slice(turnTwoStart)).toEqual([
      {
        kind: "user",
        key: "user:Now prove live recovery without losing turn one.",
        text: "Now prove live recovery without losing turn one.",
      },
      {
        kind: "text",
        key: "text:Live turn two begins before remount.",
        text: "Live turn two begins before remount.",
      },
    ]);
  });

  it("recovers a delta-streamed turn mid-segment without concatenating text segments", async () => {
    const delta = createDeltaReplayConversationFixture();
    backendStep = "turn-2-edit-started";
    transport.getConversationTimelinePage.mockImplementation(async () => delta.timelinePage(backendStep));
    transport.getConversationActiveState.mockImplementation(async () => delta.activeState(backendStep));
    const panel = renderReplayPanel();

    await waitFor(() => {
      const transcript = captureTranscriptSnapshot(panel.container);
      const textRows = transcript.filter((row) => row.kind === "text");
      expect(textRows).toEqual(expect.arrayContaining([
        // Snapshot helper trims captured text; segment identity is what matters.
        expect.objectContaining({ text: "First segment survives the switch." }),
        expect.objectContaining({ text: "Second segment resumes from its mid-stream tail." }),
      ]));
      expect(textRows.some((row) => row.text.includes(
        "First segment survives the switch. Second segment resumes",
      ))).toBe(false);
    });
  });

  it("keeps block order when late events race recovery", async () => {
    let resolveActiveState: ((value: ReturnType<typeof fixture.activeState>) => void) | undefined;
    transport.getConversationActiveState.mockImplementation(() => new Promise((resolve) => {
      resolveActiveState = resolve;
    }));
    backendStep = "turn-2-edit-started";
    const panel = renderReplayPanel();
    await waitFor(() => expect(eventBus.subscribe).toHaveBeenCalledWith("agent:tool_call", expect.any(Function)));
    await waitFor(() => expect(transport.getConversationActiveState).toHaveBeenCalled());
    await act(async () => resolveActiveState?.(fixture.activeState(backendStep)));
    // Hydration-recovered live rows carry no receivedAt timestamps.
    await waitFor(() => expect(liveRowKeys(panel.container).length).toBeGreaterThan(0));
    // Late live events land with wall-clock receivedAt while every recovered
    // row has none — the RC1 mixed-sort-key regime. The chunk opens a new
    // text block after the recovered tool group, so pre-fix wall-clock
    // sorting would float it above the recovered tail.
    await emit([
      { name: "agent:tool_call", payload: {
        conversation_id: "conversation-replay", context_id: "project-replay", context_type: "project",
        run_id: "run-replay", seq: 98, tool_name: "Write", tool_id: "late-tool",
        arguments: { file_path: "frontend/src/replay/late-write.ts" }, result: "ok",
      } },
      { name: "agent:chunk", payload: {
        conversation_id: "conversation-replay", context_id: "project-replay", context_type: "project",
        run_id: "run-replay", seq: 99, append_to_previous: true, block_index: 5,
        text: "Late text lands after the recovered tools.",
      } },
    ]);
    await waitFor(() => {
      // Live groups may already be expanded; only expand collapsed toggles.
      within(panel.container)
        .queryAllByTestId("tool-call-group-toggle")
        .filter((toggle) => toggle.getAttribute("aria-expanded") === "false")
        .forEach((toggle) => fireEvent.click(toggle));
      const transcript = captureTranscriptSnapshot(panel.container);
      const lateTextIndex = transcript.findIndex(
        (row) => row.text === "Late text lands after the recovered tools.",
      );
      const editIndex = transcript.findIndex((row) => row.key.startsWith("tool:edit"));
      expect(editIndex).toBeGreaterThan(-1);
      // The late live row joins the contiguous live tail after every
      // recovered row instead of wall-clock-sorting above it (RC1).
      expect(lateTextIndex).toBeGreaterThan(editIndex);
      expect(lateTextIndex).toBe(transcript.length - 1);
    });
  });

  it("stale timeline page does not corrupt recovery", async () => {
    const delta = createDeltaReplayConversationFixture();
    backendStep = "turn-2-edit-started";
    let timelineCalls = 0;
    transport.getConversationTimelinePage.mockImplementation(async () => {
      timelineCalls += 1;
      return delta.timelinePage(timelineCalls === 1 ? "turn-2-grep-result" : backendStep);
    });
    transport.getConversationActiveState.mockImplementation(async () => delta.activeState(backendStep));
    const panel = renderReplayPanel();
    await waitFor(() => {
      expect(panel.container.textContent).toContain("Second segment resumes from its mid-stream tail.");
      expect(captureTranscriptSnapshot(panel.container)).toEqual(
        expect.arrayContaining([expect.objectContaining({ text: "First segment survives the switch." })]),
      );
    });
  });
});
