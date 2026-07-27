import { act, fireEvent, render, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useConversationTimelineWindow } from "@/hooks/useChat";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ChatMessageList } from "./ChatMessageList";
import { createReplayConversationFixture } from "./__tests__/replayConversationFixture";
import {
  captureTranscriptSnapshot,
  expectSameTranscript,
} from "./__tests__/transcriptSnapshot";

const { timelineTransport, virtuosoHarness } = vi.hoisted(() => ({
  timelineTransport: {
    getConversationTimelinePage: vi.fn(),
  },
  virtuosoHarness: {
    firstItemIndex: 0,
    startReached: null as ((index: number) => void) | null,
    scrollToIndex: vi.fn(),
    scrollWrites: vi.fn(),
  },
}));

vi.mock("@/api/chat", async (importActual) => {
  const actual = await importActual<typeof import("@/api/chat")>();
  return { ...actual, chatApi: { ...actual.chatApi, ...timelineTransport } };
});

vi.mock("react-virtuoso", async () => {
  const React = await import("react");
  return {
    Virtuoso: React.forwardRef(function ReplayPaginationVirtuoso(
      props: {
        data?: unknown[];
        firstItemIndex?: number;
        itemContent?: (index: number, item: unknown) => React.ReactNode;
        rangeChanged?: (range: { startIndex: number; endIndex: number }) => void;
        startReached?: (index: number) => void;
        scrollerRef?: (element: HTMLElement | null) => void;
        components?: { Header?: React.ComponentType };
      },
      ref,
    ) {
      const {
        components,
        data = [],
        firstItemIndex = 0,
        itemContent,
        rangeChanged,
        scrollerRef,
        startReached,
      } = props;
      const rootRef = React.useRef<HTMLDivElement | null>(null);
      React.useImperativeHandle(ref, () => ({
        scrollToIndex: virtuosoHarness.scrollToIndex,
      }));
      React.useEffect(() => {
        const scroller = rootRef.current;
        if (scroller) {
          setScrollerGeometry(scroller, {
            clientHeight: 500,
            scrollHeight: 1_000,
            scrollTop: 480,
          });
          Object.defineProperty(scroller, "scrollTo", {
            configurable: true,
            value: (options: ScrollToOptions) => {
              virtuosoHarness.scrollWrites(options);
              if (typeof options.top === "number") scroller.scrollTop = options.top;
            },
          });
        }
        scrollerRef?.(rootRef.current);
        return () => scrollerRef?.(null);
      }, [scrollerRef]);
      React.useEffect(() => {
        virtuosoHarness.firstItemIndex = firstItemIndex;
        virtuosoHarness.startReached = startReached ?? null;
        if (data.length > 0) {
          rangeChanged?.({
            startIndex: firstItemIndex,
            endIndex: firstItemIndex + data.length - 1,
          });
        }
      }, [data.length, firstItemIndex, rangeChanged, startReached]);
      const Header = components?.Header;
      return (
        <div
          ref={rootRef}
          data-testid="pagination-virtuoso"
          data-first-item-index={firstItemIndex}
        >
          {Header ? <Header /> : null}
          {data.map((item, index) => (
            <div key={index}>
              {itemContent?.(firstItemIndex + index, item)}
            </div>
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

function setScrollerGeometry(
  element: HTMLElement,
  geometry: { clientHeight: number; scrollHeight: number; scrollTop: number },
): void {
  Object.defineProperties(element, {
    clientHeight: { configurable: true, value: geometry.clientHeight },
    scrollHeight: { configurable: true, value: geometry.scrollHeight },
    scrollTop: { configurable: true, value: geometry.scrollTop, writable: true },
  });
}

function PaginatedReplayTranscript({ pageSize }: { pageSize: number }) {
  const timeline = useConversationTimelineWindow("conversation-replay", { pageSize });
  return (
    <ChatMessageList
      messages={timeline.data?.messages ?? []}
      conversationId="conversation-replay"
      firstItemIndex={timeline.loadedStartIndex}
      failedRun={null}
      isSending={false}
      isAgentRunning={false}
      streamingToolCalls={[]}
      streamingTasks={new Map()}
      streamingContentBlocks={[]}
      hasOlderMessages={timeline.hasOlderMessages}
      isFetchingOlderMessages={timeline.isFetchingOlderMessages}
      onLoadOlderMessages={timeline.fetchOlderMessages}
    />
  );
}

function renderPaginatedTranscript(pageSize: number) {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider delayDuration={0}>
        <PaginatedReplayTranscript pageSize={pageSize} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

function expandVisibleToolGroups(container: HTMLElement): void {
  act(() => {
    within(container)
      .queryAllByTestId("tool-call-group-toggle")
      .filter((toggle) => toggle.getAttribute("aria-expanded") !== "true")
      .forEach((toggle) => fireEvent.click(toggle));
  });
}

async function loadOlderPage(): Promise<void> {
  const startReached = virtuosoHarness.startReached;
  if (!startReached) throw new Error("Expected the paginated transcript to expose startReached");
  await act(async () => {
    startReached(virtuosoHarness.firstItemIndex);
  });
}

describe("chat timeline paginated replay parity", () => {
  const fixture = createReplayConversationFixture();

  beforeEach(() => {
    vi.stubEnv("VITEST", "");
    vi.clearAllMocks();
    virtuosoHarness.firstItemIndex = 0;
    virtuosoHarness.startReached = null;
    virtuosoHarness.scrollWrites.mockReset();
    timelineTransport.getConversationTimelinePage.mockImplementation(
      async (_conversationId: string, limit: number, beforeSequence: number | null) =>
        fixture.timelinePage("turn-2-finalized", { limit, beforeSequence }),
    );
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("prepends every older page in canonical order and matches a one-page hydrate", async () => {
    const paginated = renderPaginatedTranscript(4);
    await waitFor(() => {
      expect(timelineTransport.getConversationTimelinePage).toHaveBeenCalledWith(
        "conversation-replay",
        4,
        null,
      );
      expect(paginated.container.textContent).toContain("Live turn two is ready to finalize.");
      expect(within(paginated.container).getByTestId("pagination-virtuoso"))
        .toHaveAttribute("data-first-item-index", "9");
      expect(virtuosoHarness.startReached).not.toBeNull();
    });
    expandVisibleToolGroups(paginated.container);
    let previousWindow = captureTranscriptSnapshot(paginated.container);
    const scroller = within(paginated.container).getByTestId("pagination-virtuoso");
    setScrollerGeometry(scroller, { clientHeight: 500, scrollHeight: 1_000, scrollTop: 200 });
    fireEvent.wheel(scroller, { deltaY: -80 });
    fireEvent.scroll(scroller);
    virtuosoHarness.scrollToIndex.mockClear();
    virtuosoHarness.scrollWrites.mockClear();

    for (const [pageIndex, checkpoint] of [
      { beforeSequence: 10, firstItemIndex: 5 },
      { beforeSequence: 6, firstItemIndex: 1 },
      { beforeSequence: 2, firstItemIndex: 0 },
    ].entries()) {
      await loadOlderPage();
      await waitFor(() => {
        expect(timelineTransport.getConversationTimelinePage).toHaveBeenCalledWith(
          "conversation-replay",
          4,
          checkpoint.beforeSequence,
        );
        expect(virtuosoHarness.firstItemIndex).toBe(checkpoint.firstItemIndex);
      });
      expandVisibleToolGroups(paginated.container);
      const expandedWindow = captureTranscriptSnapshot(paginated.container);
      expectSameTranscript(
        expandedWindow.slice(expandedWindow.length - previousWindow.length),
        previousWindow,
      );
      const compensatedScrollTop = 500 + pageIndex * 300;
      setScrollerGeometry(scroller, {
        clientHeight: 500,
        scrollHeight: 1_300 + pageIndex * 300,
        scrollTop: compensatedScrollTop,
      });
      fireEvent.scroll(scroller);
      expect(virtuosoHarness.scrollWrites).not.toHaveBeenCalled();
      expect(virtuosoHarness.scrollToIndex).not.toHaveBeenCalled();
      expect(scroller.scrollTop).toBe(compensatedScrollTop);
      previousWindow = expandedWindow;
    }

    expect(virtuosoHarness.startReached).toBeNull();
    const full = renderPaginatedTranscript(40);
    await waitFor(() => {
      expect(full.container.textContent).toContain("Replay the full two-turn recovery path.");
    });
    expandVisibleToolGroups(full.container);

    expectSameTranscript(previousWindow, captureTranscriptSnapshot(full.container));
  });
});
