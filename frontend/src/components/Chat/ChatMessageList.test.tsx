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
import {
  ChatMessageList,
  type ChatMessageData,
} from "./ChatMessageList";
import { foldDelegationTimelineMessages } from "./delegation-timeline";

const harness = vi.hoisted(() => ({
  props: null as Record<string, unknown> | null,
  componentsHistory: [] as unknown[],
  scrollToIndex: vi.fn(),
}));

const messageAttachments = vi.hoisted(() => vi.fn(() => ({ data: new Map() })));

vi.mock("@/hooks/useMessageAttachments", () => ({
  useMessageAttachments: (...args: unknown[]) => messageAttachments(...args),
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
  };

  const Virtuoso = React.forwardRef<unknown, VirtuosoProps>(function MockVirtuoso(props, ref) {
    const elementRef = React.useRef<HTMLDivElement | null>(null);
    const data = props.data ?? [];
    const Scroller = props.components?.Scroller ?? "div";
    const Header = props.components?.Header;
    const { rangeChanged, scrollerRef } = props;

    const setScroller = React.useCallback((element: HTMLDivElement | null) => {
      if (element) {
        setScrollerGeometry(element, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 480 });
      }
      elementRef.current = element;
    }, []);

    React.useImperativeHandle(ref, () => ({ scrollToIndex: harness.scrollToIndex }));

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
  Object.defineProperties(element, {
    clientHeight: { configurable: true, value: geometry.clientHeight },
    scrollHeight: { configurable: true, value: geometry.scrollHeight },
    scrollTop: { configurable: true, value: geometry.scrollTop, writable: true },
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

it("rehydrates one completed-run widget at the end of persisted run rows", () => {
  renderList({ messages: [
    { id: "run-row-1", role: "assistant", content: "first", createdAt: "2026-01-01T12:00:00Z", finalizedAt: "2026-01-01T12:00:10Z", runId: "run-1" },
    { id: "run-row-2", role: "assistant", content: "last", createdAt: "2026-01-01T12:00:02Z", finalizedAt: "2026-01-01T12:00:42Z", runId: "run-1" },
  ] });

  expect(screen.getAllByTestId("run-attribution-widget")).toHaveLength(1);
  expect(screen.getByTestId("run-attribution-toggle")).toHaveTextContent("Agent worked for 42s");
});

function getScroller(): HTMLElement {
  return screen.getByTestId("mock-virtuoso");
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
  setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 480 });
  flushAnimationFrames();
  scrollWrites.mockClear();
  harness.scrollToIndex.mockClear();
  return scroller;
}

describe("ChatMessageList controller integration", () => {
  beforeEach(() => {
    vi.stubEnv("VITEST", "");
    harness.props = null;
    harness.componentsHistory = [];
    harness.scrollToIndex.mockReset();
    messageAttachments.mockReturnValue({ data: new Map() });
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

  it("pins an initial conversation at the last item and preserves the paint cover until the transcript is ready", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
    const onInitialPaintReady = vi.fn();
    renderList({ initialPaintCoverKey: "conversation-a", onInitialPaintReady });

    expect(screen.getByTestId("chat-transcript-settling-placeholders")).toBeInTheDocument();
    const scroller = getScroller();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 480 });
    flushAnimationFrames();

    expect(harness.scrollToIndex).toHaveBeenCalledWith(
      expect.objectContaining({ index: 2, align: "end" }),
    );
    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 500 }));

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(onInitialPaintReady).toHaveBeenCalledWith("conversation-a");
    expect(screen.queryByTestId("chat-transcript-settling-placeholders")).not.toBeInTheDocument();
  });

  it("coalesces repeated streaming growth into bounded pinned writes", () => {
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

    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 530 }));
    expect(scrollWrites).toHaveBeenCalledTimes(1);
  });

  it("pins when the first post-attach list-height measurement reports content", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_030, scrollTop: 500 });
    act(() => totalListHeightChanged(1_030));
    flushAnimationFrames();

    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 530 }));
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
  });

  it("pins a free reader after a new user message is appended", () => {
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

    expect(harness.scrollToIndex).toHaveBeenCalledWith(
      // behavior is platform-dependent (webkit-safe "auto" vs "smooth")
      expect.objectContaining({ index: 3, align: "end" }),
    );
    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 500 }));
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

    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 540 }));
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");
  });

  it("keeps the active bottom intent through a short pin replay and native scroll event", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");
    let growsOnWrite = true;
    Object.defineProperty(scroller, "scrollTo", {
      configurable: true,
      value: (options: ScrollToOptions) => {
        scrollWrites(options);
        if (typeof options.top === "number") {
          scroller.scrollTop = options.top;
        }
        if (growsOnWrite) {
          growsOnWrite = false;
          setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_060, scrollTop: scroller.scrollTop });
        }
      },
    });

    act(() => {
      totalListHeightChanged(1_000);
      setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_030, scrollTop: 500 });
      totalListHeightChanged(1_030);
    });
    flushAnimationFrames(1);
    fireEvent.scroll(scroller);
    flushAnimationFrames();

    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 560 }));
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");
  });

  it("returns to true bottom from the control and lets wheel-up cancel a pending descent", () => {
    renderList();
    const scroller = primeAtBottom();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -100 });
    fireEvent.scroll(scroller);
    const button = screen.getByTestId("chat-scroll-to-bottom-button");
    expect(button).toBeEnabled();

    fireEvent.click(button);
    flushAnimationFrames();
    expect(scroller.scrollTop).toBe(500);
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_100, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -100 });
    fireEvent.scroll(scroller);
    fireEvent.click(screen.getByTestId("chat-scroll-to-bottom-button"));
    fireEvent.wheel(scroller, { deltaY: -80 });
    scrollWrites.mockClear();
    flushAnimationFrames();

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
    flushAnimationFrames();
    expect(scroller.scrollTop).toBe(500);

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

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_024, scrollTop: 500 });
    act(() => notifyLastRowHeight(124));
    flushAnimationFrames();

    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 524 }));
    expect(scroller.scrollTop).toBe(524);
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_024, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    scrollWrites.mockClear();
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_048, scrollTop: 200 });
    act(() => notifyLastRowHeight(148));
    flushAnimationFrames();

    expect(scrollWrites).not.toHaveBeenCalled();
    expect(scroller.scrollTop).toBe(200);
    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "false");
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

  it("resets the controller and lands a switched conversation at its bottom", () => {
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

    expect(harness.scrollToIndex).toHaveBeenCalledWith(
      expect.objectContaining({ index: 1, align: "end" }),
    );
    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 400 }));
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
    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 540 }));
  });

  it("unfollows for transcript PageUp while leaving editable key presses pinned", () => {
    renderList();
    const scroller = primeAtBottom();
    const totalListHeightChanged = callback<(height: number) => void>("totalListHeightChanged");
    const input = document.createElement("input");
    scroller.append(input);
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 220 });

    fireEvent.keyDown(input, { key: "PageUp" });
    fireEvent.keyDown(scroller, { key: "PageUp" });
    fireEvent.keyDown(scroller, { key: "PageDown" });
    scrollWrites.mockClear();
    act(() => totalListHeightChanged(1_040));
    flushAnimationFrames();

    expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "false");
    expect(scrollWrites).not.toHaveBeenCalled();
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

  it("re-pins a following transcript after its scroller resize observer reports growth", () => {
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

    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 600 }));
    expect(scroller.scrollTop).toBe(600);
  });

  it("pins on streaming start only while the reader is still following", () => {
    const { rerender } = renderList();
    const scroller = primeAtBottom();
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_100, scrollTop: 500 });
    rerender(<ChatMessageList {...defaultProps} isAgentRunning />);
    flushAnimationFrames();

    // isAgentRunning appends a streaming timeline row, so the last index is 3.
    expect(harness.scrollToIndex).toHaveBeenCalledWith(
      expect.objectContaining({ index: 3, align: "end" }),
    );
    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 600 }));

    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_100, scrollTop: 220 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    scrollWrites.mockClear();
    harness.scrollToIndex.mockClear();
    rerender(<ChatMessageList {...defaultProps} isAgentRunning={false} />);
    rerender(<ChatMessageList {...defaultProps} isAgentRunning />);
    flushAnimationFrames();

    expect(scrollWrites).not.toHaveBeenCalled();
    expect(harness.scrollToIndex).not.toHaveBeenCalled();
  });

  it("pins a following reader when the finalized provider message is revealed", () => {
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

    // Grow content so the reveal pin must actually write (a pin at true bottom is a no-op).
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_100, scrollTop: 500 });
    rerender(<ChatMessageList {...defaultProps} messages={providerMessages} isAgentRunning={false} />);
    flushAnimationFrames();

    expect(harness.scrollToIndex).toHaveBeenCalledWith(
      expect.objectContaining({ index: 2, align: "end" }),
    );
    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 600 }));
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
    expect(harness.scrollToIndex).toHaveBeenCalledWith(
      expect.objectContaining({ index: 2, align: "end" }),
    );
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

    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 600 }));
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

    expect(scrollWrites).toHaveBeenCalledWith(expect.objectContaining({ top: 540 }));
  });
});
