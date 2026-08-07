/**
 * Behavioral integration coverage for the controller-wired transcript.
 *
 * The Virtuoso test double deliberately replays the callbacks that matter to
 * the host component. Scroll geometry remains mocked, but tests only assert
 * externally observable controller effects: writes, the bottom control, and
 * callback-facing behavior.
 */

import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { logger } from "@/lib/logger";
import { useChatStore } from "@/stores/chatStore";
import {
  ChatMessageList,
  type ChatMessageData,
} from "./ChatMessageList";
import { foldDelegationTimelineMessages } from "./delegation-timeline";

const harness = vi.hoisted(() => ({
  props: null as Record<string, unknown> | null,
  componentsHistory: [] as unknown[],
  scrollToIndex: vi.fn(),
  autoscrollToBottom: vi.fn(),
}));

const messageAttachments = vi.hoisted(() => vi.fn(() => ({ data: new Map() })));
const runAttributions = vi.hoisted(() => vi.fn(() => ({
  data: new Map(),
  isPending: false,
  isError: false,
  refetch: vi.fn(),
})));

vi.mock("@/hooks/useMessageAttachments", () => ({
  useMessageAttachments: (...args: unknown[]) => messageAttachments(...args),
}));

vi.mock("@/hooks/useRunAttributions", () => ({
  useRunAttributions: (...args: unknown[]) => runAttributions(...args),
}));

vi.mock("./MessageItem", async () => {
  const { PersonaRunBadge } = await import("./PersonaRunBadge");
  return {
    MessageItem: ({
      content,
      children,
      createdAt,
      hideMeta,
      agentPersonasEnabled,
      personaSlug,
      personaVersion,
      personaInjected,
      personaSkippedReason,
    }: {
      content: string;
      children?: React.ReactNode;
      createdAt: string;
      hideMeta?: boolean;
      agentPersonasEnabled?: boolean;
      personaSlug?: string | null;
      personaVersion?: number | null;
      personaInjected?: boolean | null;
      personaSkippedReason?: string | null;
    }) => (
      <article data-chat-message-item="true">
        {content}
        {children}
        {!hideMeta && <footer data-testid="message-meta">{createdAt}</footer>}
        <PersonaRunBadge
          enabled={agentPersonasEnabled ?? false}
          personaSlug={personaSlug ?? null}
          personaVersion={personaVersion ?? null}
          personaInjected={personaInjected ?? null}
          skippedReason={personaSkippedReason ?? null}
        />
      </article>
    ),
    MessageMeta: ({ createdAt }: { createdAt: string }) => (
      <footer data-testid="message-meta">{createdAt}</footer>
    ),
  };
});

vi.mock("./TextBubble", () => ({
  TextBubble: ({ text }: { text: string }) => <span>{text}</span>,
}));

vi.mock("./TaskSubagentCard", () => ({
  TaskSubagentCard: () => <div>task</div>,
}));

vi.mock("react-virtuoso", async () => {
  const React = await import("react");

  type VirtuosoProps = Record<string, unknown> & {
    components?: {
      Footer?: React.ComponentType;
      Header?: React.ComponentType;
      Scroller?: React.ForwardRefExoticComponent<
        React.ComponentPropsWithoutRef<"div"> & React.RefAttributes<HTMLDivElement>
      >;
    };
    data?: unknown[];
    firstItemIndex?: number;
    itemContent?: (index: number, item: unknown) => React.ReactNode;
    rangeChanged?: (range: { startIndex: number; endIndex: number }) => void;
    scrollerRef?: (element: HTMLElement | Window | null) => void;
    atBottomStateChange?: (atBottom: boolean) => void;
  };

  const Virtuoso = React.forwardRef<unknown, VirtuosoProps>(function MockVirtuoso(props, ref) {
    const elementRef = React.useRef<HTMLDivElement | null>(null);
    const data = props.data ?? [];
    const Scroller = props.components?.Scroller ?? "div";
    const Footer = props.components?.Footer;
    const Header = props.components?.Header;
    const { atBottomStateChange, rangeChanged, scrollerRef } = props;

    const setScroller = React.useCallback((element: HTMLDivElement | null) => {
      if (element) {
        setScrollerGeometry(element, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 480 });
      }
      elementRef.current = element;
    }, []);

    React.useImperativeHandle(ref, () => ({
      scrollToIndex: harness.scrollToIndex,
      autoscrollToBottom: harness.autoscrollToBottom,
    }));

    React.useEffect(() => {
      harness.props = props;
      harness.componentsHistory.push(props.components);
      return () => {
        if (harness.props === props) {
          harness.props = null;
        }
      };
    });

    React.useEffect(() => {
      const element = elementRef.current;
      if (!element) return undefined;
      scrollerRef?.(element);
      if (data.length > 0) {
        rangeChanged?.({ startIndex: 0, endIndex: data.length - 1 });
      }
      return () => scrollerRef?.(null);
    }, [data.length, rangeChanged, scrollerRef]);

    // Virtuoso publishes its own debounced, distinct-until-changed at-bottom
    // state; the double derives it from the same scroll events the transcript
    // already fires so bottom-control visibility stays observable in tests.
    React.useEffect(() => {
      const element = elementRef.current;
      if (!element || !atBottomStateChange) return undefined;
      let reported: boolean | null = null;
      const publish = (): void => {
        const atBottom =
          element.scrollHeight - element.clientHeight - element.scrollTop <= 2;
        if (reported === atBottom) return;
        reported = atBottom;
        atBottomStateChange(atBottom);
      };
      publish();
      element.addEventListener("scroll", publish);
      return () => element.removeEventListener("scroll", publish);
    }, [atBottomStateChange, data.length]);

    return (
      <Scroller ref={setScroller} data-testid="mock-virtuoso">
        {Header ? <Header /> : null}
        <div data-testid="virtuoso-item-list">
          {data.map((item, index) => (
            <div key={index} data-mock-index={index}>
              {props.itemContent?.((props.firstItemIndex ?? 0) + index, item)}
            </div>
          ))}
        </div>
        {Footer ? <Footer /> : null}
      </Scroller>
    );
  });

  return { Virtuoso };
});

type ScrollerGeometry = {
  clientHeight: number;
  scrollHeight: number;
  scrollTop: number;
};

function setScrollerGeometry(element: HTMLElement, geometry: ScrollerGeometry): void {
  // Browsers clamp scrollTop to the scroll extent, on write and whenever the
  // content shrinks. Bottom follow relies on that clamp, so the double models
  // it rather than allowing positions past the true bottom.
  const maxScrollTop = Math.max(0, geometry.scrollHeight - geometry.clientHeight);
  let position = Math.min(geometry.scrollTop, maxScrollTop);
  Object.defineProperties(element, {
    clientHeight: { configurable: true, value: geometry.clientHeight },
    scrollHeight: { configurable: true, value: geometry.scrollHeight },
    scrollTop: {
      configurable: true,
      get: () => position,
      set: (next: number) => {
        position = Math.max(0, Math.min(next, maxScrollTop));
      },
    },
  });
}

function messages(count = 3, offset = 0): ChatMessageData[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `message-${offset + index + 1}`,
    role: index % 2 === 0 ? "user" : "assistant",
    content: `Message ${offset + index + 1}`,
    createdAt: new Date(2026, 0, 1, 12, offset + index).toISOString(),
    toolCalls: null,
    contentBlocks: null,
  }));
}

const defaultProps = {
  messages: messages(),
  conversationId: "conversation-a",
  failedRun: null,
  onDismissFailedRun: vi.fn(),
  isSending: false,
  isAgentRunning: false,
  streamingToolCalls: [],
  streamingTasks: new Map(),
};

function renderList(overrides: Partial<React.ComponentProps<typeof ChatMessageList>> = {}) {
  return render(<ChatMessageList {...defaultProps} {...overrides} />);
}

it("renders and registers the transcript bottom spacer after timeline content", () => {
  const registerBottomSpacer = vi.fn();

  renderList({ registerBottomSpacer });

  const spacer = screen.getByTestId("chat-transcript-bottom-spacer");
  const transcript = screen.getByTestId("integrated-chat-messages");
  expect(transcript.lastElementChild).toBe(spacer);
  expect(spacer).toHaveAttribute("aria-hidden", "true");
  expect(registerBottomSpacer).toHaveBeenCalledWith(spacer);
});

it("offsets the scroll-to-bottom control above the measured composer inset", () => {
  renderList();

  expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveStyle({
    bottom: "calc(var(--chat-bottom-inset, 0px) + 1rem)",
  });
});

it("rehydrates one completed-run widget at the end of persisted run rows", () => {
  renderList({ messages: [
    { id: "run-row-1", role: "assistant", content: "first", createdAt: "2026-01-01T12:00:00Z", finalizedAt: "2026-01-01T12:00:10Z", runId: "run-1" },
    { id: "run-row-2", role: "assistant", content: "last", createdAt: "2026-01-01T12:00:02Z", finalizedAt: "2026-01-01T12:00:42Z", runId: "run-1" },
  ] });

  expect(screen.getAllByTestId("run-attribution-widget")).toHaveLength(1);
  expect(screen.getByTestId("run-attribution-toggle")).toHaveTextContent("Agent worked for 42s");
});

it("uses settled run attribution for the transcript role and duration", () => {
  runAttributions.mockReturnValueOnce({
    data: new Map([["run-1", {
      id: "run-1", conversationId: "conversation-a", status: "completed",
      startedAt: "2026-01-01T12:00:00Z", completedAt: "2026-01-01T12:00:58Z",
      harness: null, upstreamProvider: null, providerProfile: null, providerSessionId: null,
      logicalModel: null, effectiveModelId: null, logicalEffort: null, effectiveEffort: null,
      serviceTier: null, approvalPolicy: null, sandboxMode: null, inputTokens: null,
      outputTokens: null, cacheCreationTokens: null, cacheReadTokens: null, estimatedUsd: null,
      runChainId: null, actionKind: null, personaSlug: null, agentName: "reviewer",
      launchRole: "workspace_reviewer", runtimeSource: "role_default",
    }]]),
    isPending: false,
    isError: false,
    refetch: vi.fn(),
  });

  renderList({ messages: [
    { id: "run-row", role: "assistant", content: "done", createdAt: "2026-01-01T12:00:40Z", finalizedAt: "2026-01-01T12:00:42Z", runId: "run-1" },
  ] });

  expect(screen.getByTestId("run-attribution-toggle")).toHaveTextContent("Reviewer worked for 58s");
});

it("shows an unavailable detail state when a settled batch omits a persisted run", () => {
  renderList({ messages: [
    { id: "missing-run-row", role: "assistant", content: "done", createdAt: "2026-01-01T12:00:40Z", finalizedAt: "2026-01-01T12:00:42Z", runId: "run-missing" },
  ] });

  fireEvent.click(screen.getByTestId("run-attribution-toggle"));

  expect(screen.getByText("Run attribution is unavailable.")).toBeInTheDocument();
  expect(screen.queryByTestId("run-attribution-loading")).not.toBeInTheDocument();
});

it("renders exactly one run-attribution widget after a collapsed tool-activity group whose last row is covered", () => {
  renderList({ messages: [
    {
      id: "tool-row-1",
      role: "assistant",
      content: "",
      createdAt: "2026-01-01T12:00:00Z",
      timelineSequence: 1,
      contentBlocks: [{ type: "tool_use", id: "t1", name: "Bash" }],
      runId: "run-tool",
    },
    {
      id: "tool-row-2",
      role: "assistant",
      content: "",
      createdAt: "2026-01-01T12:00:01Z",
      timelineSequence: 2,
      contentBlocks: [{ type: "tool_use", id: "t2", name: "Bash" }],
      runId: "run-tool",
      finalizedAt: "2026-01-01T12:00:05Z",
    },
  ] });

  expect(screen.getAllByTestId("run-attribution-widget")).toHaveLength(1);
  const toggle = screen.getByTestId("tool-call-group-toggle");
  const widget = screen.getByTestId("run-attribution-widget");
  expect(
    toggle.compareDocumentPosition(widget) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(widget.closest(".px-3")).not.toBeNull();
});

it("renders exactly one run-attribution widget when the run's last row is a covered persisted-thinking row", () => {
  renderList({ messages: [
    {
      id: "thinking-row-1",
      role: "assistant",
      content: "",
      createdAt: "2026-01-01T12:00:00Z",
      timelineSequence: 1,
      contentBlocks: [{ type: "thinking", text: "first thought" }],
      runId: "run-thinking",
    },
    {
      id: "thinking-row-2",
      role: "assistant",
      content: "",
      createdAt: "2026-01-01T12:00:01Z",
      timelineSequence: 2,
      contentBlocks: [{ type: "thinking", text: "second thought" }],
      runId: "run-thinking",
      finalizedAt: "2026-01-01T12:00:05Z",
    },
  ] });

  expect(screen.getAllByTestId("run-attribution-widget")).toHaveLength(1);
  const toggle = screen.getByTestId("thinking-group-toggle");
  const widget = screen.getByTestId("run-attribution-widget");
  expect(
    toggle.compareDocumentPosition(widget) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(widget.closest(".px-3")).not.toBeNull();
});

it("renders exactly one run-attribution widget when the run's last provider row is a group toggle row itself, not a covered member", () => {
  renderList({ messages: [
    {
      id: "toggle-anchor-row",
      role: "assistant",
      content: "",
      createdAt: "2026-01-01T12:00:00Z",
      timelineSequence: 1,
      contentBlocks: [{ type: "tool_use", id: "t1", name: "Bash" }],
      runId: "run-toggle-anchor",
      finalizedAt: "2026-01-01T12:00:05Z",
    },
  ] });

  expect(screen.getAllByTestId("run-attribution-widget")).toHaveLength(1);
  const toggle = screen.getByTestId("tool-call-group-toggle");
  const widget = screen.getByTestId("run-attribution-widget");
  expect(
    toggle.compareDocumentPosition(widget) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(widget.closest(".px-3")).not.toBeNull();
});

it("renders no widget and fetches no attributions when every message lacks a runId", () => {
  runAttributions.mockClear();

  renderList({ messages: [
    { id: "legacy-row", role: "assistant", content: "legacy", createdAt: "2026-01-01T12:00:00Z" },
  ] });

  expect(screen.queryByTestId("run-attribution-widget")).not.toBeInTheDocument();
  expect(runAttributions).toHaveBeenLastCalledWith([], expect.any(Object));
});

it("hides the widget for a live run while still rendering the previous run's widget", () => {
  const contextKey = "project:conversation-a";
  act(() => {
    useChatStore.setState({ activeAgentRunIds: { [contextKey]: "run-live" } });
  });

  try {
    renderList({
      contextKey,
      messages: [
        { id: "run-prev-row", role: "assistant", content: "prev", createdAt: "2026-01-01T12:00:00Z", finalizedAt: "2026-01-01T12:00:10Z", runId: "run-prev" },
        { id: "run-live-row", role: "assistant", content: "live", createdAt: "2026-01-01T12:00:20Z", runId: "run-live" },
      ],
    });

    expect(screen.getAllByTestId("run-attribution-widget")).toHaveLength(1);
    expect(screen.getByTestId("run-attribution-toggle")).toBeInTheDocument();
  } finally {
    act(() => {
      useChatStore.setState({ activeAgentRunIds: {} });
    });
  }
});

function getScroller(): HTMLElement {
  return screen.getByTestId("mock-virtuoso");
}

/**
 * Follow reaches Virtuoso one of two ways: arming its post-growth window when
 * the reader is still at the bottom, or re-issuing `scrollToIndex` when a
 * growth left them short. Both are Virtuoso-owned; neither writes scrollTop.
 */
function expectFollowDelegated(): void {
  const armed = harness.autoscrollToBottom.mock.calls.length > 0;
  const corrected = harness.scrollToIndex.mock.calls.some(
    ([location]: [{ index: unknown; align: unknown }]) =>
      location.index === "LAST" && location.align === "end",
  );
  expect(armed || corrected, "expected follow to be delegated to Virtuoso").toBe(true);
  expect(scrollWrites).not.toHaveBeenCalled();
}

function expectNoFollow(): void {
  expect(harness.autoscrollToBottom).not.toHaveBeenCalled();
  expect(harness.scrollToIndex).not.toHaveBeenCalled();
  expect(scrollWrites).not.toHaveBeenCalled();
}

type FollowOutputFn = (atBottomOrScrolling: boolean) => "auto" | false;

function followOutput(): FollowOutputFn {
  return callback<FollowOutputFn>("followOutput");
}

function callback<T>(name: string): T {
  const value = harness.props?.[name];
  expect(value).toEqual(expect.any(Function));
  return value as T;
}

let animationFrames = new Map<number, FrameRequestCallback>();
let nextAnimationFrame = 1;
let scrollWrites: ReturnType<typeof vi.fn>;

function flushAnimationFrames(limit = 20): void {
  act(() => {
    for (let pass = 0; pass < limit && animationFrames.size > 0; pass += 1) {
      const callbacks = [...animationFrames.entries()];
      animationFrames = new Map();
      callbacks.forEach(([, frame]) => frame(performance.now()));
    }
  });
}

function primeAtBottom(): HTMLElement {
  const scroller = getScroller();
  // Stands in for Virtuoso's initial-scroll gate landing the transcript on the
  // last item. Nothing in the controller writes scroll any more, so the double
  // has to place the reader genuinely at the bottom rather than short of it.
  setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 500 });
  fireEvent.scroll(scroller);
  flushAnimationFrames();
  scrollWrites.mockClear();
  harness.scrollToIndex.mockClear();
  harness.autoscrollToBottom.mockClear();
  return scroller;
}

describe("transcript bottom spacer placement", () => {
  beforeEach(() => {
    vi.stubEnv("VITEST", "");
    harness.props = null;
    harness.componentsHistory = [];
    harness.scrollToIndex.mockReset();
    harness.autoscrollToBottom.mockReset();
    messageAttachments.mockReturnValue({ data: new Map() });
    runAttributions.mockReturnValue({ data: new Map(), isPending: false, isError: false, refetch: vi.fn() });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllEnvs();
  });

  function spacers(): HTMLElement[] {
    return screen.queryAllByTestId("chat-transcript-bottom-spacer");
  }

  function itemAt(index: number): HTMLElement {
    const item = document.querySelector<HTMLElement>(`[data-mock-index="${index}"]`);
    if (!item) throw new Error(`no rendered item at mock index ${index}`);
    return item;
  }

  it("renders exactly one spacer, inside the last timeline item", () => {
    renderList();

    expect(spacers()).toHaveLength(1);
    expect(itemAt(2)).toContainElement(spacers()[0] ?? null);
  });

  it("sizes the spacer from the inherited composer-inset variable", () => {
    renderList();

    expect(spacers()[0]).toHaveStyle({ height: "var(--chat-bottom-inset, 0px)" });
    expect(spacers()[0]).toHaveAttribute("aria-hidden", "true");
  });

  it("registers the spacer with the inset writer", () => {
    const registerBottomSpacer = vi.fn();

    renderList({ registerBottomSpacer });

    expect(registerBottomSpacer).toHaveBeenCalledWith(spacers()[0]);
  });

  it("anchors the spacer on the last timeline item under a shifted firstItemIndex", () => {
    renderList({ firstItemIndex: 10, messages: messages(4, 20) });

    // itemContent receives firstItemIndex-shifted indices, and the anchor is
    // compared in that same space - not against the unshifted array position.
    expect(spacers()).toHaveLength(1);
    expect(itemAt(3)).toContainElement(spacers()[0] ?? null);
  });

  // The regression this anchor exists for: a finished assistant turn commonly
  // ends in a collapsed tool-call group, whose covered members make
  // lastVisibleTimelineIndex smaller than timeline.length - 1. Anchoring there
  // would put the run-attribution row below the reserved inset and reproduce
  // the original composer overlap. (A fully covered timeline, where that index
  // is -1, is unreachable through props: a group's first row is always its
  // visible toggle.)
  it("keeps the spacer on the last item when the tail is a covered tool-call row", () => {
    renderList({ messages: [
      {
        id: "tool-row-1",
        role: "assistant",
        content: "",
        createdAt: "2026-01-01T12:00:00Z",
        timelineSequence: 1,
        contentBlocks: [{ type: "tool_use", id: "t1", name: "Bash" }],
        runId: "run-tool",
      },
      {
        id: "tool-row-2",
        role: "assistant",
        content: "",
        createdAt: "2026-01-01T12:00:01Z",
        timelineSequence: 2,
        contentBlocks: [{ type: "tool_use", id: "t2", name: "Bash" }],
        runId: "run-tool",
        finalizedAt: "2026-01-01T12:00:05Z",
      },
    ] });

    expect(screen.getByTestId("tool-call-group-toggle")).toHaveAttribute("aria-expanded", "false");
    expect(spacers()).toHaveLength(1);
    expect(itemAt(1)).toContainElement(spacers()[0] ?? null);
  });

  it("falls back to the Virtuoso footer only while the timeline is empty", () => {
    const { rerender } = render(<ChatMessageList {...defaultProps} messages={[]} />);

    expect(spacers()).toHaveLength(1);
    expect(document.querySelector("[data-mock-index]")).toBeNull();

    rerender(<ChatMessageList {...defaultProps} />);

    expect(spacers()).toHaveLength(1);
    expect(itemAt(2)).toContainElement(spacers()[0] ?? null);
  });
});

describe("ChatMessageList controller integration", () => {
  beforeEach(() => {
    vi.stubEnv("VITEST", "");
    harness.props = null;
    harness.componentsHistory = [];
    harness.scrollToIndex.mockReset();
    harness.autoscrollToBottom.mockReset();
    messageAttachments.mockReturnValue({ data: new Map() });
    runAttributions.mockReturnValue({ data: new Map(), isPending: false, isError: false, refetch: vi.fn() });
    animationFrames = new Map();
    nextAnimationFrame = 1;
    scrollWrites = vi.fn();

    vi.stubGlobal("requestAnimationFrame", (frame: FrameRequestCallback): number => {
      const id = nextAnimationFrame;
      nextAnimationFrame += 1;
      animationFrames.set(id, frame);
      return id;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number): void => {
      animationFrames.delete(id);
    });
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value: function scrollTo(this: HTMLElement, options: ScrollToOptions): void {
        scrollWrites(options);
        if (typeof options.top === "number") {
          this.scrollTop = options.top;
        }
      },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  it("leaves the initial landing to Virtuoso and preserves the paint cover until the transcript is ready", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
    const onInitialPaintReady = vi.fn();
    renderList({ initialPaintCoverKey: "conversation-a", onInitialPaintReady });

    expect(screen.getByTestId("chat-transcript-settling-placeholders")).toBeInTheDocument();
    const scroller = getScroller();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 480 });
    flushAnimationFrames();

    // Virtuoso's once-per-mount initial-scroll gate owns first position. The
    // literal "LAST" location is what keeps it armed for a conversation that
    // mounts empty and hydrates afterwards.
    expect(harness.props?.initialTopMostItemIndex).toEqual({ index: "LAST", align: "end" });
    expect(followOutput()(true)).toBe("auto");
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
    expect(scrollWrites).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(onInitialPaintReady).toHaveBeenCalledWith("conversation-a");
    expect(screen.queryByTestId("chat-transcript-settling-placeholders")).not.toBeInTheDocument();
  });

  it("arms Virtuoso's follow for every streaming growth signal without writing scroll", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");

    act(() => {
      totalListHeightChanged(1_000);
      setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_030, scrollTop: 500 });
      totalListHeightChanged(1_010);
      totalListHeightChanged(1_020);
      totalListHeightChanged(1_030);
    });
    flushAnimationFrames();

    // Overlapping follow windows are the point: Virtuoso's post-growth window
    // is short, so every growth signal re-arms it instead of coalescing.
    expectFollowDelegated();
  });

  it("arms follow when the first post-attach list-height measurement reports content", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_030, scrollTop: 500 });
    act(() => totalListHeightChanged(1_030));
    flushAnimationFrames();

    expectFollowDelegated();
  });

  it("does not pin again when a later total-height measurement shrinks", () => {
    renderList();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");
    primeAtBottom();

    act(() => totalListHeightChanged(1_000));
    flushAnimationFrames();
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();
    act(() => totalListHeightChanged(900));
    flushAnimationFrames();

    expect(scrollWrites).not.toHaveBeenCalled();
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
  });

  it("unfollows on wheel-up, exposes the bottom control, and ignores later growth", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 220 });

    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "false");

    scrollWrites.mockClear();
    act(() => {
      totalListHeightChanged(1_000);
      totalListHeightChanged(1_050);
    });
    flushAnimationFrames();

    expect(scrollWrites).not.toHaveBeenCalled();
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
    expect(harness.autoscrollToBottom).not.toHaveBeenCalled();
  });

  it("follows the last item for a free reader after a new user message is appended", () => {
    const { rerender } = renderList();
    const scroller = primeAtBottom();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 220 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();
    const nextMessages: ChatMessageData[] = [
      ...defaultProps.messages,
      {
        id: "message-new-user",
        role: "user",
        content: "New user message",
        createdAt: new Date(2026, 0, 1, 12, 10).toISOString(),
        toolCalls: null,
        contentBlocks: null,
      },
    ];

    rerender(<ChatMessageList {...defaultProps} messages={nextMessages} />);
    flushAnimationFrames();

    // "LAST" is resolved by Virtuoso against its own unshifted totalCount, so
    // the follow never has to reason about firstItemIndex.
    expect(harness.scrollToIndex).toHaveBeenCalledExactlyOnceWith({
      index: "LAST",
      align: "end",
      behavior: "auto",
    });
    expect(scrollWrites).not.toHaveBeenCalled();
  });

  it("keeps following when a wheel-down tick occurs at the bottom", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");

    fireEvent.wheel(scroller, { deltaY: 60 });
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_040, scrollTop: 500 });
    act(() => {
      totalListHeightChanged(1_000);
      totalListHeightChanged(1_040);
    });
    flushAnimationFrames();

    expectFollowDelegated();
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");
  });

  it("never writes scroll for a native scroll event that reports a short extent", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");

    act(() => {
      totalListHeightChanged(1_000);
      setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_030, scrollTop: 500 });
      totalListHeightChanged(1_030);
    });
    scrollWrites.mockClear();
    fireEvent.scroll(scroller);
    flushAnimationFrames();

    // 30px short of the reported extent. The pin loop read exactly this as an
    // unmet bottom intent and wrote scrollTop again on the next frame, which
    // is what sustained the up-and-down jitter.
    expect(scrollWrites).not.toHaveBeenCalled();
    expect(scroller.scrollTop).toBe(500);
  });

  it("follows the last item from the bottom control and stays free after a later wheel-up", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -100 });
    fireEvent.scroll(scroller);
    const button = screen.getByTestId("chat-scroll-to-bottom-button");
    expect(button).toBeEnabled();

    fireEvent.click(button);
    flushAnimationFrames();

    expect(harness.scrollToIndex).toHaveBeenCalledExactlyOnceWith({
      index: "LAST",
      align: "end",
      behavior: "auto",
    });
    expect(scrollWrites).not.toHaveBeenCalled();

    harness.scrollToIndex.mockClear();
    harness.autoscrollToBottom.mockClear();
    harness.scrollToIndex.mockClear();
    fireEvent.wheel(scroller, { deltaY: -80 });
    act(() => totalListHeightChanged(1_100));
    flushAnimationFrames();

    expect(harness.scrollToIndex).not.toHaveBeenCalled();
    expect(harness.autoscrollToBottom).not.toHaveBeenCalled();
    expect(scrollWrites).not.toHaveBeenCalled();
  });

  it("keeps the timestamp and message actions reachable when the last row grows after returning to bottom", () => {
    const resizeObservers: Array<{
      callback: ResizeObserverCallback;
      targets: Set<Element>;
    }> = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        private readonly record: (typeof resizeObservers)[number];

        constructor(callback: ResizeObserverCallback) {
          this.record = { callback, targets: new Set() };
          resizeObservers.push(this.record);
        }

        disconnect(): void {
          this.record.targets.clear();
        }

        observe(target: Element): void {
          this.record.targets.add(target);
        }

        unobserve(target: Element): void {
          this.record.targets.delete(target);
        }
      },
    );
    renderList();
    const scroller = primeAtBottom();
    const lastMeta = screen.getAllByTestId("message-meta").at(-1);
    const lastRow = lastMeta?.closest('[data-chat-last-rendered-row="true"]');
    expect(lastRow).toBeInstanceOf(HTMLElement);

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    fireEvent.click(screen.getByTestId("chat-scroll-to-bottom-button"));
    expect(harness.scrollToIndex).toHaveBeenCalledExactlyOnceWith({
      index: "LAST",
      align: "end",
      behavior: "auto",
    });
    // Virtuoso lands the last item's end at the viewport bottom, which is the
    // composer top now that the reserved inset lives inside that item.
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 500 });
    fireEvent.scroll(scroller);

    const lastRowObserver = resizeObservers.find(({ targets }) =>
      lastRow ? targets.has(lastRow) : false,
    );
    expect(lastRowObserver).toBeDefined();
    const notifyLastRowHeight = (height: number) => {
      lastRowObserver?.callback(
        [{ contentRect: { height }, target: lastRow } as ResizeObserverEntry],
        {} as ResizeObserver,
      );
    };
    act(() => notifyLastRowHeight(100));
    scrollWrites.mockClear();
    harness.autoscrollToBottom.mockClear();
    harness.scrollToIndex.mockClear();

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_024, scrollTop: 500 });
    act(() => notifyLastRowHeight(124));
    flushAnimationFrames();

    expectFollowDelegated();
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_024, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    scrollWrites.mockClear();
    harness.autoscrollToBottom.mockClear();
    harness.scrollToIndex.mockClear();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_048, scrollTop: 200 });
    act(() => notifyLastRowHeight(148));
    flushAnimationFrames();

    expectNoFollow();
    expect(scroller.scrollTop).toBe(200);
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "false");
  });

  it("arms follow when a newly mounted streaming tail reports its first measured height", () => {
    const resizeObservers: Array<{
      callback: ResizeObserverCallback;
      targets: Set<Element>;
    }> = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        private readonly record: (typeof resizeObservers)[number];

        constructor(callback: ResizeObserverCallback) {
          this.record = { callback, targets: new Set() };
          resizeObservers.push(this.record);
        }

        disconnect(): void {
          this.record.targets.clear();
        }

        observe(target: Element): void {
          this.record.targets.add(target);
        }

        unobserve(target: Element): void {
          this.record.targets.delete(target);
        }
      },
    );
    renderList({
      isAgentRunning: true,
      streamingContentBlocks: [{ type: "text", text: "new streaming tail" }],
    });
    const scroller = primeAtBottom();
    const lastRow = screen.getByTestId("integrated-chat-messages").querySelector(
      '[data-chat-last-rendered-row="true"]',
    );
    expect(lastRow).toBeInstanceOf(HTMLElement);

    const lastRowObserver = resizeObservers.find(({ targets }) =>
      lastRow ? targets.has(lastRow) : false,
    );
    expect(lastRowObserver).toBeDefined();
    setScrollerGeometry(scroller, {
      clientHeight: 500,
      scrollHeight: 1_100,
      scrollTop: 500,
    });
    act(() => {
      lastRowObserver?.callback(
        [{ contentRect: { height: 160 }, target: lastRow } as ResizeObserverEntry],
        {} as ResizeObserver,
      );
    });
    flushAnimationFrames();

    expectFollowDelegated();
    expect(scroller.scrollTop).toBe(500);
  });

  it("arms follow for composer spacer resizes only while the reader is following", () => {
    const resizeObservers: Array<{
      callback: ResizeObserverCallback;
      targets: Set<Element>;
    }> = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        private readonly record: (typeof resizeObservers)[number];

        constructor(callback: ResizeObserverCallback) {
          this.record = { callback, targets: new Set() };
          resizeObservers.push(this.record);
        }

        disconnect(): void {
          this.record.targets.clear();
        }

        observe(target: Element): void {
          this.record.targets.add(target);
        }

        unobserve(target: Element): void {
          this.record.targets.delete(target);
        }
      },
    );
    renderList();
    const scroller = primeAtBottom();
    const spacer = screen.getByTestId("chat-transcript-bottom-spacer");
    const spacerObserver = resizeObservers.find(({ targets }) => targets.has(spacer));
    expect(spacerObserver).toBeDefined();
    const resizeSpacer = (height: number) => {
      spacerObserver?.callback(
        [{ contentRect: { height }, target: spacer } as ResizeObserverEntry],
        {} as ResizeObserver,
      );
    };

    act(() => resizeSpacer(40));
    scrollWrites.mockClear();
    harness.autoscrollToBottom.mockClear();
    harness.scrollToIndex.mockClear();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_040, scrollTop: 500 });
    act(() => resizeSpacer(80));
    flushAnimationFrames();

    expectFollowDelegated();

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_040, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    scrollWrites.mockClear();
    harness.autoscrollToBottom.mockClear();
    harness.scrollToIndex.mockClear();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_080, scrollTop: 200 });
    act(() => resizeSpacer(120));
    flushAnimationFrames();

    expectNoFollow();
    expect(scroller.scrollTop).toBe(200);
  });

  it("leaves controller follow state untouched for a prepend epoch", () => {
    const onLoadOlderMessages = vi.fn();
    renderList({ hasOlderMessages: true, onLoadOlderMessages });
    const scroller = primeAtBottom();
    const startReached = callback<(index: number) => void>("startReached");

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();
    act(() => startReached(0));
    flushAnimationFrames();

    expect(onLoadOlderMessages).toHaveBeenCalledOnce();
    expect(scrollWrites).not.toHaveBeenCalled();
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
  });

  it("reopens the prepend epoch when older items land after an async fetch", () => {
    const onLoadOlderMessages = vi.fn();
    const { rerender } = renderList({
      hasOlderMessages: true,
      onLoadOlderMessages,
      firstItemIndex: 10,
    });
    const scroller = primeAtBottom();
    const startReached = callback<(index: number) => void>("startReached");
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    act(() => startReached(10));
    flushAnimationFrames();
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();

    rerender(
      <ChatMessageList
        {...defaultProps}
        hasOlderMessages
        onLoadOlderMessages={onLoadOlderMessages}
        firstItemIndex={7}
        messages={messages(6, 20)}
      />,
    );
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_300, scrollTop: 500 });
    act(() => {
      totalListHeightChanged(1_300);
      fireEvent.scroll(scroller);
    });
    flushAnimationFrames();

    expect(onLoadOlderMessages).toHaveBeenCalledOnce();
    expect(scrollWrites).not.toHaveBeenCalled();
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "false");
  });

  it("resets the controller and leaves a switched conversation's landing to Virtuoso", () => {
    const { rerender } = renderList();
    primeAtBottom();
    harness.scrollToIndex.mockClear();
    scrollWrites.mockClear();

    rerender(
      <ChatMessageList
        {...defaultProps}
        conversationId="conversation-b"
        messages={messages(2, 20)}
      />,
    );
    const scroller = getScroller();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 900, scrollTop: 300 });
    flushAnimationFrames();

    // The conversation key remounts Virtuoso, so its initial-scroll gate lands
    // the new transcript; a controller follow here would be a second writer.
    expect(harness.props?.initialTopMostItemIndex).toEqual({ index: "LAST", align: "end" });
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
    expect(scrollWrites).not.toHaveBeenCalled();
  });

  it("uses a non-following start-aligned timestamp jump and ignores subsequent growth", () => {
    const timestamp = defaultProps.messages[1]?.createdAt;
    expect(timestamp).toBeDefined();
    renderList({ scrollToTimestamp: timestamp });
    const scroller = getScroller();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 480 });
    flushAnimationFrames();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");

    expect(harness.scrollToIndex).toHaveBeenCalledWith(
      expect.objectContaining({ index: 1, align: "start" }),
    );
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();
    act(() => {
      totalListHeightChanged(1_000);
      totalListHeightChanged(1_060);
    });
    flushAnimationFrames();

    expect(scrollWrites).not.toHaveBeenCalled();
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
    expect(scroller.scrollTop).toBe(480);
  });

  it("does not repeat a timestamp jump when messages receive a new array identity", () => {
    const timestamp = defaultProps.messages[1]?.createdAt;
    expect(timestamp).toBeDefined();
    const { rerender } = renderList({ scrollToTimestamp: timestamp });

    expect(harness.scrollToIndex).toHaveBeenCalledWith(
      expect.objectContaining({ index: 1, align: "start" }),
    );
    primeAtBottom();
    rerender(
      <ChatMessageList
        {...defaultProps}
        messages={[...defaultProps.messages]}
        scrollToTimestamp={timestamp}
      />,
    );
    flushAnimationFrames();

    expect(harness.scrollToIndex).not.toHaveBeenCalled();
  });

  it("keeps a reader pinned after returning to bottom when timestamp messages finalize", () => {
    const timestamp = defaultProps.messages[1]?.createdAt;
    expect(timestamp).toBeDefined();
    const { rerender } = renderList({ scrollToTimestamp: timestamp });
    const scroller = getScroller();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 480 });
    flushAnimationFrames();

    fireEvent.click(screen.getByTestId("chat-scroll-to-bottom-button"));
    flushAnimationFrames();
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();

    rerender(
      <ChatMessageList
        {...defaultProps}
        messages={[...defaultProps.messages]}
        scrollToTimestamp={timestamp}
      />,
    );
    flushAnimationFrames();

    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");
    expect(scrollWrites).not.toHaveBeenCalled();
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
  });

  it("jumps again when history requests a different timestamp", () => {
    const firstTimestamp = defaultProps.messages[1]?.createdAt;
    const secondTimestamp = defaultProps.messages[2]?.createdAt;
    expect(firstTimestamp).toBeDefined();
    expect(secondTimestamp).toBeDefined();
    const { rerender } = renderList({ scrollToTimestamp: firstTimestamp });
    primeAtBottom();

    rerender(
      <ChatMessageList
        {...defaultProps}
        messages={[...defaultProps.messages]}
        scrollToTimestamp={secondTimestamp}
      />,
    );
    flushAnimationFrames();

    expect(harness.scrollToIndex).toHaveBeenCalledWith(
      expect.objectContaining({ index: 2, align: "start" }),
    );
  });

  it("keeps Virtuoso components stable across streaming rerenders", () => {
    const { rerender } = renderList({ isAgentRunning: true });
    const initialComponents = harness.componentsHistory.at(-1);

    rerender(
      <ChatMessageList
        {...defaultProps}
        isAgentRunning
        streamingContentBlocks={[{ type: "text", text: "first streamed chunk" }]}
      />,
    );
    const afterFirstChunk = harness.componentsHistory.at(-1);
    rerender(
      <ChatMessageList
        {...defaultProps}
        isAgentRunning
        streamingContentBlocks={[{ type: "text", text: "second streamed chunk" }]}
      />,
    );

    expect(initialComponents).toBe(afterFirstChunk);
    expect(harness.componentsHistory.at(-1)).toBe(initialComponents);
  });

  it("keeps the transcript keyboard-focusable while ignoring key events from editable descendants", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");
    const input = document.createElement("input");
    scroller.append(input);

    fireEvent.keyDown(input, { key: "ArrowUp" });
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_040, scrollTop: 500 });
    act(() => totalListHeightChanged(1_040));
    flushAnimationFrames();

    expect(scroller).toHaveAttribute("tabindex", "0");
    expectFollowDelegated();
  });

  it("unfollows for transcript PageUp while leaving editable key presses pinned", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");
    const input = document.createElement("input");
    scroller.append(input);
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 220 });
    // A real PageUp scrolls, and the scroll event is what publishes at-bottom.
    fireEvent.scroll(scroller);

    fireEvent.keyDown(input, { key: "PageUp" });
    fireEvent.keyDown(scroller, { key: "PageUp" });
    fireEvent.keyDown(scroller, { key: "PageDown" });
    scrollWrites.mockClear();
    harness.autoscrollToBottom.mockClear();
    harness.scrollToIndex.mockClear();
    act(() => totalListHeightChanged(1_040));
    flushAnimationFrames();

    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "false");
    expect(scrollWrites).not.toHaveBeenCalled();
    expect(harness.autoscrollToBottom).not.toHaveBeenCalled();
  });

  it("unfollows after pointer-driven upward scrolling and ignores growth after pointer release", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");

    fireEvent.pointerDown(scroller);
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 220 });
    fireEvent.scroll(scroller);
    fireEvent.pointerUp(scroller);
    scrollWrites.mockClear();
    act(() => totalListHeightChanged(1_040));
    flushAnimationFrames();

    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "false");
    expect(scrollWrites).not.toHaveBeenCalled();
  });

  it("ends a pointer session released outside the scroller and removes the window listener", () => {
    const debugSpy = vi.spyOn(logger, "debug");
    const { unmount } = renderList();
    const scroller = primeAtBottom();

    fireEvent.pointerDown(scroller);
    fireEvent.pointerUp(window);
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 220 });
    fireEvent.scroll(scroller);
    flushAnimationFrames();

    // Nothing drags an unattributed scroll back down: the reader stays where
    // the scroller left them and the control simply becomes reachable.
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "false");
    expect(scrollWrites).not.toHaveBeenCalled();
    expect(scroller.scrollTop).toBe(220);

    unmount();
    debugSpy.mockClear();
    fireEvent.pointerUp(window);

    expect(debugSpy).not.toHaveBeenCalled();
  });

  it("arms follow for a following transcript after its scroller resize observer reports growth", () => {
    let onResize: ResizeObserverCallback | null = null;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: ResizeObserverCallback) {
          onResize = callback;
        }

        disconnect(): void {}
        observe(): void {}
        unobserve(): void {}
      },
    );
    renderList();
    const scroller = primeAtBottom();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_100, scrollTop: 500 });
    scrollWrites.mockClear();

    act(() => onResize?.([], {} as ResizeObserver));

    expect(harness.autoscrollToBottom).toHaveBeenCalled();
    expect(scrollWrites).not.toHaveBeenCalled();
    expect(scroller.scrollTop).toBe(500);
  });

  it("leaves streaming-start follow to Virtuoso's followOutput", () => {
    const { rerender } = renderList();
    const scroller = primeAtBottom();
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_100, scrollTop: 500 });
    rerender(<ChatMessageList {...defaultProps} isAgentRunning />);
    flushAnimationFrames();

    // isAgentRunning appends a streaming timeline row, which is precisely what
    // followOutput triggers on. A controller follow here only adds a writer.
    expect(followOutput()(true)).toBe("auto");
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
    expect(scrollWrites).not.toHaveBeenCalled();
  });

  it("vetoes followOutput once the reader has scrolled away", () => {
    renderList();
    const scroller = primeAtBottom();
    expect(followOutput()(true)).toBe("auto");
    expect(followOutput()(false)).toBe(false);

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 220 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);

    // Virtuoso passes `isAtBottom || scrollingInProgress`, so a bare
    // `followOutput` would drag a reader who just wheeled up and received a
    // message inside the scroll-in-progress window.
    expect(followOutput()(true)).toBe(false);
  });

  it("arms follow for a following reader when the finalized provider message is revealed", () => {
    const providerMessages: ChatMessageData[] = [
      ...messages(2),
      {
        id: "provider-empty",
        role: "assistant",
        content: "",
        createdAt: new Date(2026, 0, 1, 12, 10).toISOString(),
        toolCalls: null,
        contentBlocks: null,
      },
    ];
    const { rerender } = renderList({ messages: providerMessages, isAgentRunning: true });
    const scroller = primeAtBottom();
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();

    // The reveal swaps a live row for its persisted twin without changing
    // totalCount, so followOutput alone would not fire.
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_100, scrollTop: 500 });
    rerender(<ChatMessageList {...defaultProps} messages={providerMessages} isAgentRunning={false} />);
    flushAnimationFrames();

    expectFollowDelegated();
  });

  it("restores the captured at-bottom anchor while expanding a persisted tool-call group", () => {
    const toolCallMessages: ChatMessageData[] = [
      ...messages(1),
      {
        id: "tool-call-1",
        role: "assistant",
        content: "First tool call",
        createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
        toolCalls: null,
        contentBlocks: [{ type: "tool_use", id: "tool-1", name: "read_file", arguments: {} }],
        timelineSequence: 10,
      },
      {
        id: "tool-call-2",
        role: "assistant",
        content: "Second tool call",
        createdAt: new Date(2026, 0, 1, 12, 2).toISOString(),
        toolCalls: null,
        contentBlocks: [{ type: "tool_use", id: "tool-2", name: "read_file", arguments: {} }],
        timelineSequence: 11,
      },
    ];
    renderList({ messages: toolCallMessages });
    primeAtBottom();
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();

    const toggle = screen.getByTestId("tool-call-group-toggle");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    flushAnimationFrames();

    expect(screen.getByTestId("tool-call-group-toggle")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("First tool call")).toBeInTheDocument();
    expect(screen.getByText("Second tool call")).toBeInTheDocument();
    expectFollowDelegated();
  });

  it("restores the captured at-bottom anchor while toggling a persisted thinking group", () => {
    const thinkingMessages: ChatMessageData[] = [
      ...messages(1),
      {
        id: "thinking-1",
        role: "assistant",
        content: "",
        createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
        contentBlocks: [{ type: "thinking", text: "First thought", durationMs: 1_000 }],
        timelineSequence: 10,
      },
      {
        id: "thinking-2",
        role: "assistant",
        content: "",
        createdAt: new Date(2026, 0, 1, 12, 2).toISOString(),
        contentBlocks: [{ type: "thinking", text: "Second thought", durationMs: 2_000 }],
        timelineSequence: 11,
      },
    ];
    renderList({ messages: thinkingMessages });
    primeAtBottom();
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();

    const toggle = screen.getByTestId("thinking-group-toggle");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(toggle);
    flushAnimationFrames();

    expect(screen.getByTestId("thinking-group-toggle")).toHaveAttribute("aria-expanded", "false");
    expect(harness.autoscrollToBottom).toHaveBeenCalled();
    expect(scrollWrites).not.toHaveBeenCalled();
  });

  it("preserves a free reader's anchor while collapsing a persisted thinking group", () => {
    const thinkingMessages: ChatMessageData[] = [
      ...messages(1),
      {
        id: "thinking-1",
        role: "assistant",
        content: "",
        createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
        contentBlocks: [{ type: "thinking", text: "First thought", durationMs: 1_000 }],
        timelineSequence: 10,
      },
      {
        id: "thinking-2",
        role: "assistant",
        content: "",
        createdAt: new Date(2026, 0, 1, 12, 2).toISOString(),
        contentBlocks: [{ type: "thinking", text: "Second thought", durationMs: 2_000 }],
        timelineSequence: 11,
      },
    ];
    renderList({ messages: thinkingMessages });
    const scroller = primeAtBottom();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 300 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);

    const toggle = screen.getByTestId("thinking-group-toggle");
    scroller.getBoundingClientRect = () => new DOMRect(0, 0, 100, 500);
    toggle.getBoundingClientRect = () => new DOMRect(
      0,
      toggle.getAttribute("aria-expanded") === "true" ? 120 : 60,
      100,
      24,
    );
    fireEvent.click(toggle);

    expect(scroller.scrollTop).toBe(240);
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "false");
  });

  it("keeps persisted delegated cards promoted while generic tool details are collapsed", () => {
    const activityMessages: ChatMessageData[] = [
      {
        id: "generic-tool",
        role: "assistant",
        content: "Generic tool detail",
        createdAt: "2026-07-15T10:00:00Z",
        contentBlocks: [{
          type: "tool_use",
          id: "read-1",
          name: "Read",
          arguments: { file_path: "src/app.ts" },
        }],
        timelineSequence: 20,
      },
      {
        id: "delegated-tool",
        role: "assistant",
        content: "Delegated task card",
        createdAt: "2026-07-15T10:00:01Z",
        contentBlocks: [{
          type: "tool_use",
          id: "delegate-1",
          name: "ralphx::delegate_start",
          arguments: { agent_name: "ralphx-general-explorer" },
          result: { job_id: "job-1", status: "running" },
        }],
        timelineSequence: 21,
      },
    ];

    renderList({ messages: activityMessages });

    const toggle = screen.getByRole("button", {
      name: "Agent called 2 tools and delegated 1 agent. Expand tool details.",
    });
    expect(screen.queryByText("Generic tool detail")).not.toBeInTheDocument();
    expect(screen.getByText("Delegated task card")).toBeInTheDocument();

    fireEvent.click(toggle);
    expect(screen.getByText("Generic tool detail")).toBeInTheDocument();
    expect(screen.getAllByText("Delegated task card")).toHaveLength(1);
  });

  it("keeps the persisted delegate card visible when suppressing the current-turn snapshot", () => {
    const delegatedTask = {
      toolUseId: "delegate-live",
      toolName: "delegate_start",
      description: "Inspect the chat pipeline",
      subagentType: "delegated",
      model: "gpt-5.6",
      status: "running" as const,
      startedAt: 1,
      childToolCalls: [],
      delegatedJobId: "job-live",
    };

    renderList({
      messages: [
        {
          id: "parent-request",
          role: "user",
          content: "Inspect the chat pipeline",
          createdAt: "2026-07-15T10:00:00Z",
        },
        {
          id: "provider-snapshot",
          role: "assistant",
          content: "",
          createdAt: "2026-07-15T10:00:01Z",
        },
        {
          id: "persisted-delegate-lifecycle",
          role: "assistant",
          content: "Persisted delegate lifecycle",
          createdAt: "2026-07-15T10:00:02Z",
          timelineSequence: 20,
          contentBlocks: [{
            type: "tool_use",
            id: "delegate-live",
            name: "delegate_start",
            arguments: { prompt: "Inspect the chat pipeline" },
            result: { job_id: "job-live", status: "running" },
          }],
        },
      ],
      isAgentRunning: true,
      streamingContentBlocks: [{ type: "task", toolUseId: delegatedTask.toolUseId }],
      streamingTasks: new Map([[delegatedTask.toolUseId, delegatedTask]]),
    });

    expect(screen.queryByText("provider-snapshot")).not.toBeInTheDocument();
    expect(screen.getByText("Persisted delegate lifecycle")).toBeInTheDocument();
  });

  it("keeps restored persisted siblings visible when a late live block arrives", () => {
    renderList({
      messages: [
        {
          id: "turn-two-user",
          role: "user",
          content: "Inspect the timeline",
          createdAt: "2026-07-15T10:00:00Z",
        },
        {
          id: "persisted-text",
          parentMessageId: "turn-two-provider",
          role: "assistant",
          content: "Persisted text before the tool",
          createdAt: "2026-07-15T10:00:01Z",
          timelineSequence: 20,
          timelineStatus: "streaming",
          contentBlocks: [{
            type: "text",
            text: "Persisted text before the tool",
          }],
        },
        {
          id: "persisted-tool",
          parentMessageId: "turn-two-provider",
          role: "assistant",
          content: "Persisted tool call",
          createdAt: "2026-07-15T10:00:02Z",
          timelineSequence: 21,
          timelineStatus: "streaming",
          contentBlocks: [{
            type: "tool_use",
            id: "grep-persisted",
            name: "Grep",
            arguments: { pattern: "timeline" },
          }],
        },
      ],
      isAgentRunning: true,
      streamingContentBlocks: [{
        type: "text",
        text: "Late live tail",
        seq: 22,
      }],
    });

    expect(screen.getByText("Persisted text before the tool")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", {
      name: "Agent called 1 tool. Expand tool details.",
    }));
    expect(screen.getByText("Persisted tool call")).toBeInTheDocument();
    expect(screen.getByText("Late live tail")).toBeInTheDocument();
  });

  it("folds non-adjacent terminal delegation rows into the original start row", () => {
    const messages: ChatMessageData[] = [
      {
        id: "delegate-start-message",
        role: "assistant",
        content: "start",
        createdAt: "2026-07-15T10:00:00Z",
        timelineSequence: 20,
        contentBlocks: [{
          type: "tool_use",
          id: "call-start",
          name: "delegate_start",
          arguments: { prompt: "Inspect" },
          result: { job_id: "job-non-adjacent", status: "running" },
        }],
      },
      {
        id: "intervening-text",
        role: "assistant",
        content: "Continuing parent work",
        createdAt: "2026-07-15T10:00:01Z",
        timelineSequence: 21,
      },
      {
        id: "delegate-terminal-message",
        role: "assistant",
        content: "terminal",
        createdAt: "2026-07-15T10:00:02Z",
        timelineSequence: 22,
        contentBlocks: [{
          type: "tool_use",
          id: "delegation-terminal:job-non-adjacent",
          name: "delegate_terminal",
          arguments: { job_id: "job-non-adjacent" },
          result: {
            job_id: "job-non-adjacent",
            status: "completed",
            content: "Delegated result",
          },
        }],
      },
    ];

    const folded = foldDelegationTimelineMessages(messages);

    expect(folded.map((message) => message.id)).toEqual([
      "delegate-start-message",
      "intervening-text",
    ]);
    expect(folded[0]?.contentBlocks?.[0]).toMatchObject({
      name: "delegate_start",
      result: {
        job_id: "job-non-adjacent",
        status: "completed",
        content: "Delegated result",
      },
    });
  });

  it("summarizes persisted tool-call groups from hydrated diff metadata", () => {
    renderList({
      messages: [
        {
          id: "hydrated-edit-message",
          role: "assistant",
          content: "Hydrated edit detail",
          createdAt: "2026-07-15T10:00:00Z",
          contentBlocks: [{
            type: "tool_use",
            id: "hydrated-edit",
            name: "Edit",
            arguments: {},
          }],
          toolCalls: [{
            id: "hydrated-edit",
            name: "Edit",
            arguments: {},
            diffContext: {
              filePath: "src/hydrated.ts",
              oldFileExists: true,
            },
          }],
          timelineSequence: 20,
        },
      ],
    });

    expect(screen.getByRole("button", {
      name: "Agent called 1 tool and edited 1 file. Expand tool details.",
    })).toBeInTheDocument();
    expect(screen.queryByText("Hydrated edit detail")).not.toBeInTheDocument();
  });

  it("keeps a delegated card promoted when an earlier persisted tool block is malformed", () => {
    const activityMessages: ChatMessageData[] = [
      {
        id: "malformed-tool",
        role: "assistant",
        content: "Malformed tool detail",
        createdAt: "2026-07-15T10:00:00Z",
        contentBlocks: [{
          type: "tool_use",
          id: "missing-name",
          arguments: {},
        }],
        timelineSequence: 20,
      },
      {
        id: "delegated-after-malformed",
        role: "assistant",
        content: "Delegated task after malformed tool",
        createdAt: "2026-07-15T10:00:01Z",
        contentBlocks: [{
          type: "tool_use",
          id: "delegate-after-malformed",
          name: "ralphx::delegate_start",
          arguments: { agent_name: "ralphx-general-explorer" },
          result: { job_id: "job-after-malformed", status: "running" },
        }],
        timelineSequence: 21,
      },
    ];

    renderList({ messages: activityMessages });

    expect(screen.getByRole("button", {
      name: "Agent called 1 tool and delegated 1 agent. Expand tool details.",
    })).toBeInTheDocument();
    expect(screen.getByText("Delegated task after malformed tool")).toBeInTheDocument();
    expect(screen.queryByText("Malformed tool detail")).not.toBeInTheDocument();
  });

  it("summarizes mixed live file activity while keeping the delegated task visible", () => {
    const delegatedTask = {
      toolUseId: "delegate-live",
      toolName: "mcp__ralphx__delegate_start",
      description: "Inspect the chat pipeline",
      subagentType: "delegated",
      model: "gpt-5.5",
      status: "running" as const,
      startedAt: 1,
      childToolCalls: [],
      delegatedJobId: "job-live",
    };

    renderList({
      messages: [],
      isAgentRunning: true,
      streamingContentBlocks: [
        {
          type: "tool_use",
          toolCall: {
            id: "write-live",
            name: "Write",
            arguments: { file_path: "src/new.ts" },
            diffContext: { filePath: "src/new.ts", oldFileExists: false },
          },
        },
        { type: "task", toolUseId: delegatedTask.toolUseId },
      ],
      streamingTasks: new Map([[delegatedTask.toolUseId, delegatedTask]]),
    });

    expect(screen.getByRole("button", {
      name: "Agent called 2 tools, created 1 file, and delegated 1 agent. Expand tool details.",
    })).toBeInTheDocument();
    expect(screen.getByText("task")).toBeInTheDocument();
  });

  it("renders applied persona attribution on the matching transcript run boundary", async () => {
    renderList({
      messages: [
        {
          id: "assistant-run-message",
          role: "assistant",
          content: "Applied persona response",
          createdAt: "2026-07-13T06:19:00.000Z",
          runId: "run-persona",
          providerHarness: "codex",
        },
      ],
      agentPersonasEnabled: true,
      agentRun: {
        id: "run-persona",
        conversationId: "conversation-a",
        status: "running",
        startedAt: "2026-07-13T06:19:00.000Z",
        completedAt: null,
        errorMessage: null,
        modelId: null,
        modelLabel: null,
        personaSlug: "design-voice",
        personaVersion: 2,
        personaInjected: true,
      },
    });

    const badge = screen.getByTestId("persona-run-badge");
    expect(badge).toHaveTextContent("design-voice");
    fireEvent.pointerMove(badge);
    expect(
      await screen.findByRole("tooltip", {
        name: "design-voice · v2 — applied to this run",
      }),
    ).toBeInTheDocument();
  });

  it("does not render persona attribution for another run or when the flag is off", () => {
    const assistantMessages: ChatMessageData[] = [
      {
        id: "assistant-run-message",
        role: "assistant",
        content: "No matching persona badge",
        createdAt: "2026-07-13T06:19:00.000Z",
        runId: "older-run",
        providerHarness: "claude",
      },
    ];
    const agentRun = {
      id: "run-persona",
      conversationId: "conversation-a",
      status: "running" as const,
      startedAt: "2026-07-13T06:19:00.000Z",
      completedAt: null,
      errorMessage: null,
      modelId: null,
      modelLabel: null,
      personaId: "persona-design-voice",
      personaSlug: "design-voice",
      personaVersion: 2,
      personaInjected: false,
      personaSkippedReason: "persona_not_injected",
    };
    const { rerender } = renderList({
      messages: assistantMessages,
      agentPersonasEnabled: true,
      agentRun,
    });
    expect(screen.queryByTestId("persona-run-badge")).not.toBeInTheDocument();

    rerender(
      <ChatMessageList
        {...defaultProps}
        messages={[{ ...assistantMessages[0]!, runId: "run-persona" }]}
        agentPersonasEnabled={false}
        agentRun={agentRun}
      />,
    );
    expect(screen.queryByTestId("persona-run-badge")).not.toBeInTheDocument();

    rerender(
      <ChatMessageList
        {...defaultProps}
        messages={[{ ...assistantMessages[0]!, runId: "run-persona" }]}
        agentPersonasEnabled
        agentRun={agentRun}
      />,
    );
    expect(screen.getByTestId("persona-run-badge")).toHaveTextContent(
      "design-voice not applied",
    );
  });

  it("renders body-free attribution for older persisted transcript runs", () => {
    renderList({
      messages: [
        {
          id: "older-assistant-run",
          role: "assistant",
          content: "Older attributed response",
          createdAt: "2026-07-13T06:18:00.000Z",
          runId: "run-persona-older",
        },
      ],
      agentPersonasEnabled: true,
      personaRuns: [
        {
          id: "run-persona-older",
          personaId: "persona-careful-reviewer",
          personaSlug: "careful-reviewer",
          personaVersion: 1,
          personaInjected: true,
          personaSkippedReason: null,
        },
      ],
    });

    expect(screen.getByTestId("persona-run-badge")).toHaveTextContent(
      "careful-reviewer",
    );
  });

  it("forwards wheel movement from the bottom control to its scroller", () => {
    renderList();
    const scroller = primeAtBottom();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 220 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    scrollWrites.mockClear();
    Object.defineProperty(scroller, "scrollBy", { configurable: true, value: undefined });

    fireEvent.wheel(screen.getByTestId("chat-scroll-to-bottom-button"), { deltaY: 30, deltaX: 4 });

    expect(scrollWrites).toHaveBeenCalledWith(
      expect.objectContaining({ left: 4, top: 250, behavior: "auto" }),
    );
    expect(scroller.scrollTop).toBe(250);
  });

  it("forwards wheel movement from the bottom control through scrollBy when available", () => {
    renderList();
    const scroller = primeAtBottom();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 220 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    const scrollBy = vi.fn();
    Object.defineProperty(scroller, "scrollBy", { configurable: true, value: scrollBy });

    fireEvent.wheel(screen.getByTestId("chat-scroll-to-bottom-button"), { deltaY: 30, deltaX: 4 });

    expect(scrollBy).toHaveBeenCalledExactlyOnceWith({ left: 4, top: 30, behavior: "auto" });
  });

  it("keeps following growth after a cancelled pointer session and internal bottom clamp scroll", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");

    fireEvent.pointerDown(scroller);
    fireEvent.pointerCancel(scroller);
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 900, scrollTop: 400 });
    fireEvent.scroll(scroller);
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_100, scrollTop: 400 });
    act(() => totalListHeightChanged(1_100));
    flushAnimationFrames();

    expectFollowDelegated();
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");
  });

  it("does not treat wheel-up inside a nested scrollable block as an away intent", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");
    const nested = document.createElement("pre");
    scroller.append(nested);
    Object.defineProperties(nested, {
      clientHeight: { configurable: true, value: 100 },
      scrollHeight: { configurable: true, value: 300 },
    });
    vi.spyOn(window, "getComputedStyle").mockImplementation((element) => {
      if (element === nested) {
        return { overflowY: "scroll" } as CSSStyleDeclaration;
      }
      return { overflowY: "visible", paddingBottom: "0px", visibility: "visible" } as CSSStyleDeclaration;
    });

    fireEvent.wheel(nested, { deltaY: -80 });
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_040, scrollTop: 500 });
    act(() => {
      totalListHeightChanged(1_000);
      totalListHeightChanged(1_040);
    });
    flushAnimationFrames();

    expectFollowDelegated();
  });
});
