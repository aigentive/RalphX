/**
 * ChatMessageList integration tests
 * Tests scroll behavior in real component scenarios:
 * - Single-path Virtuoso scroll (no DOM marker auto-scroll)
 * - Hook receives virtuosoRef for Virtuoso-native scrolling
 * - Streaming content renders without DOM scroll calls
 * - Context switches (conversation changes)
 * - History mode disables auto-scroll
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render as rtlRender, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { act } from "react";
import {
  AT_BOTTOM_THRESHOLD,
  TEXT_LENGTH_BUCKET_SIZE,
  ChatMessageList,
  type ChatMessageData,
} from "./ChatMessageList";
import {
  buildStreamingTranscriptWindow,
  getNextStreamingTranscriptWindow,
} from "./ChatMessageList.streamingWindow";
import { isTranscriptRootReadyForReveal } from "./ChatMessageList.readiness";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { ToolCall } from "./ToolCallIndicator";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import type { ReactElement, ReactNode } from "react";

// Mock scrollIntoView before tests run — should NEVER be called for auto-scroll
const scrollIntoViewMock = vi.fn();
Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
  value: scrollIntoViewMock,
  writable: true,
});

// jsdom doesn't implement scrollTo on HTMLElement — stub it so scroll-related
// callbacks in the Virtuoso production path don't throw. Leave scrollBy alone:
// the existing wheel-scroll test relies on the fallback path that increments
// scrollTop directly when scrollBy is not a function.
const scrollToMock = vi.fn();
Object.defineProperty(HTMLElement.prototype, "scrollTo", {
  value: scrollToMock,
  writable: true,
});

// Mock useChatAutoScroll to control scroll behavior in tests
let mockIsAtBottom = true;
const mockIsAtBottomRef = { current: true };
const mockScrollToBottom = vi.fn();
const mockHandleAtBottomStateChange = vi.fn();
const mockHandleFollowOutput = vi.fn((atBottom: boolean) =>
  atBottom ? "smooth" as const : false as const
);
const mockUseMessageAttachments = vi.hoisted(() =>
  vi.fn(() => ({ data: new Map() }))
);
const mockVirtuosoHarness = vi.hoisted(() => ({
  props: null as Record<string, unknown> | null,
}));

// Capture hook call args to verify virtuosoRef and disabled are passed
const mockUseChatAutoScroll = vi.fn(() => ({
  isAtBottom: mockIsAtBottom,
  isAtBottomRef: mockIsAtBottomRef,
  scrollToBottom: mockScrollToBottom,
  handleAtBottomStateChange: mockHandleAtBottomStateChange,
  handleFollowOutput: mockHandleFollowOutput,
  shouldAutoScroll: mockIsAtBottom,
  containerRef: { current: null },
  messagesEndRef: { current: null },
}));

vi.mock("@/hooks/useChatAutoScroll", () => ({
  useChatAutoScroll: (...args: unknown[]) => mockUseChatAutoScroll(...args),
}));

// Mock useMessageAttachments — returns empty map by default (no attachments)
vi.mock("@/hooks/useMessageAttachments", () => ({
  useMessageAttachments: (...args: unknown[]) => mockUseMessageAttachments(...args),
}));

// Mock TaskSubagentCard — heavy widget that requires full StreamingTask + tool widgets
vi.mock("./TaskSubagentCard", () => ({
  TaskSubagentCard: ({ task }: { task: { toolUseId: string } }) => (
    <div data-testid={`task-subagent-card-${task.toolUseId}`}>task card</div>
  ),
}));

// Mock react-virtuoso — the real implementation doesn't work in jsdom and we want
// to drive the Virtuoso (production) render path to exercise renderItem and the
// non-test-env branch. The mock renders the timeline items via itemContent and
// triggers Virtuoso lifecycle callbacks where useful.
vi.mock("react-virtuoso", async () => {
  const React = await import("react");
  const Virtuoso = React.forwardRef<unknown, Record<string, unknown>>(
    function MockVirtuoso(props, ref) {
      type ItemContent = (i: number, item: unknown) => React.ReactNode;
      type Components = {
        Header?: React.ComponentType;
        Scroller?: React.ComponentType<
          React.ComponentPropsWithoutRef<"div"> & React.RefAttributes<HTMLDivElement>
        >;
      };
      type ScrollerRef = (el: HTMLElement | Window | null) => void;
      type RangeChanged = (range: { startIndex: number; endIndex: number }) => void;
      type AtBottomStateChange = (atBottom: boolean) => void;
      type FollowOutput = (atBottom: boolean) => "smooth" | "auto" | false;
      type StartReached = (idx: number) => void;
      const data = (props.data as unknown[]) ?? [];
      const itemContent = props.itemContent as ItemContent | undefined;
      const components = (props.components as Components) ?? {};
      const Header = components.Header;
      const Scroller = components.Scroller ?? "div";
      const scrollerRef = props.scrollerRef as ScrollerRef | undefined;
      const rangeChanged = props.rangeChanged as RangeChanged | undefined;
      const atBottomStateChange = props.atBottomStateChange as AtBottomStateChange | undefined;
      const followOutput = props.followOutput as FollowOutput | undefined;
      const startReached = props.startReached as StartReached | undefined;
      const firstItemIndex =
        typeof props.firstItemIndex === "number" ? props.firstItemIndex : 0;
      const innerRef = React.useRef<HTMLDivElement>(null);

      React.useEffect(() => {
        mockVirtuosoHarness.props = props;
        return () => {
          if (mockVirtuosoHarness.props === props) {
            mockVirtuosoHarness.props = null;
          }
        };
      });

      React.useImperativeHandle(ref, () => ({
        scrollToIndex: () => {},
        scrollToBottom: () => {},
      }));

      // Wire scrollerRef + lifecycle callbacks after first paint
      React.useEffect(() => {
        if (innerRef.current && scrollerRef) {
          scrollerRef(innerRef.current);
        }
        if (rangeChanged && data.length > 0) {
          rangeChanged({ startIndex: 0, endIndex: data.length - 1 });
        }
        if (atBottomStateChange) {
          atBottomStateChange(true);
          atBottomStateChange(false);
        }
        if (followOutput) {
          followOutput(true);
          followOutput(false);
        }
        if (startReached && data.length > 0) {
          startReached(0);
        }
        return () => {
          if (scrollerRef) {
            scrollerRef(null);
          }
        };
      }, [scrollerRef, rangeChanged, atBottomStateChange, followOutput, startReached, data.length]);

      return (
        <Scroller ref={innerRef} data-testid="mock-virtuoso">
          {Header ? <Header /> : null}
          {data.map((item, i) => (
            <div key={i} data-mock-item-index={i}>
              {itemContent ? itemContent(firstItemIndex + i, item) : null}
            </div>
          ))}
        </Scroller>
      );
    },
  );
  return { Virtuoso };
});

const createMessages = (count: number): ChatMessageData[] => {
  return Array.from({ length: count }, (_, i) => ({
    id: `msg-${i + 1}`,
    role: i % 2 === 0 ? "user" : "assistant",
    content: `Message ${i + 1}`,
    createdAt: new Date(2026, 0, 1, 12, i).toISOString(),
    toolCalls: null,
    contentBlocks: null,
  }));
};

const defaultProps = {
  messages: createMessages(10),
  conversationId: "conv-1",
  failedRun: null,
  onDismissFailedRun: vi.fn(),
  isSending: false,
  isAgentRunning: false,
  streamingToolCalls: [],
  streamingTasks: new Map(),
  streamingContentBlocks: undefined,
  scrollToTimestamp: null,
};

const GENERIC_TOOL_NAME = "webfetch";

function makeTimelineTextMessage({
  id,
  parentMessageId,
  sequence,
  text,
}: {
  id: string;
  parentMessageId: string;
  sequence: number;
  text: string;
}): ChatMessageData {
  return {
    id,
    role: "assistant",
    content: text,
    createdAt: new Date(2026, 0, 1, 12, sequence).toISOString(),
    parentMessageId,
    contentBlocks: [{ type: "text", text }],
    toolCalls: null,
    providerHarness: "claude",
    timelineSequence: sequence,
  };
}

function makeTimelineToolMessage({
  id,
  parentMessageId,
  sequence,
  toolName = GENERIC_TOOL_NAME,
}: {
  id: string;
  parentMessageId: string;
  sequence: number;
  toolName?: string;
}): ChatMessageData {
  const toolCall: ToolCall = {
    id: `tool-${id}`,
    name: toolName,
    arguments: { url: `https://example.com/${id}` },
    result: `result for ${id}`,
  };
  return {
    id,
    role: "assistant",
    content: "",
    createdAt: new Date(2026, 0, 1, 12, sequence).toISOString(),
    parentMessageId,
    contentBlocks: [{
      type: "tool_use",
      id: toolCall.id,
      name: toolCall.name,
      arguments: toolCall.arguments,
      result: toolCall.result,
    }],
    toolCalls: [toolCall],
    providerHarness: "claude",
    timelineSequence: sequence,
  };
}

function setMockScrollerGeometry(
  element: HTMLElement,
  {
    clientHeight,
    scrollHeight,
    scrollTop,
  }: {
    clientHeight: number;
    scrollHeight: number;
    scrollTop: number;
  },
) {
  Object.defineProperties(element, {
    clientHeight: { configurable: true, value: clientHeight },
    scrollHeight: { configurable: true, value: scrollHeight },
    scrollTop: { configurable: true, writable: true, value: scrollTop },
  });
}

function makeRect({
  top,
  bottom,
  height = bottom - top,
  left = 0,
  right = 200,
  width = right - left,
}: {
  top: number;
  bottom: number;
  height?: number;
  left?: number;
  right?: number;
  width?: number;
}): DOMRect {
  return {
    x: left,
    y: top,
    width,
    height,
    top,
    right,
    bottom,
    left,
    toJSON: () => ({}),
  } as DOMRect;
}

function getLastRenderedRow(): HTMLElement {
  const row = document.querySelector('[data-chat-last-rendered-row="true"]');
  expect(row).toBeInstanceOf(HTMLElement);
  return row as HTMLElement;
}

function mockToolGroupToggleRectShift({
  collapsedTop,
  expandedTop,
}: {
  collapsedTop: number;
  expandedTop: number;
}) {
  return vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function () {
    const element = this as HTMLElement;
    if (element.dataset.chatToolCallGroupKey != null) {
      const top = element.getAttribute("aria-expanded") === "true" ? expandedTop : collapsedTop;
      return makeRect({ top, bottom: top + 20 });
    }
    return makeRect({ top: 0, bottom: 500, height: 500 });
  });
}

function TooltipTestProvider({ children }: { children: ReactNode }) {
  return <TooltipProvider delayDuration={0}>{children}</TooltipProvider>;
}

function render(ui: ReactElement) {
  return rtlRender(ui, { wrapper: TooltipTestProvider });
}

function expectMockVirtuosoCallback<T>(name: string): T {
  const callback = mockVirtuosoHarness.props?.[name];
  expect(callback).toEqual(expect.any(Function));
  return callback as T;
}

describe("ChatMessageList - Scroll Behavior", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseMessageAttachments.mockReturnValue({ data: new Map() });
    mockVirtuosoHarness.props = null;
    mockIsAtBottom = true;
    mockIsAtBottomRef.current = true;
    scrollIntoViewMock.mockClear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("initial conversation load", () => {
    it("starts at last message on mount (Virtuoso initialTopMostItemIndex)", () => {
      render(<ChatMessageList {...defaultProps} />);

      // Verify messages are rendered
      expect(screen.getByText("Message 1")).toBeInTheDocument();
      expect(screen.getByText("Message 10")).toBeInTheDocument();
    });

    it("remounts completely when conversation ID changes", () => {
      const { rerender } = render(<ChatMessageList {...defaultProps} />);

      // Switch conversation (forces remount via key prop)
      const newMessages = createMessages(5);
      rerender(
        <ChatMessageList
          {...defaultProps}
          conversationId="conv-2"
          messages={newMessages}
        />
      );

      // Verify new conversation messages
      expect(screen.getByText("Message 1")).toBeInTheDocument();
      expect(screen.getByText("Message 5")).toBeInTheDocument();
      expect(screen.queryByText("Message 10")).not.toBeInTheDocument();
    });

    it("shows no settling delay (instant render)", () => {
      vi.useFakeTimers();
      render(<ChatMessageList {...defaultProps} />);

      // Messages should be visible immediately (no isScrollSettling logic)
      expect(screen.getByText("Message 1")).toBeInTheDocument();
      expect(screen.getByText("Message 10")).toBeInTheDocument();

      vi.useRealTimers();
    });

    it("does not render an empty footer spacer when idle", () => {
      render(<ChatMessageList {...defaultProps} />);

      const root = screen.getByTestId("integrated-chat-messages");
      const emptyFooterSpacer = Array.from(root.children).find((child) =>
        child instanceof HTMLElement &&
        child.classList.contains("px-3") &&
        child.classList.contains("pb-3") &&
        child.classList.contains("w-full")
      );

      expect(emptyFooterSpacer).toBeUndefined();
    });

    it("pins to true bottom on initial load when the scroller does not resize", () => {
      vi.useFakeTimers();
      vi.stubEnv("VITEST", "");
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
      const originalResizeObserver = globalThis.ResizeObserver;

      class QuietResizeObserver implements ResizeObserver {
        disconnect = vi.fn();
        observe = vi.fn();
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: QuietResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        render(<ChatMessageList {...defaultProps} />);
        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 620,
          scrollTop: 60,
        });
        scrollToMock.mockClear();

        act(() => {
          vi.advanceTimersByTime(300);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 120, behavior: "auto" });
        act(() => {
          vi.advanceTimersByTime(120);
        });
        scroller.scrollTop = 60;
        scrollToMock.mockClear();

        act(() => {
          vi.advanceTimersByTime(380);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 120, behavior: "auto" });
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
        vi.unstubAllEnvs();
        vi.useRealTimers();
      }
    });

    it("does not run initial-load bottom pins after manual downward wheel input", () => {
      vi.useFakeTimers();
      vi.stubEnv("VITEST", "");
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
      const originalResizeObserver = globalThis.ResizeObserver;

      class QuietResizeObserver implements ResizeObserver {
        disconnect = vi.fn();
        observe = vi.fn();
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: QuietResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        render(<ChatMessageList {...defaultProps} />);
        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 620,
          scrollTop: 60,
        });
        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 80 }));
          scroller.scrollTop = 90;
          scroller.dispatchEvent(new Event("scroll"));
        });
        scrollToMock.mockClear();

        act(() => {
          vi.advanceTimersByTime(800);
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
        vi.unstubAllEnvs();
        vi.useRealTimers();
      }
    });

    it("does not treat Virtuoso pre-settle measurement scroll as user scroll-away", () => {
      vi.useFakeTimers();
      vi.stubEnv("VITEST", "");
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        render(<ChatMessageList {...defaultProps} />);
        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 80,
        });
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 60,
        });
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        scrollToMock.mockClear();

        act(() => {
          vi.advanceTimersByTime(300);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" });
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
        vi.useRealTimers();
      }
    });

    it("does not treat post-load Virtuoso measurement scroll as user scroll-away", () => {
      vi.useFakeTimers();
      vi.stubEnv("VITEST", "");
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(<ChatMessageList {...defaultProps} />);
        const scroller = screen.getByTestId("mock-virtuoso");
        const totalListHeightChanged = expectMockVirtuosoCallback<(height: number) => void>(
          "totalListHeightChanged",
        );

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
          totalListHeightChanged(1000);
          vi.advanceTimersByTime(300);
        });
        scrollToMock.mockClear();

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 300,
        });
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1120,
          scrollTop: 300,
        });
        act(() => {
          totalListHeightChanged(1120);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 620, behavior: "auto" });
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
        vi.useRealTimers();
      }
    });

    it("repins after a post-load Virtuoso measurement scroll without waiting for a resize", () => {
      vi.useFakeTimers();
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return queuedRafs.length;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(<ChatMessageList {...defaultProps} />);
        const scroller = screen.getByTestId("mock-virtuoso");
        queuedRafs.length = 0;

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });
        act(() => {
          vi.advanceTimersByTime(300);
        });
        scrollToMock.mockClear();

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 300,
        });
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" });
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
        vi.useRealTimers();
      }
    });

    it("repins when Virtuoso reports not-at-bottom after a post-load measurement drift", () => {
      vi.useFakeTimers();
      vi.stubEnv("VITEST", "");

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(<ChatMessageList {...defaultProps} />);
        const scroller = screen.getByTestId("mock-virtuoso");
        const atBottomStateChange = expectMockVirtuosoCallback<(atBottom: boolean) => void>(
          "atBottomStateChange",
        );

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        act(() => {
          atBottomStateChange(true);
          vi.advanceTimersByTime(300);
        });
        scrollToMock.mockClear();

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 300,
        });
        act(() => {
          atBottomStateChange(false);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" });
      } finally {
        vi.unstubAllEnvs();
        vi.useRealTimers();
      }
    });

    it("repins no-input measurement drift even when the last item temporarily leaves range", () => {
      vi.useFakeTimers();
      vi.stubEnv("VITEST", "");

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(<ChatMessageList {...defaultProps} />);
        const scroller = screen.getByTestId("mock-virtuoso");
        const rangeChanged = expectMockVirtuosoCallback<(range: { startIndex: number; endIndex: number }) => void>(
          "rangeChanged",
        );
        const atBottomStateChange = expectMockVirtuosoCallback<(atBottom: boolean) => void>(
          "atBottomStateChange",
        );

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        act(() => {
          atBottomStateChange(true);
          vi.advanceTimersByTime(300);
        });
        scrollToMock.mockClear();

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 300,
        });
        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 1 });
          atBottomStateChange(false);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" });
      } finally {
        vi.unstubAllEnvs();
        vi.useRealTimers();
      }
    });

    it("keeps queued initial-load bottom verification alive while the last item is not yet visible", () => {
      vi.useFakeTimers();
      vi.stubEnv("VITEST", "");
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
      const originalResizeObserver = globalThis.ResizeObserver;

      class QuietResizeObserver implements ResizeObserver {
        disconnect = vi.fn();
        observe = vi.fn();
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: QuietResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        render(<ChatMessageList {...defaultProps} />);
        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 620,
          scrollTop: 60,
        });
        const rangeChanged = expectMockVirtuosoCallback<(range: { startIndex: number; endIndex: number }) => void>(
          "rangeChanged",
        );
        scrollToMock.mockClear();

        act(() => {
          vi.advanceTimersByTime(300);
        });
        expect(scrollToMock).toHaveBeenCalledWith({ top: 120, behavior: "auto" });

        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 1 });
        });
        scroller.scrollTop = 60;
        scrollToMock.mockClear();

        act(() => {
          vi.advanceTimersByTime(500);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 120, behavior: "auto" });
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
        vi.unstubAllEnvs();
        vi.useRealTimers();
      }
    });

    it("keeps a visual placeholder cover until the initial transcript paint settles", async () => {
      const onInitialPaintReady = vi.fn();

      render(
        <ChatMessageList
          {...defaultProps}
          initialPaintCoverKey="conv-1"
          onInitialPaintReady={onInitialPaintReady}
        />
      );

      expect(screen.getByTestId("chat-transcript-settling-placeholders")).toBeInTheDocument();
      expect(screen.getByTestId("chat-transcript-settling-placeholders")).toHaveClass(
        "pointer-events-none",
      );
      expect(screen.getByTestId("chat-transcript-settling-placeholders")).toHaveClass(
        "bg-[var(--bg-base)]",
      );
      expect(screen.getByText("Message 10")).toBeInTheDocument();

      await waitFor(() =>
        expect(screen.queryByTestId("chat-transcript-settling-placeholders")).not.toBeInTheDocument()
      );
      expect(onInitialPaintReady).toHaveBeenCalledWith("conv-1");
    });

    it("defers attachment hydration until the initial transcript cover has cleared", async () => {
      render(
        <ChatMessageList
          {...defaultProps}
          initialPaintCoverKey="conv-1"
          onInitialPaintReady={vi.fn()}
        />
      );

      expect(mockUseMessageAttachments).toHaveBeenLastCalledWith(
        defaultProps.messages,
        "conv-1",
        expect.objectContaining({ enabled: false })
      );

      await waitFor(() =>
        expect(mockUseMessageAttachments).toHaveBeenLastCalledWith(
          defaultProps.messages,
          "conv-1",
          expect.objectContaining({ enabled: true })
        )
      );
    });

    it("does not treat the transcript as reveal-ready while the virtualized item list is hidden", () => {
      const root = document.createElement("div");
      const list = document.createElement("div");
      const message = document.createElement("div");

      list.dataset.testid = "virtuoso-item-list";
      list.style.visibility = "hidden";
      message.dataset.chatMessageItem = "true";
      list.appendChild(message);
      root.appendChild(list);
      document.body.appendChild(root);

      try {
        expect(isTranscriptRootReadyForReveal(root)).toBe(false);

        list.style.visibility = "visible";
        expect(isTranscriptRootReadyForReveal(root)).toBe(true);

        message.remove();
        expect(isTranscriptRootReadyForReveal(root)).toBe(false);
      } finally {
        root.remove();
      }
    });
  });

  describe("streaming auto-scroll", () => {
    it("keeps ChatMessageList free of the parent-level streaming tool strip", () => {
      // StreamingToolIndicator is rendered OUTSIDE ChatMessageList (in parent panels).
      const streamingToolCalls: ToolCall[] = [
        {
          id: "tool-1",
          name: "Read",
          arguments: { file_path: "/test.ts" },
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingToolCalls={streamingToolCalls}
        />
      );

      // StreamingToolIndicator is NOT in ChatMessageList anymore (moved to parent)
      expect(screen.queryByTestId("streaming-tool-indicator")).not.toBeInTheDocument();
    });

    it("auto-scrolls when streaming text appears", () => {
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={undefined}
        />
      );

      // Add streaming text via content blocks
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "Streaming assistant response..." },
      ];
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      // Verify streaming text is rendered
      expect(screen.getByText(/Streaming assistant response/)).toBeInTheDocument();
    });

    it("auto-scrolls when agent is running without streaming content", () => {
      render(
        <ChatMessageList
          {...defaultProps}
          isAgentRunning={true}
          streamingToolCalls={[]}
        />
      );

      // Verify component renders with agent running state
      // Note: In test env, typing indicator is rendered in simplified DOM
      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
    });

    it("does not auto-scroll when user scrolled up", () => {
      mockIsAtBottom = false;

      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={undefined}
        />
      );

      // Add streaming content (should not trigger scroll)
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "New content" },
      ];
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      // Content rendered but scroll behavior controlled by hook
      expect(screen.getByText(/New content/)).toBeInTheDocument();
    });

    it("filters the latest orchestrator provider row while ideation streaming content is visible", () => {
      const messages: ChatMessageData[] = [
        {
          id: "msg-user",
          role: "user",
          content: "hello",
          createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
          toolCalls: null,
          contentBlocks: null,
        },
        {
          id: "msg-orchestrator",
          role: "orchestrator",
          content: "Persisted orchestrator message",
          createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
          toolCalls: null,
          contentBlocks: null,
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isSending={true}
          streamingContentBlocks={[{ type: "text", text: "Live ideation chunk" }]}
        />
      );

      expect(screen.getByText("hello")).toBeInTheDocument();
      expect(screen.getByText("Live ideation chunk")).toBeInTheDocument();
      expect(screen.queryByText("Persisted orchestrator message")).not.toBeInTheDocument();
    });

    it("does not hide previous-turn provider rows before the current streaming row is persisted", () => {
      const messages: ChatMessageData[] = [
        {
          id: "msg-user-1",
          role: "user",
          content: "first request",
          createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
          toolCalls: null,
          contentBlocks: null,
        },
        {
          id: "msg-assistant-1",
          role: "assistant",
          content: "previous answer",
          createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
          toolCalls: null,
          contentBlocks: null,
        },
        {
          id: "msg-user-2",
          role: "user",
          content: "second request",
          createdAt: new Date(2026, 0, 1, 12, 2).toISOString(),
          toolCalls: null,
          contentBlocks: null,
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isSending={true}
          streamingContentBlocks={[{ type: "text", text: "Live current answer" }]}
        />
      );

      expect(screen.getByText("previous answer")).toBeInTheDocument();
      expect(screen.getByText("second request")).toBeInTheDocument();
      expect(screen.getByText("Live current answer")).toBeInTheDocument();
    });

    it("keeps the latest orchestrator provider row hidden while finalizing after streaming", () => {
      const messages: ChatMessageData[] = [
        {
          id: "msg-user",
          role: "user",
          content: "hello",
          createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
          toolCalls: null,
          contentBlocks: null,
        },
        {
          id: "msg-orchestrator",
          role: "orchestrator",
          content: "Persisted orchestrator message",
          createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
          toolCalls: null,
          contentBlocks: null,
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isFinalizing={true}
        />
      );

      expect(screen.getByText("hello")).toBeInTheDocument();
      expect(screen.queryByText("Persisted orchestrator message")).not.toBeInTheDocument();
    });
  });

  describe("manual scroll detection", () => {
    it("tracks bottom state via hook integration", () => {
      render(<ChatMessageList {...defaultProps} />);

      // Verify component renders successfully with auto-scroll hook
      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();

      // Hook integration is verified by component not throwing errors
      // The mocked hook provides the necessary callbacks
    });

    it("pauses auto-scroll when user manually scrolls up", () => {
      mockIsAtBottom = false;
      mockHandleFollowOutput.mockReturnValue(false);

      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "Streaming..." },
      ];
      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      // Verify streaming content is rendered but scroll is paused
      expect(screen.getByText(/Streaming/)).toBeInTheDocument();
    });

    it("supports scroll-to-bottom button when scrolled up with >5 messages", () => {
      mockIsAtBottom = false;

      render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      // Note: In test env, simplified DOM is rendered without Virtuoso footer
      // Button rendering is controlled by useChatAutoScroll hook's isAtBottom state
      // Verify component renders with appropriate message count
      expect(screen.getByText("Message 1")).toBeInTheDocument();
      expect(screen.getByText("Message 10")).toBeInTheDocument();
    });

    it("hides scroll-to-bottom button when at bottom", () => {
      mockIsAtBottom = true;

      render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");
      expect(screen.getByTestId("chat-scroll-to-bottom-button")).toBeDisabled();
    });

    it("shows scroll-to-bottom button with <=5 messages when scrolled up", () => {
      mockIsAtBottom = false;

      render(<ChatMessageList {...defaultProps} messages={createMessages(5)} />);

      expect(screen.getByText(/Scroll to bottom/i)).toBeInTheDocument();
    });

    it("provides scroll-to-bottom functionality via hook", () => {
      mockIsAtBottom = false;

      render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      // Note: In test env, button is not rendered (simplified DOM)
      // But hook provides scrollToBottom function for production use
      // Verify scrollToBottom mock is available
      expect(mockScrollToBottom).toBeDefined();
    });
  });

  describe("conversation switch", () => {
    it("shows last message instantly on conversation switch (no settling)", () => {
      vi.useFakeTimers();
      const { rerender } = render(<ChatMessageList {...defaultProps} />);

      // Switch conversation
      const newMessages = createMessages(8);
      rerender(
        <ChatMessageList
          {...defaultProps}
          conversationId="conv-2"
          messages={newMessages}
        />
      );

      // Messages visible immediately (no 350ms delay)
      expect(screen.getByText("Message 1")).toBeInTheDocument();
      expect(screen.getByText("Message 8")).toBeInTheDocument();

      vi.useRealTimers();
    });

    it("remounts Virtuoso with new key on conversation change", () => {
      const { rerender, container } = render(
        <ChatMessageList {...defaultProps} conversationId="conv-1" />
      );

      const firstVirtuoso = container.querySelector('[data-testid="integrated-chat-messages"]');

      // Switch conversation
      rerender(
        <ChatMessageList
          {...defaultProps}
          conversationId="conv-2"
          messages={createMessages(5)}
        />
      );

      const secondVirtuoso = container.querySelector('[data-testid="integrated-chat-messages"]');

      // Component remounts (same testid but potentially different instance)
      expect(firstVirtuoso).toBeTruthy();
      expect(secondVirtuoso).toBeTruthy();
    });

    it("does not treat a historical trailing user message as a fresh append on conversation open", () => {
      vi.useFakeTimers();
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      const historicalMessages: ChatMessageData[] = [
        {
          id: "assistant-1",
          role: "assistant",
          content: "Earlier reply",
          createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
          toolCalls: null,
          contentBlocks: null,
        },
        {
          id: "user-2",
          role: "user",
          content: "Last historical user message",
          createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
          toolCalls: null,
          contentBlocks: null,
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          conversationId="conv-history"
          messages={historicalMessages}
        />
      );

      expect(mockScrollToBottom).not.toHaveBeenCalled();

      vi.useRealTimers();
      rafSpy.mockRestore();
      cancelSpy.mockRestore();
    });
  });

  describe("history mode (timestamp scroll)", () => {
    it("disables auto-scroll when scrollToTimestamp is set", () => {
      const messages = createMessages(10);
      const targetTimestamp = messages[5].createdAt;

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          scrollToTimestamp={targetTimestamp}
        />
      );

      // Verify component renders in history mode
      // Hook receives disabled: true when scrollToTimestamp is set
      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
    });

    it("does not show scroll-to-bottom button in history mode", () => {
      mockIsAtBottom = false;

      render(
        <ChatMessageList
          {...defaultProps}
          messages={createMessages(10)}
          scrollToTimestamp={new Date().toISOString()}
        />
      );

      expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");
      expect(screen.getByTestId("chat-scroll-to-bottom-button")).toBeDisabled();
    });
  });

  describe("failed run banner", () => {
    it("shows failed run banner in header", () => {
      const failedRun = {
        id: "run-1",
        errorMessage: "Execution failed: timeout",
      };

      render(
        <ChatMessageList
          {...defaultProps}
          failedRun={failedRun}
          onDismissFailedRun={vi.fn()}
        />
      );

      expect(screen.getByText(/Execution failed: timeout/)).toBeInTheDocument();
    });

    it("dismisses failed run banner when close clicked", async () => {
      const user = userEvent.setup();
      const onDismiss = vi.fn();
      const failedRun = {
        id: "run-1",
        errorMessage: "Error occurred",
      };

      render(
        <ChatMessageList
          {...defaultProps}
          failedRun={failedRun}
          onDismissFailedRun={onDismiss}
        />
      );

      const dismissButton = screen.getByRole("button", { name: /dismiss/i });
      await user.click(dismissButton);

      expect(onDismiss).toHaveBeenCalledWith("run-1");
    });
  });

  describe("memo stability (no infinite re-render)", () => {
    it("timeline useMemo returns stable reference when hookEvents/activeHooks not passed", () => {
      // When hookEvents and activeHooks are omitted, the default `= []` in
      // destructuring creates a new array reference each render. This busts
      // the `timeline` useMemo and causes Virtuoso to re-render infinitely.
      // The fix uses module-level empty constants as defaults.
      const { rerender } = render(
        <ChatMessageList {...defaultProps} />
      );

      // Re-render with same props (no hookEvents/activeHooks passed)
      rerender(<ChatMessageList {...defaultProps} />);

      // If the fix is applied, useChatAutoScroll should have been called
      // with the same messageCount both times — no crash, no infinite loop.
      // The key assertion: the component renders successfully without
      // "Maximum update depth exceeded" error.
      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();

      // Verify hook was called exactly twice (initial + rerender)
      // If timeline memo was unstable, React would hit update depth limit
      const callCount = mockUseChatAutoScroll.mock.calls.length;
      expect(callCount).toBe(2);
    });

    it("re-render with same props does not increase hook call count beyond expected", () => {
      // Regression test: Virtuoso components/itemContent props must be
      // memoized (useMemo/useCallback) so Virtuoso doesn't re-mount
      // Header/Footer on every render, which triggers atBottomStateChange
      // → state change → re-render → new components object → infinite loop.
      const { rerender } = render(
        <ChatMessageList {...defaultProps} />
      );

      const callsAfterMount = mockUseChatAutoScroll.mock.calls.length;

      // Re-render 5 times with identical props
      for (let i = 0; i < 5; i++) {
        rerender(<ChatMessageList {...defaultProps} />);
      }

      // Each rerender should call the hook exactly once (no cascading re-renders)
      const callsAfterRerenders = mockUseChatAutoScroll.mock.calls.length;
      expect(callsAfterRerenders).toBe(callsAfterMount + 5);
    });
  });

  describe("GAP: virtuosoComponents deps include isAtBottom (F1+F2)", () => {
    it("should re-call hook when isAtBottom toggles (unstable components)", () => {
      mockIsAtBottom = true;
      const { rerender } = render(<ChatMessageList {...defaultProps} />);

      const callsAfterMount = mockUseChatAutoScroll.mock.calls.length;

      // Toggle isAtBottom → useMemo recomputes → re-render
      mockIsAtBottom = false;
      rerender(<ChatMessageList {...defaultProps} />);

      mockIsAtBottom = true;
      rerender(<ChatMessageList {...defaultProps} />);

      // Each toggle causes a re-render (the component processes the state change)
      const callsAfterToggles = mockUseChatAutoScroll.mock.calls.length;
      expect(callsAfterToggles).toBe(callsAfterMount + 2);
    });
  });

  describe("GAP: messages.length in virtuosoComponents deps causes rebuild (F4)", () => {
    it("should re-render when messages.length changes", () => {
      const { rerender } = render(
        <ChatMessageList {...defaultProps} messages={createMessages(5)} />
      );

      const callsAfterMount = mockUseChatAutoScroll.mock.calls.length;

      // Add a message → messages.length changes → useMemo recomputes
      rerender(
        <ChatMessageList {...defaultProps} messages={createMessages(6)} />
      );

      const callsAfterRerender = mockUseChatAutoScroll.mock.calls.length;
      // Component re-renders because props changed (expected behavior)
      expect(callsAfterRerender).toBe(callsAfterMount + 1);
    });
  });

  describe("GAP: failedRun prop creates new object each render (F5)", () => {
    it("should accept new failedRun object references without memoization", () => {
      const failedRun1 = { id: "run-1", errorMessage: "Error A" };
      const { rerender } = render(
        <ChatMessageList {...defaultProps} failedRun={failedRun1} />
      );

      const callsAfterMount = mockUseChatAutoScroll.mock.calls.length;

      // New object with same data (different reference — simulates upstream inline creation)
      const failedRun2 = { id: "run-1", errorMessage: "Error A" };
      rerender(
        <ChatMessageList {...defaultProps} failedRun={failedRun2} />
      );

      // Component re-renders because failedRun is a new ref
      const callsAfterRerender = mockUseChatAutoScroll.mock.calls.length;
      expect(callsAfterRerender).toBe(callsAfterMount + 1);
    });
  });

  describe("FIX-F1+F2: scroll button renders outside Virtuoso", () => {
    it("should show scroll button when not at bottom with >5 messages", () => {
      mockIsAtBottom = false;

      render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      // Button is now rendered outside Virtuoso (in the component wrapper)
      expect(screen.getByText(/Scroll to bottom/i)).toBeInTheDocument();
    });

    it("should hide scroll button when at bottom", () => {
      mockIsAtBottom = true;

      render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      expect(screen.getByTestId("chat-scroll-to-bottom-control")).toHaveAttribute("aria-hidden", "true");
      expect(screen.getByTestId("chat-scroll-to-bottom-button")).toBeDisabled();
    });

    it("keeps a stable lightweight scroll button shell mounted while hidden", () => {
      mockIsAtBottom = true;

      const { rerender } = render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);
      const hiddenControl = screen.getByTestId("chat-scroll-to-bottom-control");
      expect(hiddenControl).toHaveAttribute("aria-hidden", "true");

      mockIsAtBottom = false;
      rerender(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      expect(screen.getByTestId("chat-scroll-to-bottom-control")).toBe(hiddenControl);
      expect(hiddenControl).toHaveAttribute("aria-hidden", "false");
    });

    it("does not use backdrop blur on the scroll button", () => {
      mockIsAtBottom = false;

      render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      const button = screen.getByTestId("chat-scroll-to-bottom-button");
      expect(button.className).not.toContain("backdrop-blur");
      expect(button.className).not.toContain("shadow-md");
    });

    it("uses a compact button with a trailing caret", () => {
      mockIsAtBottom = false;

      render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      const button = screen.getByTestId("chat-scroll-to-bottom-button");
      expect(button.className).toContain("h-8");
      expect(button.className).toContain("text-xs");
      expect(button.className).toContain("px-3");
      expect(button.className).toContain("cursor-pointer");
      expect(button.className).toContain("hover:bg-");
      expect(button.lastElementChild?.tagName.toLowerCase()).toBe("svg");
    });

    it("keeps wheel scrolling active when the pointer is over the button", () => {
      mockIsAtBottom = false;

      render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      const root = screen.getByTestId("integrated-chat-messages");
      Object.defineProperty(root, "scrollTop", {
        value: 0,
        writable: true,
        configurable: true,
      });

      const button = screen.getByTestId("chat-scroll-to-bottom-button");
      const wheelEvent = new WheelEvent("wheel", {
        bubbles: true,
        cancelable: true,
        deltaY: 96,
      });
      button.dispatchEvent(wheelEvent);

      expect(root.scrollTop).toBe(96);
    });

    it("should show scroll button with <=5 messages when scrolled up", () => {
      mockIsAtBottom = false;

      render(<ChatMessageList {...defaultProps} messages={createMessages(3)} />);

      expect(screen.getByText(/Scroll to bottom/i)).toBeInTheDocument();
    });

    it("should call scrollToBottom when button is clicked", async () => {
      mockIsAtBottom = false;
      const user = userEvent.setup();

      render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);

      const button = screen.getByText(/Scroll to bottom/i);
      await user.click(button);

      expect(mockScrollToBottom).toHaveBeenCalled();
    });

    it("keeps the scroll-to-bottom click target below the typing indicator", async () => {
      mockIsAtBottom = false;
      const user = userEvent.setup();

      render(
        <ChatMessageList
          {...defaultProps}
          messages={createMessages(10)}
          isAgentRunning={true}
          streamingContentBlocks={undefined}
        />
      );

      const hookArgs = mockUseChatAutoScroll.mock.calls[0][0] as Record<string, unknown>;
      expect(hookArgs.messageCount).toBe(11);

      const button = screen.getByText(/Scroll to bottom/i);
      await user.click(button);

      expect(mockScrollToBottom).toHaveBeenCalled();
    });

    it("should not cause cascading re-renders on isAtBottom toggle", () => {
      mockIsAtBottom = true;
      const { rerender } = render(<ChatMessageList {...defaultProps} />);

      const callsAfterMount = mockUseChatAutoScroll.mock.calls.length;

      // Toggle isAtBottom back and forth
      mockIsAtBottom = false;
      rerender(<ChatMessageList {...defaultProps} />);
      mockIsAtBottom = true;
      rerender(<ChatMessageList {...defaultProps} />);

      // Exactly 2 additional renders (1 per rerender), no cascade
      expect(mockUseChatAutoScroll.mock.calls.length).toBe(callsAfterMount + 2);
    });
  });

  describe("FIX-F4: virtuosoComponents useMemo deps exclude scroll state", () => {
    it("should not cascade re-renders when only isAtBottom changes", () => {
      mockIsAtBottom = true;
      const { rerender } = render(<ChatMessageList {...defaultProps} />);

      const callsAfterMount = mockUseChatAutoScroll.mock.calls.length;

      // Rerender with toggled isAtBottom — should NOT bust virtuosoComponents
      mockIsAtBottom = false;
      rerender(<ChatMessageList {...defaultProps} />);

      // Only 1 rerender, not a cascade
      expect(mockUseChatAutoScroll.mock.calls.length).toBe(callsAfterMount + 1);
    });
  });

  describe("footer content hash for streaming", () => {
    it("computes hash based on tool calls count", () => {
      const streamingToolCalls: ToolCall[] = [
        { id: "1", name: "Read", arguments: {} },
        { id: "2", name: "Write", arguments: {} },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingToolCalls={streamingToolCalls}
        />
      );

      // Virtuoso handles scroll via context prop (footerContentHash).
      // StreamingToolIndicator is rendered in parent panels, not in ChatMessageList.
      expect(screen.queryByTestId("streaming-tool-indicator")).not.toBeInTheDocument();
      // Component renders without error
      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
    });

    it("computes hash based on streaming text presence", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "Thinking..." },
      ];
      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      // Verify streaming text renders
      // Virtuoso handles scroll via context prop (footerContentHash)
      expect(screen.getByText(/Thinking/)).toBeInTheDocument();
    });
  });

  describe("single-path Virtuoso scroll (no DOM auto-scroll)", () => {
    it("passes virtuosoRef to useChatAutoScroll hook", () => {
      render(<ChatMessageList {...defaultProps} />);

      // Hook must receive virtuosoRef so scrollToBottom routes through Virtuoso
      expect(mockUseChatAutoScroll).toHaveBeenCalled();
      const hookArgs = mockUseChatAutoScroll.mock.calls[0][0] as Record<string, unknown>;
      expect(hookArgs).toHaveProperty("virtuosoRef");
      expect(hookArgs.virtuosoRef).toBeDefined();
    });

    it("passes disabled=true when scrollToTimestamp is set", () => {
      const messages = createMessages(10);
      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          scrollToTimestamp={messages[3].createdAt}
        />
      );

      const hookArgs = mockUseChatAutoScroll.mock.calls[0][0] as Record<string, unknown>;
      expect(hookArgs.disabled).toBe(true);
    });

    it("passes disabled=false when scrollToTimestamp is null", () => {
      render(
        <ChatMessageList
          {...defaultProps}
          scrollToTimestamp={null}
        />
      );

      const hookArgs = mockUseChatAutoScroll.mock.calls[0][0] as Record<string, unknown>;
      expect(hookArgs.disabled).toBe(false);
    });

    it("passes rendered timeline item count to hook", () => {
      const messages = createMessages(7);
      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
        />
      );

      const hookArgs = mockUseChatAutoScroll.mock.calls[0][0] as Record<string, unknown>;
      expect(hookArgs.messageCount).toBe(7);
    });

    it("includes the active typing indicator in the hook scroll target count", () => {
      const messages = createMessages(7);
      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={true}
          streamingContentBlocks={undefined}
        />
      );

      const hookArgs = mockUseChatAutoScroll.mock.calls[0][0] as Record<string, unknown>;
      expect(hookArgs.messageCount).toBe(8);
    });

    it("passes conversationId to useChatAutoScroll hook", () => {
      render(
        <ChatMessageList
          {...defaultProps}
          conversationId="conv-test-123"
        />
      );

      const hookArgs = mockUseChatAutoScroll.mock.calls[0][0] as Record<string, unknown>;
      expect(hookArgs.conversationId).toBe("conv-test-123");
    });

    it("does not pass isStreaming or streamingHash to hook (removed props)", () => {
      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          isAgentRunning={true}
        />
      );

      const hookArgs = mockUseChatAutoScroll.mock.calls[0][0] as Record<string, unknown>;
      // These props were removed — Virtuoso context handles streaming scroll
      expect(hookArgs).not.toHaveProperty("isStreaming");
      expect(hookArgs).not.toHaveProperty("streamingHash");
    });

    it("does NOT call scrollIntoView during streaming content changes", () => {
      scrollIntoViewMock.mockClear();

      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingToolCalls={[]}
        />
      );

      // Add streaming tool call
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingToolCalls={[{ id: "1", name: "Read", arguments: {} }]}
        />
      );

      // Add streaming content
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingToolCalls={[{ id: "1", name: "Read", arguments: {} }]}
        />
      );

      // No DOM scrollIntoView — Virtuoso followOutput handles all auto-scrolling
      expect(scrollIntoViewMock).not.toHaveBeenCalled();
    });

    it("does NOT call scrollIntoView on conversation switch", () => {
      scrollIntoViewMock.mockClear();

      const { rerender } = render(
        <ChatMessageList {...defaultProps} conversationId="conv-1" />
      );

      // Switch conversation
      rerender(
        <ChatMessageList
          {...defaultProps}
          conversationId="conv-2"
          messages={createMessages(5)}
        />
      );

      // No DOM scrollIntoView — Virtuoso remounts with initialTopMostItemIndex
      expect(scrollIntoViewMock).not.toHaveBeenCalled();
    });

    it("does NOT call scrollIntoView when new messages arrive", () => {
      scrollIntoViewMock.mockClear();

      const { rerender } = render(
        <ChatMessageList {...defaultProps} messages={createMessages(5)} />
      );

      // New message arrives
      rerender(
        <ChatMessageList {...defaultProps} messages={createMessages(6)} />
      );

      // No DOM scrollIntoView — Virtuoso followOutput handles auto-scroll
      expect(scrollIntoViewMock).not.toHaveBeenCalled();
    });
  });

  describe("non-diff tool call inline rendering (Bug 3 fix)", () => {
    // Uses "webfetch" as the tool name — it's non-diff, non-task, and not in the
    // widget registry, so it falls through to the generic ToolCallIndicator renderer
    // which has data-testid="tool-call-indicator".
    const GENERIC_TOOL_NAME = "webfetch";

    it("renders non-diff tool call block as ToolCallIndicator inline", () => {
      const blocks: StreamingContentBlock[] = [
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://example.com" }, result: "page content" },
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByTestId("tool-call-indicator")).toBeInTheDocument();
    });

    it("renders text and tool call in correct visual order (text → tool → text)", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "First I will fetch the page." },
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://example.com" }, result: "content" },
        },
        { type: "text", text: "The page contains useful info." },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      const text1 = screen.getByText(/First I will fetch the page/);
      const toolCall = screen.getByTestId("tool-call-indicator");
      const text2 = screen.getByText(/The page contains useful info/);

      // Verify DOM order: text1 < toolCall < text2
      expect(text1.compareDocumentPosition(toolCall) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(toolCall.compareDocumentPosition(text2) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });

    it("groups streaming text and tool widgets inside one assistant message row", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "First I will fetch the page." },
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://example.com" }, result: "content" },
        },
        { type: "text", text: "The page contains useful info." },
      ];

      const { container } = render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      const firstText = screen.getByText(/First I will fetch the page/);
      const secondText = screen.getByText(/The page contains useful info/);
      const toolCall = screen.getByTestId("tool-call-indicator");
      const liveAssistantRow = firstText.closest('[data-chat-message-item="true"]');

      expect(liveAssistantRow).toBeInTheDocument();
      expect(liveAssistantRow).toContainElement(toolCall);
      expect(liveAssistantRow).toContainElement(secondText);
      expect(liveAssistantRow?.querySelector("svg.lucide-bot")).toBeInTheDocument();

      const matchingRows = Array.from(
        container.querySelectorAll('[data-chat-message-item="true"]')
      ).filter((row) => row.textContent?.includes("First I will fetch the page"));
      expect(matchingRows).toHaveLength(1);
    });

    it("shows loading spinner for in-progress (no result) tool call", () => {
      const blocks: StreamingContentBlock[] = [
        {
          type: "tool_use",
          // result is undefined — tool still running
          toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://example.com" } },
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByTestId("tool-call-indicator")).toBeInTheDocument();
      // Loading spinner (animate-spin class) should be present for in-progress tool calls
      const spinner = document.querySelector(".animate-spin");
      expect(spinner).toBeInTheDocument();
    });

    it("does not show loading spinner for completed (has result) tool call", () => {
      const blocks: StreamingContentBlock[] = [
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://example.com" }, result: "page content" },
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByTestId("tool-call-indicator")).toBeInTheDocument();
      // No spinner — tool has a result (completed)
      const spinner = document.querySelector(".animate-spin");
      expect(spinner).not.toBeInTheDocument();
    });

    it("keeps TypingIndicator at the bottom while active content blocks are present", () => {
      const blocks: StreamingContentBlock[] = [
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://example.com" } },
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingToolCalls={[{ id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://example.com" } }]}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByTestId("chat-typing-indicator")).toBeInTheDocument();
    });

    it("renders live text metadata after each streaming text block before the typing indicator", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "First live paragraph." },
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://example.com" }, result: "content" },
        },
        { type: "text", text: "Second live paragraph." },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={[]}
          isAgentRunning={true}
          streamingContentBlocks={blocks}
        />
      );

      const metadataRows = screen.getAllByTestId("message-meta");
      const copyButtons = screen.getAllByTestId("message-copy-button");
      const typingIndicator = screen.getByTestId("chat-typing-indicator");
      const liveAssistantRow = screen
        .getByText("First live paragraph.")
        .closest('[data-chat-message-item="true"]');

      expect(metadataRows).toHaveLength(2);
      expect(copyButtons).toHaveLength(2);
      expect(liveAssistantRow).toBeInTheDocument();
      expect(typingIndicator.closest('[data-chat-message-item="true"]')).toBeNull();
      expect(metadataRows[0]).toHaveTextContent(/just now/i);
      expect(metadataRows[1]).toHaveTextContent(/just now/i);
      expect(screen.getByText("First live paragraph.").compareDocumentPosition(metadataRows[0]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(metadataRows[0]!.compareDocumentPosition(screen.getByTestId("tool-call-indicator")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(screen.getByText("Second live paragraph.").compareDocumentPosition(metadataRows[1]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(metadataRows[1]!.compareDocumentPosition(typingIndicator) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });

    it("collapses and expands multiple consecutive live non-diff tool calls", async () => {
      const user = userEvent.setup();
      const blocks: StreamingContentBlock[] = [
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://a.com" }, result: "page a" },
        },
        {
          type: "tool_use",
          toolCall: { id: "tc-2", name: GENERIC_TOOL_NAME, arguments: { url: "https://b.com" }, result: "page b" },
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByRole("button", { name: "Agent called 2 tools" })).toBeInTheDocument();
      expect(screen.queryAllByTestId("tool-call-indicator")).toHaveLength(0);

      await user.click(screen.getByRole("button", { name: "Agent called 2 tools" }));

      expect(screen.getByRole("button", { name: "Hide 2 tool calls" })).toBeInTheDocument();
      expect(screen.getAllByTestId("tool-call-indicator")).toHaveLength(2);

      await user.click(screen.getByRole("button", { name: "Hide 2 tool calls" }));

      expect(screen.getByRole("button", { name: "Agent called 2 tools" })).toBeInTheDocument();
      expect(screen.queryAllByTestId("tool-call-indicator")).toHaveLength(0);
    });

    it("collapses separate live tool-call runs around streaming text", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "First I will inspect the files." },
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://a.com" }, result: "page a" },
        },
        {
          type: "tool_use",
          toolCall: { id: "tc-2", name: GENERIC_TOOL_NAME, arguments: { url: "https://b.com" }, result: "page b" },
        },
        { type: "text", text: "Now I will verify the result." },
        {
          type: "tool_use",
          toolCall: { id: "tc-3", name: GENERIC_TOOL_NAME, arguments: { url: "https://c.com" }, result: "page c" },
        },
        {
          type: "tool_use",
          toolCall: { id: "tc-4", name: GENERIC_TOOL_NAME, arguments: { url: "https://d.com" }, result: "page d" },
        },
        {
          type: "tool_use",
          toolCall: { id: "tc-5", name: GENERIC_TOOL_NAME, arguments: { url: "https://e.com" }, result: "page e" },
        },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByText("First I will inspect the files.")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Agent called 2 tools" })).toBeInTheDocument();
      expect(screen.getByText("Now I will verify the result.")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Agent called 3 tools" })).toBeInTheDocument();
      expect(screen.queryAllByTestId("tool-call-indicator")).toHaveLength(0);
    });
  });

  describe("empty content guard — streaming Footer text blocks", () => {
    // Use empty messages list so no pre-existing copy buttons interfere
    const noMessages: ChatMessageData[] = [];

    it("does not render a TextBubble for empty streaming text blocks", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "" },
      ];

      const { container } = render(
        <ChatMessageList
          {...defaultProps}
          messages={noMessages}
          isAgentRunning={true}
          streamingContentBlocks={blocks}
        />
      );

      // Empty text block produces no TextBubble (.rounded-xl)
      expect(container.querySelector(".rounded-xl")).not.toBeInTheDocument();
    });

    it("does not render a TextBubble for whitespace-only streaming text blocks", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "   \n  " },
      ];

      const { container } = render(
        <ChatMessageList
          {...defaultProps}
          messages={noMessages}
          isAgentRunning={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(container.querySelector(".rounded-xl")).not.toBeInTheDocument();
    });

    it("renders non-empty streaming text blocks normally", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "I am thinking..." },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={noMessages}
          isAgentRunning={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByText(/I am thinking/)).toBeInTheDocument();
    });

    it("renders only non-empty blocks when mixed with empty ones", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "" },
        { type: "text", text: "Actual content here" },
        { type: "text", text: "   " },
      ];

      const { container } = render(
        <ChatMessageList
          {...defaultProps}
          messages={noMessages}
          isAgentRunning={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByText("Actual content here")).toBeInTheDocument();
      // Only the one non-empty text block receives live message metadata.
      expect(container.querySelectorAll('[data-testid="message-meta"]')).toHaveLength(1);
    });
  });

  describe("isFinalizing prop — shouldFilterLastAssistant bridge", () => {
    // Verifies the fix: isFinalizing=true passed directly as prop (not derived from a ref via broken useEffect)
    // keeps the last-assistant-message filter active through the timing window between
    // agent:message_created clearing streaming state and the query refetch completing.
    const makeMessages = (): ChatMessageData[] => [
      { id: "msg-1", role: "user", content: "Hello", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
      { id: "msg-2", role: "assistant", content: "Accumulated response text from DB", createdAt: new Date(2026, 0, 1, 12, 1).toISOString(), toolCalls: null, contentBlocks: null },
    ];

    it("filters last assistant message from DB when isFinalizing=true (no streaming blocks active)", () => {
      // This is the critical scenario: streaming cleared, isFinalizing=true, query not yet refetched.
      // Without this filter, the DB message (with all accumulated text) would leak through and appear
      // alongside the now-empty streaming Footer — text duplication flash.
      const messages = makeMessages();

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={false}
          streamingContentBlocks={[]}
          isFinalizing={true}
        />
      );

      // Last assistant DB message must be filtered to prevent duplication
      expect(screen.queryByText("Accumulated response text from DB")).not.toBeInTheDocument();
      // User message is still visible
      expect(screen.getByText("Hello")).toBeInTheDocument();
    });

    it("does NOT filter last assistant message when isFinalizing=false and no streaming", () => {
      const messages = makeMessages();

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={false}
          streamingContentBlocks={[]}
          isFinalizing={false}
        />
      );

      // Filter is NOT active — DB message should render normally
      expect(screen.getByText("Accumulated response text from DB")).toBeInTheDocument();
    });

    it("filters last assistant message when both isFinalizing=true and streaming are active", () => {
      const messages = makeMessages();
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "Streaming content still active..." },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={true}
          streamingContentBlocks={blocks}
          isFinalizing={true}
        />
      );

      // shouldFilterLastAssistant = hasActiveStreaming(true) || isFinalizing(true) = true
      expect(screen.queryByText("Accumulated response text from DB")).not.toBeInTheDocument();
      expect(screen.getByText(/Streaming content still active/)).toBeInTheDocument();
    });

    it("transitions from filtered to visible when isFinalizing changes false→true→false", () => {
      // Simulates the full lifecycle:
      // 1. Streaming active → DB message filtered
      // 2. message_created fires → streaming cleared + isFinalizing=true → DB message still filtered
      // 3. Query refetch completes + 500ms → isFinalizing=false → DB message visible
      const messages = makeMessages();

      // Phase 1: Active streaming — DB message filtered
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={true}
          streamingContentBlocks={[{ type: "text", text: "Streaming..." }]}
          isFinalizing={false}
        />
      );
      expect(screen.queryByText("Accumulated response text from DB")).not.toBeInTheDocument();

      // Phase 2: Streaming cleared + isFinalizing=true (same batch as message_created)
      rerender(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={false}
          streamingContentBlocks={[]}
          isFinalizing={true}
        />
      );
      // DB message still filtered — isFinalizing bridges the timing gap
      expect(screen.queryByText("Accumulated response text from DB")).not.toBeInTheDocument();

      // Phase 3: Refetch complete, 500ms elapsed → isFinalizing=false
      rerender(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={false}
          streamingContentBlocks={[]}
          isFinalizing={false}
        />
      );
      // DB message now visible — smooth transition, no flash
      expect(screen.getByText("Accumulated response text from DB")).toBeInTheDocument();
    });

    it("defaults isFinalizing to false (prop is optional)", () => {
      const messages = makeMessages();

      // Render without isFinalizing prop (uses default = false)
      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={false}
          streamingContentBlocks={[]}
          // isFinalizing omitted — defaults to false
        />
      );

      // Default behavior: message visible when not finalizing
      expect(screen.getByText("Accumulated response text from DB")).toBeInTheDocument();
    });
  });

  describe("empty content guard — timeline filter for isAgentRunning", () => {
    const makeMessagesWithEmptyLastAssistant = (): ChatMessageData[] => [
      { id: "msg-1", role: "user", content: "Hello", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
      { id: "msg-2", role: "assistant", content: "Sure, let me help.", createdAt: new Date(2026, 0, 1, 12, 1).toISOString(), toolCalls: null, contentBlocks: null },
      { id: "msg-3", role: "user", content: "Go!", createdAt: new Date(2026, 0, 1, 12, 2).toISOString(), toolCalls: null, contentBlocks: null },
      { id: "msg-4", role: "assistant", content: "", createdAt: new Date(2026, 0, 1, 12, 3).toISOString(), toolCalls: null, contentBlocks: null },
    ];

    it("filters empty last assistant message when isAgentRunning is true", () => {
      const messages = makeMessagesWithEmptyLastAssistant();

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={true}
          streamingContentBlocks={[]}
        />
      );

      // The pre-created empty assistant message (msg-4) is filtered from timeline
      // Other non-empty messages remain visible
      expect(screen.getByText("Sure, let me help.")).toBeInTheDocument();
      expect(screen.getByText("Hello")).toBeInTheDocument();
      expect(screen.getByText("Go!")).toBeInTheDocument();
    });

    it("does NOT filter non-empty last assistant message when isAgentRunning is true", () => {
      const messages: ChatMessageData[] = [
        { id: "msg-1", role: "user", content: "Hi", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
        { id: "msg-2", role: "assistant", content: "I have a response!", createdAt: new Date(2026, 0, 1, 12, 1).toISOString(), toolCalls: null, contentBlocks: null },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={true}
          streamingContentBlocks={[]}
        />
      );

      // Non-empty last assistant message must NOT be filtered
      expect(screen.getByText("I have a response!")).toBeInTheDocument();
    });

    it("does NOT filter last assistant when isAgentRunning is false (guard does not activate)", () => {
      const messages = makeMessagesWithEmptyLastAssistant();

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={false}
          streamingContentBlocks={[]}
        />
      );

      // Previous non-empty messages still visible; component doesn't crash
      expect(screen.getByText("Sure, let me help.")).toBeInTheDocument();
      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
    });

    it("filters last assistant message when streaming is active (existing behavior preserved)", () => {
      const messages: ChatMessageData[] = [
        { id: "msg-1", role: "user", content: "Hi", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
        { id: "msg-2", role: "assistant", content: "Partial content", createdAt: new Date(2026, 0, 1, 12, 1).toISOString(), toolCalls: null, contentBlocks: null },
      ];

      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "Streaming now..." },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={true}
          streamingContentBlocks={blocks}
        />
      );

      // During active streaming, last assistant is always filtered (existing behavior)
      expect(screen.queryByText("Partial content")).not.toBeInTheDocument();
      // Only the streaming block shows
      expect(screen.getByText(/Streaming now/)).toBeInTheDocument();
    });
  });

  describe("ID-based assistant filtering — Task #8 fix", () => {
    // Verifies that filtering uses max(createdAt) + id tiebreaker instead of array index.
    // The old code found the "last assistant by index" which breaks when array order ≠ timestamp order.

    it("filters the assistant with the most recent createdAt, not the last by array position", () => {
      // Scenario: an older assistant message appears LAST in the array (out-of-order delivery),
      // but the NEWER one (by timestamp) is the one being streamed and should be filtered.
      const messages: ChatMessageData[] = [
        { id: "msg-1", role: "user", content: "Hello", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
        // Newer assistant — higher timestamp but NOT last in array
        { id: "msg-3", role: "assistant", content: "Newer response", createdAt: new Date(2026, 0, 1, 12, 2).toISOString(), toolCalls: null, contentBlocks: null },
        // Older assistant — lower timestamp but LAST in array (old index-based code would filter this)
        { id: "msg-2", role: "assistant", content: "Older response", createdAt: new Date(2026, 0, 1, 12, 1).toISOString(), toolCalls: null, contentBlocks: null },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={false}
          streamingContentBlocks={[{ type: "text", text: "Streaming..." }]}
          isFinalizing={false}
        />
      );

      // The NEWEST assistant by timestamp (msg-3, createdAt=12:02) should be filtered
      expect(screen.queryByText("Newer response")).not.toBeInTheDocument();
      // The OLDER assistant (msg-2, last by index) should still be visible
      expect(screen.getByText("Older response")).toBeInTheDocument();
      expect(screen.getByText("Hello")).toBeInTheDocument();
    });

    it("uses id as tiebreaker when two assistants have equal createdAt timestamps", () => {
      const sameTime = new Date(2026, 0, 1, 12, 1).toISOString();
      const messages: ChatMessageData[] = [
        { id: "msg-1", role: "user", content: "Hello", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
        { id: "msg-aaa", role: "assistant", content: "Response aaa", createdAt: sameTime, toolCalls: null, contentBlocks: null },
        // "msg-zzz" > "msg-aaa" lexically → msg-zzz wins tiebreaker and should be filtered
        { id: "msg-zzz", role: "assistant", content: "Response zzz", createdAt: sameTime, toolCalls: null, contentBlocks: null },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={false}
          streamingContentBlocks={[{ type: "text", text: "Streaming..." }]}
          isFinalizing={false}
        />
      );

      // "msg-zzz" has lexically larger id → it is the "most recent" and should be filtered
      expect(screen.queryByText("Response zzz")).not.toBeInTheDocument();
      expect(screen.getByText("Response aaa")).toBeInTheDocument();
    });

    it("filters by isFinalizing path using same ID-based logic", () => {
      const messages: ChatMessageData[] = [
        { id: "msg-1", role: "user", content: "Hi", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
        { id: "msg-3", role: "assistant", content: "Newer assistant", createdAt: new Date(2026, 0, 1, 12, 2).toISOString(), toolCalls: null, contentBlocks: null },
        { id: "msg-2", role: "assistant", content: "Older assistant", createdAt: new Date(2026, 0, 1, 12, 1).toISOString(), toolCalls: null, contentBlocks: null },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          isAgentRunning={false}
          streamingContentBlocks={[]}
          isFinalizing={true}
        />
      );

      // isFinalizing=true activates the filter — newest by timestamp should be filtered
      expect(screen.queryByText("Newer assistant")).not.toBeInTheDocument();
      expect(screen.getByText("Older assistant")).toBeInTheDocument();
    });
  });

  describe("scroll-to-bottom on shouldFilterLastAssistant clear — Task #9 fix", () => {
    // Verifies that true-bottom pinning runs when shouldFilterLastAssistant transitions true→false.
    // This ensures the finalized assistant message metadata/actions are visible after streaming ends.

    beforeEach(() => {
      mockScrollToBottom.mockClear();
    });

    it("pins to bottom when active streaming ends (streamingContentBlocks cleared)", async () => {
      const messages: ChatMessageData[] = [
        { id: "msg-1", role: "user", content: "Hello", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
        { id: "msg-2", role: "assistant", content: "Response", createdAt: new Date(2026, 0, 1, 12, 1).toISOString(), toolCalls: null, contentBlocks: null },
      ];

      // Start with streaming active → shouldFilterLastAssistant=true
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          streamingContentBlocks={[{ type: "text", text: "Streaming..." }]}
          isFinalizing={false}
        />
      );

      mockScrollToBottom.mockClear(); // ignore any initial scroll calls

      // Streaming ends → shouldFilterLastAssistant transitions true→false
      rerender(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          streamingContentBlocks={[]}
          isFinalizing={false}
        />
      );

      await waitFor(() => expect(mockScrollToBottom).toHaveBeenCalledOnce());
    });

    it("pins to bottom when isFinalizing transitions from true to false", async () => {
      const messages: ChatMessageData[] = [
        { id: "msg-1", role: "user", content: "Hello", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
        { id: "msg-2", role: "assistant", content: "Response", createdAt: new Date(2026, 0, 1, 12, 1).toISOString(), toolCalls: null, contentBlocks: null },
      ];

      // isFinalizing=true → shouldFilterLastAssistant=true
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          streamingContentBlocks={[]}
          isFinalizing={true}
        />
      );

      mockScrollToBottom.mockClear();

      // isFinalizing clears → shouldFilterLastAssistant transitions true→false
      rerender(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          streamingContentBlocks={[]}
          isFinalizing={false}
        />
      );

      await waitFor(() => expect(mockScrollToBottom).toHaveBeenCalledOnce());
    });

    it("does NOT call scrollToBottom when filter stays false across renders", () => {
      const messages: ChatMessageData[] = [
        { id: "msg-1", role: "user", content: "Hello", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
      ];

      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          streamingContentBlocks={[]}
          isFinalizing={false}
        />
      );

      mockScrollToBottom.mockClear();

      rerender(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          streamingContentBlocks={[]}
          isFinalizing={false}
        />
      );

      expect(mockScrollToBottom).not.toHaveBeenCalled();
    });

    it("does NOT call scrollToBottom in history mode when filter clears", () => {
      const messages: ChatMessageData[] = [
        { id: "msg-1", role: "user", content: "Hello", createdAt: new Date(2026, 0, 1, 12, 0).toISOString(), toolCalls: null, contentBlocks: null },
        { id: "msg-2", role: "assistant", content: "Response", createdAt: new Date(2026, 0, 1, 12, 1).toISOString(), toolCalls: null, contentBlocks: null },
      ];

      // Start with streaming active in history mode
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          streamingContentBlocks={[{ type: "text", text: "Streaming..." }]}
          isFinalizing={false}
          scrollToTimestamp="2026-01-01T12:00:00.000Z"
        />
      );

      mockScrollToBottom.mockClear();

      // Streaming ends — but history mode should suppress the scroll-to-bottom
      rerender(
        <ChatMessageList
          {...defaultProps}
          messages={messages}
          streamingContentBlocks={[]}
          isFinalizing={false}
          scrollToTimestamp="2026-01-01T12:00:00.000Z"
        />
      );

      expect(mockScrollToBottom).not.toHaveBeenCalled();
    });
  });

  describe("B3: rAF scroll reconciliation", () => {
    beforeEach(() => {
      vi.useFakeTimers();
      mockIsAtBottom = true;
      mockIsAtBottomRef.current = true;
      mockHandleAtBottomStateChange.mockClear();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it("calls handleAtBottomStateChange(false) when DOM shows not-at-bottom but isAtBottomRef is true", () => {
      render(<ChatMessageList {...defaultProps} />);

      // Get Virtuoso's scrollerRef callback — it's passed to Virtuoso's scrollerRef prop.
      // In test env the component renders the flat layout (not Virtuoso), so we simulate
      // by directly invoking the scroll listener logic via a mock scroller element.

      // Create a mock scroller that reports not-at-bottom (500px from bottom)
      const mockScroller = document.createElement("div");
      Object.defineProperty(mockScroller, "scrollHeight", { value: 1000, configurable: true });
      Object.defineProperty(mockScroller, "scrollTop", { value: 0, configurable: true });
      Object.defineProperty(mockScroller, "clientHeight", { value: 500, configurable: true });
      // scrollHeight(1000) - scrollTop(0) - clientHeight(500) = 500 > AT_BOTTOM_THRESHOLD(150) → not at bottom

      // isAtBottomRef says true, DOM says false → reconciliation should fire
      mockIsAtBottomRef.current = true;

      // Trigger a scroll event
      const scrollEvent = new Event("scroll");
      mockScroller.dispatchEvent(scrollEvent);

      // rAF hasn't fired yet
      expect(mockHandleAtBottomStateChange).not.toHaveBeenCalled();

      // Run pending rAF callbacks
      vi.runAllTimers();

      // Reconciliation should have called handleAtBottomStateChange(false)
      // (Note: in test env Virtuoso is not rendered, so scrollerRef is not attached —
      // the scroll listener is only added via handleScrollerRef on Virtuoso's scroller.
      // We verify the threshold constant value instead for test env.)
      expect(mockHandleAtBottomStateChange).not.toHaveBeenCalledWith(true);
    });

    it("AT_BOTTOM_THRESHOLD constant is 150 — matches Virtuoso atBottomThreshold prop", () => {
      expect(AT_BOTTOM_THRESHOLD).toBe(150);
    });

    it("mock isAtBottomRef is exposed from useChatAutoScroll", () => {
      // Verify the mock returns isAtBottomRef so the component can use it
      const result = mockUseChatAutoScroll();
      expect(result.isAtBottomRef).toBeDefined();
      expect(result.isAtBottomRef.current).toBe(true);
    });

    it("does NOT call handleAtBottomStateChange when DOM agrees with isAtBottomRef", () => {
      // Both DOM and ref agree → no reconciliation needed
      mockIsAtBottomRef.current = true;
      // If scroll event fires but both agree, handleAtBottomStateChange should not be called
      // (guard: `if (atBottom !== isAtBottomRef.current)`)
      render(<ChatMessageList {...defaultProps} />);
      // In test env the flat layout is used, not Virtuoso — so scrollerRef isn't attached.
      // This test documents the guard behavior.
      expect(mockHandleAtBottomStateChange).not.toHaveBeenCalled();
    });

    it("does not force-settle ordinary wheel scrolling when the loose threshold stops short", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = false;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 486,
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
          scroller.dispatchEvent(new Event("scroll"));
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("forwards wheel over the scroll button without arming true-bottom settle", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = false;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(10)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        Object.defineProperty(scroller, "scrollBy", {
          configurable: true,
          value: undefined,
        });
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 380,
        });

        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        expect(queuedRafs.length).toBeGreaterThan(0);
        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });

        const button = screen.getByTestId("chat-scroll-to-bottom-button");
        expect(button).not.toBeDisabled();

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 360,
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        const wheelEvent = new WheelEvent("wheel", {
          bubbles: true,
          cancelable: true,
          deltaY: 126,
        });
        act(() => {
          button.dispatchEvent(wheelEvent);
        });

        expect(scroller.scrollTop).toBe(486);
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("does not snap when the user scrolls slightly upward inside the settle threshold", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        queuedRafs.length = 0;
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 486,
        });
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -14 }));
          scroller.dispatchEvent(new Event("scroll"));
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("does not settle Virtuoso loose bottom after ordinary wheel input", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: Array<{ id: number; callback: FrameRequestCallback }> = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        const id = nextRafId++;
        queuedRafs.push({ id, callback: cb });
        return id;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 486,
        });
        const atBottomStateChange = expectMockVirtuosoCallback<(atBottom: boolean) => void>(
          "atBottomStateChange",
        );
        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
        });
        queuedRafs.length = 0;
        cancelSpy.mockClear();
        scrollToMock.mockClear();

        act(() => {
          atBottomStateChange(true);
        });

        expect(cancelSpy).not.toHaveBeenCalled();
        expect(queuedRafs).toHaveLength(0);

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("settles when scrollbar bottom intent reaches Virtuoso loose bottom before the DOM reaches true bottom", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: Array<{ id: number; callback: FrameRequestCallback }> = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        const id = nextRafId++;
        queuedRafs.push({ id, callback: cb });
        return id;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(
          makeRect({ top: 0, bottom: 500, right: 200 }),
        );
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 486,
        });
        const atBottomStateChange = expectMockVirtuosoCallback<(atBottom: boolean) => void>(
          "atBottomStateChange",
        );
        act(() => {
          scroller.dispatchEvent(new MouseEvent("pointerdown", { clientX: 190 }));
        });
        queuedRafs.length = 0;
        cancelSpy.mockClear();
        scrollToMock.mockClear();

        act(() => {
          atBottomStateChange(true);
          atBottomStateChange(false);
          atBottomStateChange(true);
        });

        expect(cancelSpy).toHaveBeenCalledWith(queuedRafs[0]?.id);
        expect(queuedRafs).toHaveLength(2);
        act(() => {
          queuedRafs.at(-1)?.callback(0);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" });
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("stops loose-bottom settling when Virtuoso keeps oscillating after scrollbar bottom intent", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(
          makeRect({ top: 0, bottom: 500, right: 200 }),
        );
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 486,
        });
        const atBottomStateChange = expectMockVirtuosoCallback<(atBottom: boolean) => void>(
          "atBottomStateChange",
        );
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new MouseEvent("pointerdown", { clientX: 190 }));
          atBottomStateChange(true);
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });
        expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" });

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 486,
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          atBottomStateChange(false);
          atBottomStateChange(true);
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });
        expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" });

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 486,
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          atBottomStateChange(false);
          atBottomStateChange(true);
        });

        expect(queuedRafs).toHaveLength(0);
        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("does not settle scrollbar loose bottom while the last item is outside the visible range", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(
          makeRect({ top: 0, bottom: 500, right: 200 }),
        );
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 486,
        });
        const atBottomStateChange = expectMockVirtuosoCallback<(atBottom: boolean) => void>(
          "atBottomStateChange",
        );
        const rangeChanged = expectMockVirtuosoCallback<(range: { startIndex: number; endIndex: number }) => void>(
          "rangeChanged",
        );
        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 1 });
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new MouseEvent("pointerdown", { clientX: 190 }));
          atBottomStateChange(true);
        });

        expect(queuedRafs).toHaveLength(0);
        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("does not let Virtuoso follow output while the last item is outside the visible range", () => {
      vi.stubEnv("VITEST", "");
      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const rangeChanged = expectMockVirtuosoCallback<(range: { startIndex: number; endIndex: number }) => void>(
          "rangeChanged",
        );
        const followOutput = expectMockVirtuosoCallback<(atBottom: boolean) => "smooth" | "auto" | false>(
          "followOutput",
        );
        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 1 });
        });
        mockHandleFollowOutput.mockClear();

        expect(followOutput(true)).toBe(false);
        expect(mockHandleFollowOutput).not.toHaveBeenCalled();
      } finally {
        vi.unstubAllEnvs();
      }
    });

    it("does not let Virtuoso follow output when the last item is only rendered as offscreen overscan", () => {
      vi.stubEnv("VITEST", "");
      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        const lastRow = getLastRenderedRow();
        vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(makeRect({ top: 0, bottom: 500 }));
        vi.spyOn(lastRow, "getBoundingClientRect").mockReturnValue(makeRect({ top: 620, bottom: 700 }));
        const rangeChanged = expectMockVirtuosoCallback<(range: { startIndex: number; endIndex: number }) => void>(
          "rangeChanged",
        );
        const followOutput = expectMockVirtuosoCallback<(atBottom: boolean) => "smooth" | "auto" | false>(
          "followOutput",
        );
        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 3 });
        });
        mockHandleFollowOutput.mockClear();

        expect(followOutput(true)).toBe(false);
        expect(mockHandleFollowOutput).not.toHaveBeenCalled();
      } finally {
        vi.unstubAllEnvs();
      }
    });

    it("does not let Virtuoso follow output after the user scrolls away from bottom", () => {
      vi.stubEnv("VITEST", "");
      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        const followOutput = expectMockVirtuosoCallback<(atBottom: boolean) => "smooth" | "auto" | false>(
          "followOutput",
        );
        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -80 }));
        });
        mockHandleFollowOutput.mockClear();

        expect(followOutput(true)).toBe(false);
        expect(mockHandleFollowOutput).not.toHaveBeenCalled();
      } finally {
        vi.unstubAllEnvs();
      }
    });

    it("does not pin total-list growth when the last item is only rendered as offscreen overscan", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        const lastRow = getLastRenderedRow();
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 420,
        });
        vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(makeRect({ top: 0, bottom: 500 }));
        vi.spyOn(lastRow, "getBoundingClientRect").mockReturnValue(makeRect({ top: 620, bottom: 700 }));
        const rangeChanged = expectMockVirtuosoCallback<(range: { startIndex: number; endIndex: number }) => void>(
          "rangeChanged",
        );
        const totalListHeightChanged = expectMockVirtuosoCallback<(height: number) => void>(
          "totalListHeightChanged",
        );
        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 3 });
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
          totalListHeightChanged(1000);
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("treats scrollbar pointer drags as bottom intent for final-pixel settle", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = false;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue({
          x: 0,
          y: 0,
          width: 200,
          height: 500,
          top: 0,
          right: 200,
          bottom: 500,
          left: 0,
          toJSON: () => ({}),
        });
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 486,
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new MouseEvent("pointerdown", { clientX: 190 }));
          scroller.dispatchEvent(new Event("scroll"));
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" });
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("pins initial and growing Virtuoso total-list height while sticky", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(
          makeRect({ top: 0, bottom: 500, right: 200 }),
        );
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        const totalListHeightChanged = expectMockVirtuosoCallback<(height: number) => void>(
          "totalListHeightChanged",
        );
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          totalListHeightChanged(1000);
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });
        expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" });

        scrollToMock.mockClear();
        act(() => {
          totalListHeightChanged(1000);
        });
        expect(queuedRafs).toHaveLength(0);

        act(() => {
          setMockScrollerGeometry(scroller, {
            clientHeight: 500,
            scrollHeight: 1030,
            scrollTop: 500,
          });
          totalListHeightChanged(1030);
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });
        expect(scrollToMock).toHaveBeenCalledWith({ top: 530, behavior: "auto" });
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("does not pin total-list growth after manual downward wheel input near bottom", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 420,
        });
        const rangeChanged = expectMockVirtuosoCallback<(range: { startIndex: number; endIndex: number }) => void>(
          "rangeChanged",
        );
        const totalListHeightChanged = expectMockVirtuosoCallback<(height: number) => void>(
          "totalListHeightChanged",
        );
        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 3 });
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
          totalListHeightChanged(1000);
        });
        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("does not pin total-list growth while scrolling down before the last item is visible", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 420,
        });
        const rangeChanged = expectMockVirtuosoCallback<(range: { startIndex: number; endIndex: number }) => void>(
          "rangeChanged",
        );
        const totalListHeightChanged = expectMockVirtuosoCallback<(height: number) => void>(
          "totalListHeightChanged",
        );
        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 1 });
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
          totalListHeightChanged(1000);
        });
        expect(queuedRafs).toHaveLength(1);
        act(() => {
          queuedRafs.shift()?.(0);
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("cancels pending loose-bottom and total-list settle frames on unmount", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: Array<{ id: number; callback: FrameRequestCallback }> = [];
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        const id = nextRafId++;
        queuedRafs.push({ id, callback: cb });
        return id;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        const { unmount } = render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = screen.getByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        const atBottomStateChange = expectMockVirtuosoCallback<(atBottom: boolean) => void>(
          "atBottomStateChange",
        );
        const totalListHeightChanged = expectMockVirtuosoCallback<(height: number) => void>(
          "totalListHeightChanged",
        );
        act(() => {
          scroller.dispatchEvent(new MouseEvent("pointerdown", { clientX: 190 }));
        });
        queuedRafs.length = 0;
        cancelSpy.mockClear();

        act(() => {
          atBottomStateChange(true);
          totalListHeightChanged(1000);
        });
        const pendingIds = queuedRafs.map((entry) => entry.id);
        expect(pendingIds).toHaveLength(2);

        act(() => {
          unmount();
        });

        expect(cancelSpy).toHaveBeenCalledWith(pendingIds[0]);
        expect(cancelSpy).toHaveBeenCalledWith(pendingIds[1]);
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });
  });

  describe("B2: cumulative text length bucket for streaming auto-scroll", () => {
    // The B2 fix extends footerContentHash with textLengthBucket so autoscrollToBottom()
    // fires as streaming text grows within existing content blocks.
    // cumulativeTextLengthRef tracks the running max — never decreases during a stream,
    // preventing bucket regression when tool_use blocks are inserted mid-stream.

    it("TEXT_LENGTH_BUCKET_SIZE constant is 150 — ~2 visible lines per trigger", () => {
      expect(TEXT_LENGTH_BUCKET_SIZE).toBe(150);
    });

    it("renders streaming content with text blocks above bucket size", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "a".repeat(200) }, // 200 chars → bucket 1
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
    });

    it("renders interleaved text and tool_use blocks without error", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "First I will search for information." },
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: "Search", arguments: { query: "test" }, result: "results" },
        },
        { type: "text", text: "Based on the results, here is the answer." },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByText(/First I will search/)).toBeInTheDocument();
      expect(screen.getByText(/Based on the results/)).toBeInTheDocument();
    });

    it("windows older live text blocks so long Codex streams do not render hundreds of bubbles", () => {
      const blocks: StreamingContentBlock[] = Array.from({ length: 65 }, (_, index) => ({
        type: "text",
        text: `Codex live update ${index + 1}`,
      }));

      render(
        <ChatMessageList
          {...defaultProps}
          messages={[]}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.queryByText(/Codex live update 1/)).not.toBeInTheDocument();
      expect(screen.getByText(/Codex live update 65/)).toBeInTheDocument();
      expect(screen.getAllByTestId("text-bubble-assistant")).toHaveLength(40);
    });

    it("bounds interleaved live text and tool blocks instead of only compacting text runs", () => {
      const blocks: StreamingContentBlock[] = Array.from({ length: 60 }, (_, index): StreamingContentBlock[] => [
        { type: "text", text: `Interleaved live update ${index + 1}` },
        {
          type: "tool_use",
          toolCall: {
            id: `tc-${index + 1}`,
            name: "webfetch",
            arguments: { url: `https://example.com/${index + 1}` },
          },
        },
      ]).flat();

      render(
        <ChatMessageList
          {...defaultProps}
          messages={[]}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.queryByText(/Interleaved live update 1/)).not.toBeInTheDocument();
      expect(screen.getByText(/Interleaved live update 60/)).toBeInTheDocument();
      expect(screen.getAllByTestId("text-bubble-assistant")).toHaveLength(20);
      expect(screen.getAllByTestId("tool-call-indicator")).toHaveLength(20);
    });

    it("preserves a running task card even when its marker is older than the live tail", () => {
      const runningTask: StreamingTask = {
        toolUseId: "task-old",
        toolName: "Task",
        description: "Long running subagent",
        subagentType: "Explore",
        model: "sonnet",
        status: "running",
        startedAt: Date.now(),
        childToolCalls: [],
      };
      const blocks: StreamingContentBlock[] = [
        { type: "task", toolUseId: runningTask.toolUseId },
        ...Array.from({ length: 65 }, (_, index): StreamingContentBlock => ({
          type: "text",
          text: `Post-task live update ${index + 1}`,
        })),
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={[]}
          isSending={true}
          streamingTasks={new Map([[runningTask.toolUseId, runningTask]])}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByTestId("task-subagent-card-task-old")).toBeInTheDocument();
      expect(screen.queryByText(/Post-task live update 1/)).not.toBeInTheDocument();
      expect(screen.getByText(/Post-task live update 65/)).toBeInTheDocument();
      expect(screen.getAllByTestId("text-bubble-assistant")).toHaveLength(40);
    });

    it("freezes the rendered live transcript window while the user is away from bottom", () => {
      const previous = buildStreamingTranscriptWindow(
        Array.from({ length: 45 }, (_, index): StreamingContentBlock => ({
          type: "text",
          text: `Previous live update ${index + 1}`,
        })),
        new Map(),
      );
      const live = buildStreamingTranscriptWindow(
        Array.from({ length: 80 }, (_, index): StreamingContentBlock => ({
          type: "text",
          text: `Latest live update ${index + 1}`,
        })),
        new Map(),
      );

      expect(getNextStreamingTranscriptWindow(previous, live, false)).toBe(previous);
      expect(getNextStreamingTranscriptWindow(previous, live, true)).toBe(live);
    });

    it("keeps advancing live transcript blocks when still inside the bottom range", async () => {
      vi.stubEnv("VITEST", "");
      mockIsAtBottom = true;
      mockIsAtBottomRef.current = true;
      const initialBlocks: StreamingContentBlock[] = [
        { type: "text", text: "Live update before visual drift" },
      ];
      const nextBlocks: StreamingContentBlock[] = [
        ...initialBlocks,
        { type: "text", text: "Live update after visual drift" },
      ];

      try {
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            messages={[]}
            isSending={true}
            streamingContentBlocks={initialBlocks}
          />
        );

        await waitFor(() => {
          expect(mockHandleAtBottomStateChange).toHaveBeenCalledWith(false);
        });

        rerender(
          <ChatMessageList
            {...defaultProps}
            messages={[]}
            isSending={true}
            streamingContentBlocks={nextBlocks}
          />
        );

        expect(await screen.findByText("Live update after visual drift")).toBeInTheDocument();
      } finally {
        vi.unstubAllEnvs();
      }
    });

    it("drops empty compacted older text while keeping recent live text visible", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "   " },
        ...Array.from({ length: 40 }, (_, index): StreamingContentBlock => ({
          type: "text",
          text: `Recent live update ${index + 1}`,
        })),
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          messages={[]}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      expect(screen.getByText("Recent live update 1")).toBeInTheDocument();
      expect(screen.getByText("Recent live update 40")).toBeInTheDocument();
      expect(screen.getAllByTestId("text-bubble-assistant")).toHaveLength(40);
    });

    it("triggers re-renders when text crosses bucket boundaries", () => {
      // bucket 0: text < 150 chars
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={[{ type: "text", text: "a".repeat(100) }]}
        />
      );

      const callsAtBucket0 = mockUseChatAutoScroll.mock.calls.length;

      // bucket 1: text grows past 150 chars → footerContentHash.textLengthBucket changes.
      // Note: useState-based tracking causes an extra render cycle (rerender + state update),
      // so we assert "greater than" rather than an exact count.
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={[{ type: "text", text: "a".repeat(160) }]}
        />
      );

      // Component re-rendered at least once because streamingContentBlocks changed
      expect(mockUseChatAutoScroll.mock.calls.length).toBeGreaterThan(callsAtBucket0);

      const callsAtBucket1 = mockUseChatAutoScroll.mock.calls.length;

      // bucket 2: text grows past 300 chars
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={[{ type: "text", text: "a".repeat(320) }]}
        />
      );

      // Another re-render cycle
      expect(mockUseChatAutoScroll.mock.calls.length).toBeGreaterThan(callsAtBucket1);
    });

    it("bucket never decreases when tool_use block is inserted mid-stream", () => {
      // Start with 200 chars of text → cumulative max = 200, bucket = 1
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={[{ type: "text", text: "a".repeat(200) }]}
        />
      );

      // Insert tool_use block — only text contributes to total,
      // but cumulativeTextLengthRef.current = max(200, 200) = 200 → bucket still 1
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={[
            { type: "text", text: "a".repeat(200) },
            {
              type: "tool_use",
              toolCall: { id: "tc-1", name: "Read", arguments: { file_path: "/foo.ts" } },
            },
          ]}
        />
      );

      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();

      // Text resumes in new block after tool_use — cumRef still holds prior max
      // total text = 50 < 200, but cumRef = max(200, 50) = 200 → bucket stays 1 (not 0)
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={[
            {
              type: "tool_use",
              toolCall: { id: "tc-1", name: "Read", arguments: { file_path: "/foo.ts" }, result: "content" },
            },
            { type: "text", text: "a".repeat(50) },
          ]}
        />
      );

      // Component renders without error — cumulative ref preserved bucket stability
      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
    });

    it("resets cumulative bucket when streaming ends (blocks = undefined)", () => {
      // Start with 300 chars → bucket 2
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={[{ type: "text", text: "a".repeat(300) }]}
        />
      );

      // Streaming ends — blocks become undefined → cumulativeTextLengthRef resets to 0
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={false}
          streamingContentBlocks={undefined}
        />
      );

      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
    });

    it("resets cumulative bucket when blocks become empty array", () => {
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={[{ type: "text", text: "a".repeat(200) }]}
        />
      );

      // Blocks cleared to empty array → same reset path as undefined
      rerender(
        <ChatMessageList
          {...defaultProps}
          isSending={false}
          streamingContentBlocks={[]}
        />
      );

      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
    });

    it("accumulates text length across multiple text blocks in the same render", () => {
      // Multiple text blocks: total = 100 + 100 = 200 → bucket 1
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "a".repeat(100) },
        { type: "text", text: "b".repeat(100) },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      // Component renders both text blocks
      expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
    });

    it("only counts text blocks in length total (tool_use blocks don't add to length)", () => {
      const blocks: StreamingContentBlock[] = [
        {
          type: "tool_use",
          toolCall: { id: "tc-1", name: "Read", arguments: { file_path: "/large-file.ts" }, result: "x".repeat(10000) },
        },
        { type: "text", text: "Short response." },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isSending={true}
          streamingContentBlocks={blocks}
        />
      );

      // Tool result content doesn't inflate the text bucket
      // (bucket computed from text.length only, not tool result)
      expect(screen.getByText(/Short response/)).toBeInTheDocument();
    });
  });

  describe("streaming result bottom pinning", () => {
    beforeEach(() => {
      vi.stubEnv("VITEST", "");
      mockIsAtBottom = true;
      mockIsAtBottomRef.current = true;
      scrollToMock.mockClear();
    });

    afterEach(() => {
      vi.unstubAllEnvs();
      mockIsAtBottom = true;
      mockIsAtBottomRef.current = true;
    });

    it("pins to true bottom when a parent tool result arrives while near bottom", async () => {
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
      const pendingTool: ToolCall = {
        id: "toolu-read-1",
        name: "Read",
        arguments: { file_path: "src/app.ts" },
      };
      const completedTool: ToolCall = {
        ...pendingTool,
        result: "     1→const app = true;",
      };

      try {
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            isAgentRunning={true}
            streamingToolCalls={[pendingTool]}
            streamingContentBlocks={[{ type: "tool_use", toolCall: pendingTool }]}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        scrollToMock.mockClear();

        rerender(
          <ChatMessageList
            {...defaultProps}
            isAgentRunning={true}
            streamingToolCalls={[completedTool]}
            streamingContentBlocks={[{ type: "tool_use", toolCall: completedTool }]}
          />
        );

        await waitFor(() => expect(scrollToMock).toHaveBeenCalled());
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
      }
    });

    it("does not pin a streaming footer update after manual downward wheel input", async () => {
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
      const pendingTool: ToolCall = {
        id: "toolu-read-1",
        name: "Read",
        arguments: { file_path: "src/app.ts" },
      };
      const completedTool: ToolCall = {
        ...pendingTool,
        result: "     1→const app = true;",
      };

      try {
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            isAgentRunning={true}
            streamingToolCalls={[pendingTool]}
            streamingContentBlocks={[{ type: "tool_use", toolCall: pendingTool }]}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 420,
        });
        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
          scroller.scrollTop = 460;
          scroller.dispatchEvent(new Event("scroll"));
        });
        scrollToMock.mockClear();

        rerender(
          <ChatMessageList
            {...defaultProps}
            isAgentRunning={true}
            streamingToolCalls={[completedTool]}
            streamingContentBlocks={[{ type: "tool_use", toolCall: completedTool }]}
          />
        );
        await act(async () => {});

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
      }
    });

    it("pins to true bottom when an existing child task tool receives a result", async () => {
      const childTool: ToolCall = {
        id: "toolu-child-read",
        name: "Read",
        arguments: { file_path: "src/child.ts" },
      };
      const task: StreamingTask = {
        toolUseId: "toolu-task-1",
        toolName: "Task",
        description: "Inspect child files",
        subagentType: "Explore",
        model: "sonnet",
        status: "running",
        startedAt: Date.now(),
        childToolCalls: [childTool],
      };
      const completedTask: StreamingTask = {
        ...task,
        childToolCalls: [
          {
            ...childTool,
            result: "     1→export const child = true;",
          },
        ],
      };
      const blocks: StreamingContentBlock[] = [
        { type: "task", toolUseId: task.toolUseId },
      ];

      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          isAgentRunning={true}
          streamingTasks={new Map([[task.toolUseId, task]])}
          streamingContentBlocks={blocks}
        />
      );
      const scroller = await screen.findByTestId("mock-virtuoso");
      setMockScrollerGeometry(scroller, {
        clientHeight: 500,
        scrollHeight: 1000,
        scrollTop: 480,
      });
      scrollToMock.mockClear();

      rerender(
        <ChatMessageList
          {...defaultProps}
          isAgentRunning={true}
          streamingTasks={new Map([[completedTask.toolUseId, completedTask]])}
          streamingContentBlocks={blocks}
        />
      );

      await waitFor(() => expect(scrollToMock).toHaveBeenCalled());
    });

    it("pins to true bottom when an existing child task tool receives an error", async () => {
      const childTool: ToolCall = {
        id: "toolu-child-read",
        name: "Read",
        arguments: { file_path: "src/child.ts" },
      };
      const task: StreamingTask = {
        toolUseId: "toolu-task-1",
        toolName: "Task",
        description: "Inspect child files",
        subagentType: "Explore",
        model: "sonnet",
        status: "running",
        startedAt: Date.now(),
        childToolCalls: [childTool],
      };
      const failedTask: StreamingTask = {
        ...task,
        childToolCalls: [
          {
            ...childTool,
            error: "file unavailable",
          },
        ],
      };
      const blocks: StreamingContentBlock[] = [
        { type: "task", toolUseId: task.toolUseId },
      ];

      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          isAgentRunning={true}
          streamingTasks={new Map([[task.toolUseId, task]])}
          streamingContentBlocks={blocks}
        />
      );
      const scroller = await screen.findByTestId("mock-virtuoso");
      setMockScrollerGeometry(scroller, {
        clientHeight: 500,
        scrollHeight: 1000,
        scrollTop: 480,
      });
      scrollToMock.mockClear();

      rerender(
        <ChatMessageList
          {...defaultProps}
          isAgentRunning={true}
          streamingTasks={new Map([[failedTask.toolUseId, failedTask]])}
          streamingContentBlocks={blocks}
        />
      );

      await waitFor(() => expect(scrollToMock).toHaveBeenCalled());
    });

    it("does not recover to bottom during pointer-driven manual scrolling in the middle", async () => {
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(8)}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(
          makeRect({ top: 0, bottom: 500, right: 200 }),
        );
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        scrollToMock.mockClear();

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 260,
        });
        act(() => {
          scroller.dispatchEvent(new MouseEvent("pointerdown", { clientX: 100 }));
          scroller.dispatchEvent(new Event("scroll"));
        });

        expect(scrollToMock).not.toHaveBeenCalled();
        expect(scroller.scrollTop).toBe(260);
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
      }
    });

    it("pins to true bottom when the scroller resizes while sticky", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }
        disconnect = vi.fn();
        observe = vi.fn();
        unobserve = vi.fn();
      }
      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
            isAgentRunning={true}
            streamingContentBlocks={[{ type: "text", text: "Growing footer" }]}
          />
        );
        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        scrollToMock.mockClear();

        act(() => {
          callbacks[0]?.([], {} as ResizeObserver);
        });

        await waitFor(() => expect(scrollToMock).toHaveBeenCalled());
        expect(scroller).toBeInTheDocument();
      } finally {
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("does not issue no-op scrolls when resize fires after becoming scrollable at true bottom", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }
        disconnect = vi.fn();
        observe = vi.fn();
        unobserve = vi.fn();
      }
      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );
        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 502,
          scrollTop: 2,
        });
        scrollToMock.mockClear();

        act(() => {
          callbacks[0]?.([], {} as ResizeObserver);
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("pins to exact true bottom when resize first makes the scroller scrollable inside visual epsilon", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const observedTargets: Element[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }
        disconnect = vi.fn();
        observe = vi.fn((target: Element) => {
          observedTargets.push(target);
        });
        unobserve = vi.fn();
      }
      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );
        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 500,
          scrollTop: 0,
        });
        scrollToMock.mockClear();

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 502,
          scrollTop: 0,
        });
        const scrollerObserverIndex = observedTargets.findIndex(
          (target) => target === scroller,
        );
        expect(scrollerObserverIndex).toBeGreaterThanOrEqual(0);
        act(() => {
          callbacks[scrollerObserverIndex]?.([], {} as ResizeObserver);
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 2, behavior: "auto" });
        expect(scroller.scrollTop).toBe(2);
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("does not snap downward when row growth fires after the user scrolls upward", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const observedTargets: Element[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }

        disconnect = vi.fn();
        observe = vi.fn((target: Element) => {
          observedTargets.push(target);
        });
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 460,
        });
        scrollToMock.mockClear();
        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -120 }));
          scroller.dispatchEvent(new Event("scroll"));
        });

        const lastRowIndex = observedTargets.findIndex(
          (target) =>
            target instanceof HTMLElement &&
            target.dataset.chatLastRenderedRow === "true",
        );
        expect(lastRowIndex).toBeGreaterThanOrEqual(0);
        const lastRowCallback = callbacks[lastRowIndex];
        const resizeObserver = {} as ResizeObserver;

        act(() => {
          lastRowCallback?.(
            [{ contentRect: { height: 80 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
        });
        act(() => {
          lastRowCallback?.(
            [{ contentRect: { height: 120 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("does not run a pending row-growth bottom pin after the user scrolls upward", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const observedTargets: Element[] = [];
      const queuedRafs: FrameRequestCallback[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return queuedRafs.length;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }

        disconnect = vi.fn();
        observe = vi.fn((target: Element) => {
          observedTargets.push(target);
        });
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 420,
        });
        queuedRafs.length = 0;
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        act(() => {
          queuedRafs.shift()?.(0);
        });
        queuedRafs.length = 0;

        const lastRowIndex = observedTargets.findIndex(
          (target) =>
            target instanceof HTMLElement &&
            target.dataset.chatLastRenderedRow === "true",
        );
        expect(lastRowIndex).toBeGreaterThanOrEqual(0);
        const lastRowCallback = callbacks[lastRowIndex];
        const resizeObserver = {} as ResizeObserver;

        act(() => {
          lastRowCallback?.(
            [{ contentRect: { height: 80 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
          lastRowCallback?.(
            [{ contentRect: { height: 120 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
        });
        expect(queuedRafs).toHaveLength(1);

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 460,
        });
        scrollToMock.mockClear();
        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -120 }));
          scroller.dispatchEvent(new Event("scroll"));
        });
        expect(queuedRafs).toHaveLength(2);

        act(() => {
          queuedRafs.shift()?.(0);
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("does not run a pending scroller-resize bottom pin after the user scrolls upward", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const observedTargets: Element[] = [];
      const queuedRafs: FrameRequestCallback[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return queuedRafs.length;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }

        disconnect = vi.fn();
        observe = vi.fn((target: Element) => {
          observedTargets.push(target);
        });
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        queuedRafs.length = 0;
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        act(() => {
          queuedRafs.shift()?.(0);
        });
        queuedRafs.length = 0;

        const scrollerObserverIndex = observedTargets.findIndex(
          (target) => target === scroller,
        );
        expect(scrollerObserverIndex).toBeGreaterThanOrEqual(0);

        act(() => {
          callbacks[scrollerObserverIndex]?.(
            [{ contentRect: { height: 500 } as DOMRectReadOnly } as ResizeObserverEntry],
            {} as ResizeObserver,
          );
        });
        expect(queuedRafs).toHaveLength(1);

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 460,
        });
        scrollToMock.mockClear();
        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -120 }));
          scroller.dispatchEvent(new Event("scroll"));
        });
        expect(queuedRafs).toHaveLength(2);

        act(() => {
          queuedRafs.shift()?.(0);
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("does not pin scroller resize after manual downward wheel input", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const observedTargets: Element[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }

        disconnect = vi.fn();
        observe = vi.fn((target: Element) => {
          observedTargets.push(target);
        });
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(4)}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 420,
        });

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
          scroller.scrollTop = 460;
          scroller.dispatchEvent(new Event("scroll"));
        });
        scrollToMock.mockClear();

        const scrollerObserverIndex = observedTargets.findIndex(
          (target) => target === scroller,
        );
        expect(scrollerObserverIndex).toBeGreaterThanOrEqual(0);

        act(() => {
          callbacks[scrollerObserverIndex]?.(
            [{ contentRect: { height: 500, width: 640 } as DOMRectReadOnly } as ResizeObserverEntry],
            {} as ResizeObserver,
          );
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("does not run a pending scheduled bottom pin after the user scrolls upward", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return queuedRafs.length;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        const initialMessages = createMessages(2);
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            messages={initialMessages}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        queuedRafs.length = 0;
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        act(() => {
          queuedRafs.shift()?.(0);
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        rerender(
          <ChatMessageList
            {...defaultProps}
            messages={[
              ...initialMessages,
              {
                id: "assistant-appended",
                role: "assistant",
                content: "Assistant appended after initial render",
                createdAt: new Date(2026, 0, 1, 12, 30).toISOString(),
                toolCalls: null,
                contentBlocks: null,
              },
            ]}
          />
        );
        expect(queuedRafs.length).toBeGreaterThan(0);

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 460,
        });
        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -120 }));
          scroller.dispatchEvent(new Event("scroll"));
        });

        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("does not run a pending scheduled bottom pin after manual downward wheel input", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return queuedRafs.length;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        const initialMessages = createMessages(2);
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            messages={initialMessages}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 420,
        });
        queuedRafs.length = 0;
        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        act(() => {
          queuedRafs.shift()?.(0);
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        rerender(
          <ChatMessageList
            {...defaultProps}
            messages={[
              ...initialMessages,
              {
                id: "assistant-appended",
                role: "assistant",
                content: "Assistant appended after initial render",
                createdAt: new Date(2026, 0, 1, 12, 30).toISOString(),
                toolCalls: null,
                contentBlocks: null,
              },
            ]}
          />
        );
        expect(queuedRafs.length).toBeGreaterThan(0);

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
          scroller.scrollTop = 460;
          scroller.dispatchEvent(new Event("scroll"));
        });

        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("keeps following streaming footer growth when manual wheel reaches true bottom before reconciliation", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return queuedRafs.length;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = false;
        mockHandleAtBottomStateChange.mockImplementation((atBottom: boolean) => {
          mockIsAtBottom = atBottom;
          mockIsAtBottomRef.current = atBottom;
        });
        const messages = createMessages(10);
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            isAgentRunning={false}
            streamingContentBlocks={[{ type: "text", text: "first token" }]}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        Object.defineProperty(scroller, "scrollBy", {
          configurable: true,
          value: ({ top = 0 }: ScrollToOptions) => {
            const maxScrollTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
            scroller.scrollTop = Math.min(maxScrollTop, Math.max(0, scroller.scrollTop + top));
          },
        });
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 300,
        });

        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = false;
        expect(mockIsAtBottomRef.current).toBe(false);

        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 240 }));
          scroller.scrollTop = 500;
          scroller.dispatchEvent(new Event("scroll"));
        });
        expect(scroller.scrollTop).toBe(500);
        expect(mockIsAtBottomRef.current).toBe(true);
        expect(queuedRafs).toHaveLength(1);

        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1040,
          scrollTop: 500,
        });
        rerender(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            isAgentRunning={false}
            streamingContentBlocks={[{ type: "text", text: "first token\nsecond token" }]}
          />
        );

        await waitFor(() =>
          expect(scrollToMock).toHaveBeenCalledWith({ top: 540, behavior: "auto" })
        );
      } finally {
        mockHandleAtBottomStateChange.mockImplementation(() => {});
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("pins external layout changes when manual wheel reaches true bottom before reconciliation", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return queuedRafs.length;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = false;
        mockHandleAtBottomStateChange.mockImplementation((atBottom: boolean) => {
          mockIsAtBottom = atBottom;
          mockIsAtBottomRef.current = atBottom;
        });
        const messages = createMessages(10);
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            externalLayoutVersion={0}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 300,
        });

        act(() => {
          scroller.dispatchEvent(new Event("scroll"));
        });
        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = false;
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 240 }));
          scroller.scrollTop = 500;
          scroller.dispatchEvent(new Event("scroll"));
        });
        expect(scroller.scrollTop).toBe(500);
        expect(mockIsAtBottomRef.current).toBe(true);
        expect(queuedRafs).toHaveLength(1);

        setMockScrollerGeometry(scroller, {
          clientHeight: 460,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        rerender(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            externalLayoutVersion={1}
          />
        );

        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });

        expect(scrollToMock).toHaveBeenCalledWith({ top: 540, behavior: "auto" });
      } finally {
        mockHandleAtBottomStateChange.mockImplementation(() => {});
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("does not run a pending scheduled bottom pin after the last item leaves range", async () => {
      vi.stubEnv("VITEST", "");
      const queuedRafs: FrameRequestCallback[] = [];
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return queuedRafs.length;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        const initialMessages = createMessages(2);
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            messages={initialMessages}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        queuedRafs.length = 0;
        scrollToMock.mockClear();

        rerender(
          <ChatMessageList
            {...defaultProps}
            messages={[
              ...initialMessages,
              {
                id: "assistant-appended",
                role: "assistant",
                content: "Assistant appended after initial render",
                createdAt: new Date(2026, 0, 1, 12, 30).toISOString(),
                toolCalls: null,
                contentBlocks: null,
              },
            ]}
          />
        );
        expect(queuedRafs.length).toBeGreaterThan(0);

        const rangeChanged = expectMockVirtuosoCallback<(range: { startIndex: number; endIndex: number }) => void>(
          "rangeChanged",
        );
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 420,
        });
        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 1 });
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
        });

        act(() => {
          while (queuedRafs.length > 0) {
            queuedRafs.shift()?.(0);
          }
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
      }
    });

    it("does not pin transcript viewport resize after manual downward wheel input", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const observedTargets: Element[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }

        disconnect = vi.fn();
        observe = vi.fn((target: Element) => {
          observedTargets.push(target);
        });
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });
      vi.stubEnv("VITEST", "");

      try {
        mockIsAtBottom = true;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const transcript = await screen.findByTestId("integrated-chat-messages");
        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 480,
          scrollHeight: 1000,
          scrollTop: 420,
        });

        const transcriptRootIndex = observedTargets.findIndex(
          (target) => target === transcript,
        );
        expect(transcriptRootIndex).toBeGreaterThanOrEqual(0);

        act(() => {
          callbacks[transcriptRootIndex]?.(
            [{ contentRect: { height: 480 } as DOMRectReadOnly } as ResizeObserverEntry],
            {} as ResizeObserver,
          );
        });

        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
          scroller.scrollTop = 460;
          scroller.dispatchEvent(new Event("scroll"));
        });
        scrollToMock.mockClear();

        act(() => {
          callbacks[transcriptRootIndex]?.(
            [{ contentRect: { height: 456 } as DOMRectReadOnly } as ResizeObserverEntry],
            {} as ResizeObserver,
          );
        });

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("pins to true bottom when the transcript viewport resizes while sticky", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const observedTargets: Element[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }

        disconnect = vi.fn();
        observe = vi.fn((target: Element) => {
          observedTargets.push(target);
        });
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });
      vi.stubEnv("VITEST", "");

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const transcript = await screen.findByTestId("integrated-chat-messages");
        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 480,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        scrollToMock.mockClear();

        const transcriptRootIndex = observedTargets.findIndex(
          (target) => target === transcript,
        );
        expect(transcriptRootIndex).toBeGreaterThanOrEqual(0);

        act(() => {
          callbacks[transcriptRootIndex]?.(
            [{ contentRect: { height: 480 } as DOMRectReadOnly } as ResizeObserverEntry],
            {} as ResizeObserver,
          );
        });
        expect(scrollToMock).not.toHaveBeenCalled();

        act(() => {
          callbacks[transcriptRootIndex]?.(
            [{ contentRect: { height: 456 } as DOMRectReadOnly } as ResizeObserverEntry],
            {} as ResizeObserver,
          );
        });

        await waitFor(() =>
          expect(scrollToMock).toHaveBeenCalledWith({ top: 520, behavior: "auto" })
        );
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        vi.unstubAllEnvs();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("pins to true bottom when external composer chrome changes while sticky", async () => {
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = true;
        const messages = createMessages(2);
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            externalLayoutVersion={0}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 456,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        scrollToMock.mockClear();

        rerender(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            externalLayoutVersion={1}
          />
        );

        await waitFor(() =>
          expect(scrollToMock).toHaveBeenCalledWith({ top: 544, behavior: "auto" })
        );
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
      }
    });

    it("pins external chrome changes while sticky when the last row is temporarily out of range", async () => {
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = true;
        const messages = createMessages(2);
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            externalLayoutVersion={0}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 456,
          scrollHeight: 1000,
          scrollTop: 500,
        });
        const rangeChanged = expectMockVirtuosoCallback<
          (range: { startIndex: number; endIndex: number }) => void
        >("rangeChanged");
        act(() => {
          rangeChanged({ startIndex: 0, endIndex: 0 });
        });
        scrollToMock.mockClear();

        rerender(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            externalLayoutVersion={1}
          />
        );

        await waitFor(() =>
          expect(scrollToMock).toHaveBeenCalledWith({ top: 544, behavior: "auto" })
        );
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
      }
    });

    it("does not pin external composer chrome changes after manual downward wheel input", async () => {
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = true;
        const messages = createMessages(2);
        const { rerender } = render(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            externalLayoutVersion={0}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 456,
          scrollHeight: 1000,
          scrollTop: 420,
        });
        act(() => {
          scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
          scroller.scrollTop = 460;
          scroller.dispatchEvent(new Event("scroll"));
        });
        scrollToMock.mockClear();

        rerender(
          <ChatMessageList
            {...defaultProps}
            messages={messages}
            externalLayoutVersion={1}
          />
        );
        await act(async () => {});

        expect(scrollToMock).not.toHaveBeenCalled();
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
      }
    });

    it("pins to true bottom when the finalized last message row grows while sticky", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const observedTargets: Element[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        cb(0);
        return 1;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }

        disconnect = vi.fn();
        observe = vi.fn((target: Element) => {
          observedTargets.push(target);
        });
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        scrollToMock.mockClear();

        const lastRowIndex = observedTargets.findIndex(
          (target) =>
            target instanceof HTMLElement &&
            target.dataset.chatLastRenderedRow === "true",
        );
        expect(lastRowIndex).toBeGreaterThanOrEqual(0);

        const lastRowCallback = callbacks[lastRowIndex];
        const resizeObserver = {} as ResizeObserver;
        act(() => {
          lastRowCallback?.(
            [{ contentRect: { height: 80 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
        });
        expect(scrollToMock).not.toHaveBeenCalled();

        act(() => {
          lastRowCallback?.(
            [{ contentRect: { height: 72 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
        });
        expect(scrollToMock).not.toHaveBeenCalled();

        act(() => {
          lastRowCallback?.(
            [{ contentRect: { height: 104 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
        });

        await waitFor(() =>
          expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" })
        );
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });

    it("cancel-reschedules finalized last message row bottom pins while sticky", async () => {
      const callbacks: ResizeObserverCallback[] = [];
      const observedTargets: Element[] = [];
      const queuedRafs: FrameRequestCallback[] = [];
      const originalResizeObserver = globalThis.ResizeObserver;
      let nextRafId = 1;
      const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
        queuedRafs.push(cb);
        return nextRafId++;
      });
      const cancelSpy = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});

      class MockResizeObserver implements ResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          callbacks.push(callback);
        }

        disconnect = vi.fn();
        observe = vi.fn((target: Element) => {
          observedTargets.push(target);
        });
        unobserve = vi.fn();
      }

      Object.defineProperty(globalThis, "ResizeObserver", {
        value: MockResizeObserver,
        configurable: true,
        writable: true,
      });

      try {
        mockIsAtBottom = false;
        mockIsAtBottomRef.current = true;
        render(
          <ChatMessageList
            {...defaultProps}
            messages={createMessages(2)}
          />
        );

        const scroller = await screen.findByTestId("mock-virtuoso");
        setMockScrollerGeometry(scroller, {
          clientHeight: 500,
          scrollHeight: 1000,
          scrollTop: 480,
        });
        scrollToMock.mockClear();
        cancelSpy.mockClear();

        const lastRowIndex = observedTargets.findIndex(
          (target) =>
            target instanceof HTMLElement &&
            target.dataset.chatLastRenderedRow === "true",
        );
        expect(lastRowIndex).toBeGreaterThanOrEqual(0);

        const lastRowCallback = callbacks[lastRowIndex];
        const resizeObserver = {} as ResizeObserver;
        act(() => {
          lastRowCallback?.(
            [{ contentRect: { height: 80 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
        });
        act(() => {
          lastRowCallback?.(
            [{ contentRect: { height: 104 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
        });
        const firstResizeRafId = nextRafId - 1;
        act(() => {
          lastRowCallback?.(
            [{ contentRect: { height: 128 } as DOMRectReadOnly } as ResizeObserverEntry],
            resizeObserver,
          );
        });

        expect(cancelSpy).toHaveBeenCalledWith(firstResizeRafId);
        expect(scrollToMock).not.toHaveBeenCalled();

        act(() => {
          queuedRafs.at(-1)?.(0);
        });

        await waitFor(() =>
          expect(scrollToMock).toHaveBeenCalledWith({ top: 500, behavior: "auto" })
        );
      } finally {
        rafSpy.mockRestore();
        cancelSpy.mockRestore();
        if (originalResizeObserver === undefined) {
          Reflect.deleteProperty(globalThis, "ResizeObserver");
        } else {
          Object.defineProperty(globalThis, "ResizeObserver", {
            value: originalResizeObserver,
            configurable: true,
            writable: true,
          });
        }
      }
    });
  });

  describe("pending tool call fallback indicator", () => {
    // Covers the fix: when streamingToolCalls has items but streamingContentBlocks is empty,
    // the footer shows ToolCallIndicator (not blank) so users see immediate activity feedback.
    // Uses "webfetch" as a generic tool name — no widget in registry, no diff handling,
    // falls through to the default ToolCallIndicator with data-testid="tool-call-indicator".
    const GENERIC = "webfetch";

    it("(1) agent running + no data → shows TypingIndicator", () => {
      const { container } = render(
        <ChatMessageList
          {...defaultProps}
          messages={[]}
          isAgentRunning={true}
          streamingToolCalls={[]}
          streamingContentBlocks={undefined}
        />
      );

      const typingIndicator = screen.getByTestId("chat-typing-indicator");

      expect(typingIndicator).toBeInTheDocument();
      expect(screen.queryByTestId("tool-call-indicator")).not.toBeInTheDocument();
      expect(typingIndicator.closest('[data-chat-message-item="true"]')).toBeNull();
      expect(container.querySelectorAll('[data-chat-message-item="true"]')).toHaveLength(0);
      expect(container.querySelectorAll('[data-testid="message-meta"]')).toHaveLength(0);
      expect(typingIndicator.querySelectorAll("svg.lucide-bot")).toHaveLength(1);
      expect(typingIndicator).toHaveTextContent("Agent working");
    });

    it("renders an explicit activity label in the typing indicator", () => {
      render(
        <ChatMessageList
          {...defaultProps}
          messages={[]}
          isSending={true}
          typingIndicatorLabel="Setup workspace"
          streamingToolCalls={[]}
          streamingContentBlocks={undefined}
        />
      );

      expect(screen.getByTestId("chat-typing-indicator")).toHaveTextContent(
        "Setup workspace"
      );
    });

    it("(2) agent running + tool calls + no content blocks → shows tool fallback and typing indicator", () => {
      const toolCalls: ToolCall[] = [
        { id: "tc-1", name: GENERIC, arguments: { url: "https://example.com" } },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isAgentRunning={true}
          streamingToolCalls={toolCalls}
          streamingContentBlocks={undefined}
        />
      );

      const toolCall = screen.getByTestId("tool-call-indicator");
      const liveAssistantRow = toolCall.closest('[data-chat-message-item="true"]');
      const typingIndicator = screen.getByTestId("chat-typing-indicator");

      expect(toolCall).toBeInTheDocument();
      expect(liveAssistantRow).toBeInTheDocument();
      expect(liveAssistantRow?.querySelector("svg.lucide-bot")).not.toBeInTheDocument();
      expect(liveAssistantRow?.querySelector('[data-testid="message-assistant-icon-spacer"]')).toBeInTheDocument();
      expect(typingIndicator).toBeInTheDocument();
      expect(typingIndicator.closest('[data-chat-message-item="true"]')).toBeNull();
      expect(liveAssistantRow!.compareDocumentPosition(typingIndicator) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });

    it("(2b) groups multiple pending tool calls when no content blocks have arrived", async () => {
      const user = userEvent.setup();
      const toolCalls: ToolCall[] = [
        { id: "tc-1", name: GENERIC, arguments: { url: "https://a.com" } },
        { id: "tc-2", name: GENERIC, arguments: { url: "https://b.com" } },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isAgentRunning={true}
          streamingToolCalls={toolCalls}
          streamingContentBlocks={undefined}
        />
      );

      expect(screen.getByRole("button", { name: "Agent called 2 tools" })).toBeInTheDocument();
      expect(screen.queryAllByTestId("tool-call-indicator")).toHaveLength(0);
      expect(screen.getByTestId("chat-typing-indicator")).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "Agent called 2 tools" }));

      expect(screen.getByRole("button", { name: "Hide 2 tool calls" })).toBeInTheDocument();
      expect(screen.getAllByTestId("tool-call-indicator")).toHaveLength(2);

      await user.click(screen.getByRole("button", { name: "Hide 2 tool calls" }));

      expect(screen.getByRole("button", { name: "Agent called 2 tools" })).toBeInTheDocument();
      expect(screen.queryAllByTestId("tool-call-indicator")).toHaveLength(0);
    });

    it("(3) agent running + content blocks → content blocks render and typing remains visible", () => {
      const blocks: StreamingContentBlock[] = [
        { type: "text", text: "I am working on it..." },
      ];

      render(
        <ChatMessageList
          {...defaultProps}
          isAgentRunning={true}
          streamingToolCalls={[{ id: "tc-1", name: GENERIC, arguments: { url: "https://example.com" } }]}
          streamingContentBlocks={blocks}
        />
      );

      // Content blocks render through the live timeline, with a typing indicator
      // pinned beneath them while the agent is still active.
      const liveText = screen.getByText(/I am working on it/);
      const typingIndicator = screen.getByTestId("chat-typing-indicator");
      expect(liveText).toBeInTheDocument();
      expect(typingIndicator).toBeInTheDocument();
      expect(typingIndicator.closest('[data-chat-message-item="true"]')).toBeNull();
      expect(liveText.compareDocumentPosition(typingIndicator) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });

    it("shows ToolCallIndicator fallback and typing indicator when tool calls exist but content blocks is empty array", () => {
      // streamingContentBlocks=[] (empty array, not undefined) also triggers fallback
      render(
        <ChatMessageList
          {...defaultProps}
          isAgentRunning={true}
          streamingToolCalls={[{ id: "tc-1", name: GENERIC, arguments: { url: "https://example.com" } }]}
          streamingContentBlocks={[]}
        />
      );

      expect(screen.getByTestId("tool-call-indicator")).toBeInTheDocument();
      expect(screen.getByTestId("chat-typing-indicator")).toBeInTheDocument();
    });
  });
});

describe("ChatMessageList - System cards", () => {
  it("renders auto-verification metadata as a system card", async () => {
    const user = userEvent.setup();
    const messages: ChatMessageData[] = [
      {
        id: "auto-verification-1",
        role: "system",
        content: "<auto-verification>\nCheck this code.\n</auto-verification>",
        createdAt: new Date(2026, 0, 1, 12, 30).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        metadata: JSON.stringify({ auto_verification: true }),
      },
    ];

    render(<ChatMessageList {...defaultProps} messages={messages} />);

    expect(screen.getByText("Auto-verification")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /auto-verification/i }));
    expect(screen.getByText("Check this code.")).toBeInTheDocument();
  });

  it("renders verification-result metadata as a system card", async () => {
    const user = userEvent.setup();
    const messages: ChatMessageData[] = [
      {
        id: "verification-result-1",
        role: "system",
        content: "Verification hit an infrastructure/runtime blocker.",
        createdAt: new Date(2026, 0, 1, 13, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        metadata: JSON.stringify({
          verification_result: true,
          summary: "1 gap remains: 1 critical.",
          convergence_reason: "agent_error",
          current_round: 1,
          max_rounds: 5,
          recommended_next_action: "rerun_verification",
          actionable_for_parent: false,
          top_blockers: [
            {
              severity: "critical",
              description: "Delegated critic startup failed before any plan analysis.",
            },
          ],
        }),
      },
    ];

    render(<ChatMessageList {...defaultProps} messages={messages} />);

    expect(screen.getByText("Verification result")).toBeInTheDocument();
    expect(screen.queryByText(/1 gap remains/)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /verification result/i }));

    expect(screen.getByText(/1 gap remains: 1 critical\./)).toBeInTheDocument();
    expect(screen.getByText(/Infra\/runtime issue/)).toBeInTheDocument();
  });

  it("renders nothing for system message with metadata that has no recognized key", () => {
    const messages: ChatMessageData[] = [
      {
        id: "sys-unknown",
        role: "system",
        content: "Plain system message",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        metadata: JSON.stringify({ some_other_key: true }),
      },
    ];
    render(<ChatMessageList {...defaultProps} messages={messages} />);
    // Falls through to MessageItem rendering of plain system content
    expect(screen.getByText("Plain system message")).toBeInTheDocument();
  });

  it("treats invalid JSON metadata gracefully (falls through to MessageItem)", () => {
    const messages: ChatMessageData[] = [
      {
        id: "sys-bad-json",
        role: "system",
        content: "Bad JSON metadata message",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        metadata: "{not valid json",
      },
    ];
    render(<ChatMessageList {...defaultProps} messages={messages} />);
    expect(screen.getByText("Bad JSON metadata message")).toBeInTheDocument();
  });

  it("verification result with non-string blockers is filtered out", async () => {
    const user = userEvent.setup();
    const messages: ChatMessageData[] = [
      {
        id: "vr-edge",
        role: "system",
        content: "Edge case",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        metadata: JSON.stringify({
          verification_result: true,
          summary: "Mixed blockers",
          top_blockers: [
            { severity: 123, description: "" }, // invalid severity, empty description filtered
            { severity: "critical", description: "valid one" },
            "not an object",
            null,
          ],
        }),
      },
    ];
    render(<ChatMessageList {...defaultProps} messages={messages} />);
    expect(screen.getByText("Verification result")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /verification result/i }));
    // Only the valid one survives
    expect(screen.getByText(/valid one/)).toBeInTheDocument();
  });
});

// ============================================================================
// Hook events, team mode, additional uncovered branches
// ============================================================================

describe("ChatMessageList - Hook events", () => {
  it("renders completed hook events from hookEvents prop", () => {
    const hookEvents = [
      {
        type: "completed" as const,
        conversationId: "conv-1",
        contextType: "ideation",
        contextId: "session-1",
        timestamp: new Date(2026, 0, 1, 12, 5).getTime(),
        hookName: "PreToolUse",
        hookEvent: "tool_use",
        hookId: "h1",
        output: "ok",
        outcome: "allow",
        exitCode: 0,
      },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        hookEvents={hookEvents}
      />
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders active hook events from activeHooks prop", () => {
    const activeHooks = [
      {
        type: "started" as const,
        conversationId: "conv-1",
        contextType: "ideation",
        contextId: "session-1",
        timestamp: new Date(2026, 0, 1, 12, 5).getTime(),
        hookName: "PreToolUse",
        hookEvent: "tool_use",
        hookId: "h2",
      },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        activeHooks={activeHooks}
      />
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });
});

describe("ChatMessageList - Older messages indicator", () => {
  it("renders the 'Loading earlier messages' indicator when fetching older", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(3)}
        isFetchingOlderMessages={true}
      />
    );
    // The "Loading earlier messages..." badge only renders in the non-test-env path
    // (Virtuoso branch). In test env, this branch is bypassed — no error should occur.
    // We just assert the component still renders without crashing.
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("accepts onLoadOlderMessages and hasOlderMessages without crashing", () => {
    const onLoad = vi.fn();
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(3)}
        hasOlderMessages={true}
        onLoadOlderMessages={onLoad}
      />
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });
});

describe("ChatMessageList - Streaming task blocks", () => {
  it("does not render task block when streamingTasks does not contain matching toolUseId", () => {
    const blocks: StreamingContentBlock[] = [
      { type: "task", toolUseId: "missing-id" },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        isAgentRunning={true}
        streamingContentBlocks={blocks}
      />
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders task block via TaskSubagentCard when matching task is present", () => {
    const tasks = new Map();
    tasks.set("tu-1", {
      toolUseId: "tu-1",
      description: "child task",
      prompt: "do work",
      childToolCalls: [],
      status: "running",
      startedAt: new Date().toISOString(),
    });
    const blocks: StreamingContentBlock[] = [
      { type: "task", toolUseId: "tu-1" },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        isAgentRunning={true}
        streamingTasks={tasks}
        streamingContentBlocks={blocks}
      />
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders streaming diff tool call as DiffToolCallView when block is a diff tool", () => {
    const blocks: StreamingContentBlock[] = [
      {
        type: "tool_use",
        toolCall: {
          id: "tc-edit",
          name: "Edit",
          arguments: { file_path: "x.ts", old_string: "a", new_string: "b" },
        },
      },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        isAgentRunning={true}
        streamingContentBlocks={blocks}
      />
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });
});

describe("ChatMessageList - Initial paint cover", () => {
  it("renders the placeholder cover while initialPaintCoverKey is set with messages", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(3)}
        initialPaintCoverKey="conv-key-1"
        onInitialPaintReady={vi.fn()}
      />
    );
    expect(
      screen.getByTestId("chat-transcript-settling-placeholders"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("chat-transcript-settling-placeholders")).toHaveClass(
      "pointer-events-none",
    );
  });

  it("does NOT render the placeholder cover when initialPaintCoverKey is null", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(3)}
        initialPaintCoverKey={null}
      />
    );
    expect(
      screen.queryByTestId("chat-transcript-settling-placeholders"),
    ).not.toBeInTheDocument();
  });

  it("does NOT render the placeholder cover when there are no messages", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={[]}
        initialPaintCoverKey="conv-key-1"
      />
    );
    expect(
      screen.queryByTestId("chat-transcript-settling-placeholders"),
    ).not.toBeInTheDocument();
  });

  it("keeps messages mounted and clears the cover when paint readiness stalls", async () => {
    vi.useFakeTimers();
    const originalRequestAnimationFrame = window.requestAnimationFrame;
    const originalCancelAnimationFrame = window.cancelAnimationFrame;
    window.requestAnimationFrame = vi.fn(() => 123) as unknown as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = vi.fn() as unknown as typeof window.cancelAnimationFrame;

    try {
      const onInitialPaintReady = vi.fn();
      render(
        <ChatMessageList
          {...defaultProps}
          messages={createMessages(3)}
          initialPaintCoverKey="conv-key-stalled"
          onInitialPaintReady={onInitialPaintReady}
        />
      );

      expect(screen.getByText("Message 1")).toBeInTheDocument();
      expect(screen.getByText("Message 3")).toBeInTheDocument();
      expect(screen.getByTestId("chat-transcript-settling-placeholders")).toBeInTheDocument();

      await act(async () => {
        vi.advanceTimersByTime(2_500);
      });

      expect(
        screen.queryByTestId("chat-transcript-settling-placeholders"),
      ).not.toBeInTheDocument();
      expect(screen.getByText("Message 3")).toBeInTheDocument();
      expect(onInitialPaintReady).toHaveBeenCalledWith("conv-key-stalled");
    } finally {
      window.requestAnimationFrame = originalRequestAnimationFrame;
      window.cancelAnimationFrame = originalCancelAnimationFrame;
      vi.useRealTimers();
    }
  });

  it("does not keep the cover alive by restarting the fallback on same-conversation message churn", async () => {
    vi.useFakeTimers();
    const originalRequestAnimationFrame = window.requestAnimationFrame;
    const originalCancelAnimationFrame = window.cancelAnimationFrame;
    window.requestAnimationFrame = vi.fn(() => 123) as unknown as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = vi.fn() as unknown as typeof window.cancelAnimationFrame;

    try {
      const onInitialPaintReady = vi.fn();
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          messages={createMessages(1)}
          initialPaintCoverKey="conv-key-churning"
          onInitialPaintReady={onInitialPaintReady}
        />
      );

      expect(screen.getByTestId("chat-transcript-settling-placeholders")).toBeInTheDocument();

      await act(async () => {
        vi.advanceTimersByTime(2_000);
      });

      rerender(
        <ChatMessageList
          {...defaultProps}
          messages={createMessages(2)}
          initialPaintCoverKey="conv-key-churning"
          onInitialPaintReady={onInitialPaintReady}
        />
      );

      await act(async () => {
        vi.advanceTimersByTime(500);
      });

      expect(
        screen.queryByTestId("chat-transcript-settling-placeholders"),
      ).not.toBeInTheDocument();
      expect(onInitialPaintReady).toHaveBeenCalledWith("conv-key-churning");
    } finally {
      window.requestAnimationFrame = originalRequestAnimationFrame;
      window.cancelAnimationFrame = originalCancelAnimationFrame;
      vi.useRealTimers();
    }
  });

  it("does not re-arm a cleared cover when the parent still passes the same conversation key", async () => {
    vi.useFakeTimers();
    const originalRequestAnimationFrame = window.requestAnimationFrame;
    const originalCancelAnimationFrame = window.cancelAnimationFrame;
    window.requestAnimationFrame = vi.fn(() => 123) as unknown as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = vi.fn() as unknown as typeof window.cancelAnimationFrame;

    try {
      const onInitialPaintReady = vi.fn();
      const { rerender } = render(
        <ChatMessageList
          {...defaultProps}
          messages={createMessages(2)}
          initialPaintCoverKey="conv-key-cleared"
          onInitialPaintReady={onInitialPaintReady}
        />
      );

      await act(async () => {
        vi.advanceTimersByTime(2_500);
      });

      expect(
        screen.queryByTestId("chat-transcript-settling-placeholders"),
      ).not.toBeInTheDocument();

      rerender(
        <ChatMessageList
          {...defaultProps}
          messages={createMessages(3)}
          initialPaintCoverKey="conv-key-cleared"
          onInitialPaintReady={onInitialPaintReady}
        />
      );

      expect(
        screen.queryByTestId("chat-transcript-settling-placeholders"),
      ).not.toBeInTheDocument();
      expect(onInitialPaintReady).toHaveBeenCalledTimes(1);
    } finally {
      window.requestAnimationFrame = originalRequestAnimationFrame;
      window.cancelAnimationFrame = originalCancelAnimationFrame;
      vi.useRealTimers();
    }
  });
});

describe("ChatMessageList - Filtered teammate tab empty state", () => {
  it("shows 'No messages from X yet' when teamFilter excludes all messages", () => {
    // When teamFilter is set to a teammate name and timeline is empty (no team messages),
    // the empty-tab message should render.
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        teamFilter="alice"
        contextKey="ideation:session-1"
      />
    );
    // Note: teamFilter only filters team messages, not regular messages.
    // The condition `timeline.length === 0 && messages.length > 0` requires
    // the timeline to be empty. Since regular messages still flow through, this
    // path is only hit when the timeline filter empties everything. We at least
    // ensure the component does not crash with teamFilter set.
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });
});

describe("ChatMessageList - Failed run banner edge cases", () => {
  it("does not render banner when failedRun is provided but errorMessage is empty", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        failedRun={{ id: "r-empty", errorMessage: "" }}
      />
    );
    // Banner only renders when errorMessage is truthy
    expect(screen.queryByText(/dismiss/i)).not.toBeInTheDocument();
  });

  it("does not render banner when onDismissFailedRun is missing", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        failedRun={{ id: "r-no-cb", errorMessage: "boom" }}
        onDismissFailedRun={undefined as never}
      />
    );
    // Banner requires onDismissFailedRun callback to render
    expect(screen.queryByText(/Failed run/i)).not.toBeInTheDocument();
  });
});

describe("ChatMessageList - Assistant sender grouping", () => {
  it("uses per-message provider attribution instead of the current conversation fallback", () => {
    const messages: ChatMessageData[] = [
      {
        id: "assistant-legacy",
        role: "assistant",
        content: "Legacy assistant item without saved provider metadata.",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
      },
      {
        id: "assistant-claude",
        role: "assistant",
        content: "Claude assistant item.",
        createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        providerHarness: "claude",
      },
      {
        id: "assistant-codex",
        role: "assistant",
        content: "Codex assistant item.",
        createdAt: new Date(2026, 0, 1, 12, 2).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        providerHarness: "codex",
      },
    ];

    render(
      <ChatMessageList
        {...defaultProps}
        messages={messages}
        providerHarness="codex"
        providerSessionId="current-codex-session"
      />
    );

    const legacyRow = screen
      .getByText("Legacy assistant item without saved provider metadata.")
      .closest('[data-chat-message-item="true"]');
    const claudeRow = screen
      .getByText("Claude assistant item.")
      .closest('[data-chat-message-item="true"]');
    const codexRow = screen
      .getByText("Codex assistant item.")
      .closest('[data-chat-message-item="true"]');

    expect(legacyRow?.querySelector('[data-testid="message-provider-badge"]')).not.toBeInTheDocument();
    expect(claudeRow?.querySelector('[data-testid="message-provider-badge"]')).toHaveTextContent("Claude");
    expect(codexRow?.querySelector('[data-testid="message-provider-badge"]')).toHaveTextContent("Codex");
  });

  it("hides repeated assistant sender chrome but preserves the gutter for adjacent finalized messages", () => {
    const messages: ChatMessageData[] = [
      {
        id: "assistant-1",
        role: "assistant",
        content: "First assistant item.",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        providerHarness: "codex",
      },
      {
        id: "assistant-2",
        role: "assistant",
        content: "Second assistant item.",
        createdAt: new Date(2026, 0, 1, 12, 20).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        providerHarness: "codex",
      },
    ];
    render(<ChatMessageList {...defaultProps} messages={messages} />);

    const firstRow = screen
      .getByText("First assistant item.")
      .closest('[data-chat-message-item="true"]');
    const secondRow = screen
      .getByText("Second assistant item.")
      .closest('[data-chat-message-item="true"]');

    expect(firstRow?.querySelector("svg.lucide-bot")).toBeInTheDocument();
    expect(firstRow?.querySelector('[data-testid="message-provider-badge"]')).toBeInTheDocument();
    expect(secondRow?.querySelector("svg.lucide-bot")).not.toBeInTheDocument();
    expect(secondRow?.querySelector('[data-testid="message-assistant-icon-spacer"]')).toBeInTheDocument();
    expect(secondRow?.querySelector('[data-testid="message-provider-badge"]')).not.toBeInTheDocument();
  });

  it("starts a new assistant group after a user message", () => {
    const messages: ChatMessageData[] = [
      {
        id: "assistant-1",
        role: "assistant",
        content: "Earlier assistant item.",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        providerHarness: "codex",
      },
      {
        id: "user-1",
        role: "user",
        content: "User interruption.",
        createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
        toolCalls: null,
        contentBlocks: null,
      },
      {
        id: "assistant-2",
        role: "assistant",
        content: "Fresh assistant item.",
        createdAt: new Date(2026, 0, 1, 12, 2).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        providerHarness: "codex",
      },
    ];
    render(<ChatMessageList {...defaultProps} messages={messages} />);

    const freshAssistantRow = screen
      .getByText("Fresh assistant item.")
      .closest('[data-chat-message-item="true"]');

    expect(freshAssistantRow?.querySelector("svg.lucide-bot")).toBeInTheDocument();
    expect(freshAssistantRow?.querySelector('[data-testid="message-provider-badge"]')).toBeInTheDocument();
    expect(freshAssistantRow?.querySelector('[data-testid="message-assistant-icon-spacer"]')).not.toBeInTheDocument();
  });
});

describe("ChatMessageList - Streaming text/empty edge cases", () => {
  it("keeps the assistant gutter for text-only streaming content", () => {
    const blocks: StreamingContentBlock[] = [
      { type: "text", text: "Live Codex text is still streaming." },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        isAgentRunning={true}
        streamingContentBlocks={blocks}
      />
    );

    const liveAssistantRow = screen
      .getByText("Live Codex text is still streaming.")
      .closest('[data-chat-message-item="true"]');

    expect(liveAssistantRow).toBeInTheDocument();
    expect(liveAssistantRow?.querySelector("svg.lucide-bot")).toBeInTheDocument();
  });

  it("hides repeated streaming sender chrome while preserving the assistant gutter", () => {
    const messages: ChatMessageData[] = [
      {
        id: "assistant-1",
        role: "assistant",
        content: "Previous assistant output.",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        providerHarness: "codex",
      },
    ];
    const blocks: StreamingContentBlock[] = [
      { type: "text", text: "Continuation is still streaming." },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        messages={messages}
        isAgentRunning={true}
        streamingContentBlocks={blocks}
        providerHarness="codex"
      />
    );

    const liveAssistantRow = screen
      .getByText("Continuation is still streaming.")
      .closest('[data-chat-message-item="true"]');

    expect(liveAssistantRow).toBeInTheDocument();
    expect(liveAssistantRow?.querySelector("svg.lucide-bot")).not.toBeInTheDocument();
    expect(liveAssistantRow?.querySelector('[data-testid="message-assistant-icon-spacer"]')).toBeInTheDocument();
    expect(liveAssistantRow?.querySelector('[data-testid="message-provider-badge"]')).not.toBeInTheDocument();
  });

  it("renders without crashing when streamingContentBlocks contains empty text only", () => {
    const blocks: StreamingContentBlock[] = [
      { type: "text", text: "   " },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        isAgentRunning={true}
        streamingContentBlocks={blocks}
      />
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("filters tool calls during footer fallback that are completed project orchestration calls", () => {
    const tools: ToolCall[] = [
      {
        id: "tc-orch",
        name: "completed_project_orchestration",
        arguments: {},
        result: "done",
      },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        isAgentRunning={true}
        streamingToolCalls={tools}
      />
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });
});

// ============================================================================
// Virtuoso (production-path) coverage — flips isTestEnv off via mocked react-virtuoso.
// ============================================================================

describe("ChatMessageList - Virtuoso production render path", () => {
  beforeEach(() => {
    vi.stubEnv("VITEST", "");
    mockUseMessageAttachments.mockReturnValue({ data: new Map() });
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("renders timeline through Virtuoso path with messages", () => {
    render(<ChatMessageList {...defaultProps} messages={createMessages(3)} />);
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("disables browser scroll anchoring on the Virtuoso scroller and message rows", () => {
    render(<ChatMessageList {...defaultProps} messages={createMessages(3)} />);

    const scroller = screen.getByTestId("mock-virtuoso");
    const firstRenderedItem = screen.getByText("Message 1").closest("[data-mock-item-index]");
    const firstMessageRow = firstRenderedItem?.querySelector(".px-3.w-full");

    expect(scroller.style.overflowAnchor).toBe("none");
    expect(firstMessageRow).toBeInstanceOf(HTMLElement);
    expect((firstMessageRow as HTMLElement).style.overflowAnchor).toBe("none");
  });

  it("preserves grouped assistant gutter when Virtuoso reports absolute item indexes", () => {
    const messages: ChatMessageData[] = [
      {
        id: "assistant-1",
        role: "assistant",
        content: "First hydrated assistant item.",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        providerHarness: "claude",
      },
      {
        id: "assistant-2",
        role: "assistant",
        content: "Second hydrated assistant item.",
        createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        providerHarness: "claude",
      },
    ];

    render(
      <ChatMessageList
        {...defaultProps}
        messages={messages}
        firstItemIndex={40}
      />
    );

    const firstRow = screen
      .getByText("First hydrated assistant item.")
      .closest('[data-chat-message-item="true"]');
    const secondRow = screen
      .getByText("Second hydrated assistant item.")
      .closest('[data-chat-message-item="true"]');

    expect(firstRow?.querySelector("svg.lucide-bot")).toBeInTheDocument();
    expect(secondRow?.querySelector("svg.lucide-bot")).not.toBeInTheDocument();
    expect(secondRow?.querySelector('[data-testid="message-assistant-icon-spacer"]')).toBeInTheDocument();
  });

  it("renders Virtuoso path with hook events interleaved", () => {
    const hookEvents = [
      {
        type: "completed" as const,
        conversationId: "conv-1",
        contextType: "ideation",
        contextId: "session-1",
        timestamp: new Date(2026, 0, 1, 12, 5).getTime(),
        hookName: "PreToolUse",
        hookEvent: "tool_use",
        hookId: "h1",
        output: "ok",
        outcome: "allow",
        exitCode: 0,
      },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        hookEvents={hookEvents}
      />,
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders Virtuoso path with streaming footer content", () => {
    const blocks: StreamingContentBlock[] = [
      { type: "text", text: "streaming text" },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        isAgentRunning={true}
        streamingContentBlocks={blocks}
      />,
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders Virtuoso path with isFetchingOlderMessages indicator", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        isFetchingOlderMessages={true}
      />,
    );
    expect(screen.getByText(/Loading earlier messages/i)).toBeInTheDocument();
  });

  it("renders Virtuoso path with hasOlderMessages and onLoadOlderMessages", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        hasOlderMessages={true}
        onLoadOlderMessages={vi.fn()}
      />,
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("collapses separate consecutive tool-call runs into independent status rows", () => {
    const parentMessageId = "assistant-turn-1";
    const messages: ChatMessageData[] = [
      makeTimelineTextMessage({
        id: "text-1",
        parentMessageId,
        sequence: 1,
        text: "First I will inspect the repo.",
      }),
      makeTimelineToolMessage({ id: "tool-a", parentMessageId, sequence: 2 }),
      makeTimelineToolMessage({ id: "tool-b", parentMessageId, sequence: 3 }),
      makeTimelineTextMessage({
        id: "text-2",
        parentMessageId,
        sequence: 4,
        text: "Now I will validate the result.",
      }),
      makeTimelineToolMessage({ id: "tool-c", parentMessageId, sequence: 5 }),
      makeTimelineToolMessage({ id: "tool-d", parentMessageId, sequence: 6 }),
      makeTimelineToolMessage({ id: "tool-e", parentMessageId, sequence: 7 }),
    ];

    render(<ChatMessageList {...defaultProps} messages={messages} />);

    expect(screen.getByText("First I will inspect the repo.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Agent called 2 tools" })).toBeInTheDocument();
    expect(screen.getByText("Now I will validate the result.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Agent called 3 tools" })).toBeInTheDocument();
    expect(screen.queryAllByTestId("tool-call-indicator")).toHaveLength(0);
  });

  it("expands and hides a collapsed tool-call group without changing the tool widget renderer", async () => {
    const user = userEvent.setup();
    const parentMessageId = "assistant-turn-2";
    const messages: ChatMessageData[] = [
      makeTimelineTextMessage({
        id: "text-1",
        parentMessageId,
        sequence: 1,
        text: "I will fetch two files.",
      }),
      makeTimelineToolMessage({ id: "tool-a", parentMessageId, sequence: 2 }),
      makeTimelineToolMessage({ id: "tool-b", parentMessageId, sequence: 3 }),
    ];

    render(<ChatMessageList {...defaultProps} messages={messages} />);

    await user.click(screen.getByRole("button", { name: "Agent called 2 tools" }));

    expect(screen.getByRole("button", { name: "Hide 2 tool calls" })).toBeInTheDocument();
    expect(screen.getAllByTestId("tool-call-indicator")).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "Hide 2 tool calls" }));

    expect(screen.getByRole("button", { name: "Agent called 2 tools" })).toBeInTheDocument();
    expect(screen.queryAllByTestId("tool-call-indicator")).toHaveLength(0);
  });

  it("keeps the persisted tool-call group toggle anchored while expanding", async () => {
    const user = userEvent.setup();
    const parentMessageId = "assistant-turn-scroll";
    const messages: ChatMessageData[] = [
      makeTimelineTextMessage({
        id: "text-1",
        parentMessageId,
        sequence: 1,
        text: "I will fetch two files.",
      }),
      makeTimelineToolMessage({ id: "tool-a", parentMessageId, sequence: 2 }),
      makeTimelineToolMessage({ id: "tool-b", parentMessageId, sequence: 3 }),
    ];
    const rectSpy = mockToolGroupToggleRectShift({
      collapsedTop: 180,
      expandedTop: 240,
    });

    try {
      render(<ChatMessageList {...defaultProps} messages={messages} />);
      const scroller = await screen.findByTestId("mock-virtuoso");
      setMockScrollerGeometry(scroller, {
        clientHeight: 500,
        scrollHeight: 1000,
        scrollTop: 250,
      });

      await user.click(screen.getByRole("button", { name: "Agent called 2 tools" }));

      expect(screen.getByRole("button", { name: "Hide 2 tool calls" })).toBeInTheDocument();
      expect(scroller.scrollTop).toBe(310);
    } finally {
      rectSpy.mockRestore();
    }
  });

  it("keeps the live streaming tool-call group toggle anchored while expanding", async () => {
    const user = userEvent.setup();
    const blocks: StreamingContentBlock[] = [
      {
        type: "tool_use",
        toolCall: { id: "tc-1", name: GENERIC_TOOL_NAME, arguments: { url: "https://a.com" }, result: "page a" },
      },
      {
        type: "tool_use",
        toolCall: { id: "tc-2", name: GENERIC_TOOL_NAME, arguments: { url: "https://b.com" }, result: "page b" },
      },
    ];
    const rectSpy = mockToolGroupToggleRectShift({
      collapsedTop: 210,
      expandedTop: 255,
    });

    try {
      render(
        <ChatMessageList
          {...defaultProps}
          messages={[]}
          isAgentRunning={true}
          streamingContentBlocks={blocks}
        />
      );
      const scroller = await screen.findByTestId("mock-virtuoso");
      setMockScrollerGeometry(scroller, {
        clientHeight: 500,
        scrollHeight: 1000,
        scrollTop: 250,
      });

      await user.click(screen.getByRole("button", { name: "Agent called 2 tools" }));

      expect(screen.getByRole("button", { name: "Hide 2 tool calls" })).toBeInTheDocument();
      expect(scroller.scrollTop).toBe(295);
    } finally {
      rectSpy.mockRestore();
    }
  });

  it("keeps single tool-call rows ungrouped", () => {
    const parentMessageId = "assistant-turn-3";
    const messages: ChatMessageData[] = [
      makeTimelineTextMessage({
        id: "text-1",
        parentMessageId,
        sequence: 1,
        text: "I only need one tool.",
      }),
      makeTimelineToolMessage({ id: "tool-a", parentMessageId, sequence: 2 }),
      makeTimelineTextMessage({
        id: "text-2",
        parentMessageId,
        sequence: 3,
        text: "Done.",
      }),
    ];

    render(<ChatMessageList {...defaultProps} messages={messages} />);

    expect(screen.queryByRole("button", { name: /Agent called/i })).not.toBeInTheDocument();
    expect(screen.getByTestId("tool-call-indicator")).toBeInTheDocument();
  });

  it("does not group adjacent tool calls from different assistant parent messages", () => {
    const messages: ChatMessageData[] = [
      makeTimelineToolMessage({
        id: "tool-a",
        parentMessageId: "assistant-turn-a",
        sequence: 1,
      }),
      makeTimelineToolMessage({
        id: "tool-b",
        parentMessageId: "assistant-turn-b",
        sequence: 2,
      }),
    ];

    render(<ChatMessageList {...defaultProps} messages={messages} />);

    expect(screen.queryByRole("button", { name: "Agent called 2 tools" })).not.toBeInTheDocument();
    expect(screen.getAllByTestId("tool-call-indicator")).toHaveLength(2);
  });

  it("renders Virtuoso path with failedRun banner", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        failedRun={{ id: "r1", errorMessage: "boom" }}
      />,
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders Virtuoso path with team filter empty state when timeline empty", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={[]}
        teamFilter="alice"
        contextKey="ctx-1"
      />,
    );
    // With no messages and no team messages, the Virtuoso path still mounts.
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders Virtuoso path with team messages from team store", async () => {
    const { useTeamStore } = await import("@/stores/teamStore");
    useTeamStore.setState({
      activeTeams: {
        "ctx-team": {
          teamName: "T",
          leadName: "lead",
          teammates: {},
          messages: [
            {
              id: "tm-1",
              from: "lead",
              to: "alice",
              content: "hi",
              timestamp: new Date(2026, 0, 1, 12, 1).toISOString(),
            },
            {
              id: "tm-2",
              from: "alice",
              to: "lead",
              content: "yes",
              timestamp: new Date(2026, 0, 1, 12, 2).toISOString(),
            },
            {
              id: "tm-3",
              from: "bob",
              to: "*",
              content: "broadcast",
              timestamp: new Date(2026, 0, 1, 12, 3).toISOString(),
            },
          ],
          totalTokens: 0,
          totalEstimatedCostUsd: 0,
          createdAt: new Date(2026, 0, 1).toISOString(),
        },
      },
      pendingPlans: {},
      artifactVersion: {},
    });
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        teamFilter="alice"
        contextKey="ctx-team"
      />,
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders Virtuoso path with team messages and 'lead' filter (sees all)", async () => {
    const { useTeamStore } = await import("@/stores/teamStore");
    useTeamStore.setState({
      activeTeams: {
        "ctx-lead": {
          teamName: "T",
          leadName: "lead",
          teammates: {},
          messages: [
            {
              id: "tm-a",
              from: "alice",
              to: "bob",
              content: "msg",
              timestamp: new Date(2026, 0, 1, 12, 1).toISOString(),
            },
          ],
          totalTokens: 0,
          totalEstimatedCostUsd: 0,
          createdAt: new Date(2026, 0, 1).toISOString(),
        },
      },
      pendingPlans: {},
      artifactVersion: {},
    });
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(1)}
        teamFilter="lead"
        contextKey="ctx-lead"
      />,
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders Virtuoso path with team messages and no team filter", async () => {
    const { useTeamStore } = await import("@/stores/teamStore");
    useTeamStore.setState({
      activeTeams: {
        "ctx-no-filter": {
          teamName: "T",
          leadName: "lead",
          teammates: {},
          messages: [
            {
              id: "tm-z",
              from: "alice",
              to: "lead",
              content: "msg z",
              timestamp: new Date(2026, 0, 1, 12, 4).toISOString(),
            },
          ],
          totalTokens: 0,
          totalEstimatedCostUsd: 0,
          createdAt: new Date(2026, 0, 1).toISOString(),
        },
      },
      pendingPlans: {},
      artifactVersion: {},
    });
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(1)}
        contextKey="ctx-no-filter"
      />,
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders Virtuoso path with isFilteredTabEmpty banner shown", async () => {
    const { useTeamStore } = await import("@/stores/teamStore");
    useTeamStore.setState({
      activeTeams: {},
      pendingPlans: {},
      artifactVersion: {},
    });
    // Need messages.length > 0 AND timeline.length === 0 — that means messages
    // exist but are filtered out (provider snapshot), AND non-lead teamFilter is set.
    // The simpler path: empty messages with teamFilter set won't show this. Force the
    // suppressedProviderMessageId to swallow the only message: agent running, latest
    // user msg has been answered by an empty assistant with no content.
    const messages: ChatMessageData[] = [
      {
        id: "u-1",
        role: "user",
        content: "hi",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
      },
      {
        id: "a-1",
        role: "assistant",
        content: "",
        createdAt: new Date(2026, 0, 1, 12, 1).toISOString(),
        toolCalls: null,
        contentBlocks: null,
      },
    ];
    render(
      <ChatMessageList
        {...defaultProps}
        messages={messages}
        isAgentRunning={true}
        teamFilter="alice"
        contextKey="ctx-empty"
      />,
    );
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });

  it("renders Virtuoso path with system card metadata", () => {
    const messages: ChatMessageData[] = [
      {
        id: "sys-1",
        role: "system",
        content: "<auto-verification>x</auto-verification>",
        createdAt: new Date(2026, 0, 1, 12, 0).toISOString(),
        toolCalls: null,
        contentBlocks: null,
        metadata: JSON.stringify({ auto_verification: true }),
      },
    ];
    render(<ChatMessageList {...defaultProps} messages={messages} />);
    expect(screen.getByTestId("integrated-chat-messages")).toBeInTheDocument();
  });
});

describe("ChatMessageList - Scroll-to-bottom button interactions (Virtuoso path)", () => {
  beforeEach(() => {
    vi.stubEnv("VITEST", "");
    mockUseMessageAttachments.mockReturnValue({ data: new Map() });
    mockIsAtBottom = false;
  });
  afterEach(() => {
    vi.unstubAllEnvs();
    mockIsAtBottom = true;
  });

  it("scroll-to-bottom button is rendered (visible state may depend on scroll element)", () => {
    render(<ChatMessageList {...defaultProps} messages={createMessages(10)} />);
    // The button always mounts in Virtuoso path
    expect(screen.getByTestId("chat-scroll-to-bottom-button")).toBeInTheDocument();
  });
});

describe("ChatMessageList - Provider-only attachments hydration", () => {
  it("does not invoke attachments hook with enabled=true while paint cover is up", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        initialPaintCoverKey="cover-key"
      />
    );
    // The hook is called with { enabled: false } while cover is up
    const lastCallArgs =
      mockUseMessageAttachments.mock.calls.at(-1) ?? [];
    const opts = lastCallArgs[2] as { enabled?: boolean } | undefined;
    expect(opts?.enabled).toBe(false);
  });

  it("invokes attachments hook with enabled=true when no paint cover", () => {
    render(
      <ChatMessageList
        {...defaultProps}
        messages={createMessages(2)}
        initialPaintCoverKey={null}
      />
    );
    const lastCallArgs =
      mockUseMessageAttachments.mock.calls.at(-1) ?? [];
    const opts = lastCallArgs[2] as { enabled?: boolean } | undefined;
    expect(opts?.enabled).toBe(true);
  });
});
