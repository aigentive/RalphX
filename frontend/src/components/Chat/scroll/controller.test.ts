import { describe, expect, it, vi } from "vitest";

import {
  createChatScrollController,
  type ChatScrollController,
  type ChatScrollControllerDeps,
} from "./controller";

interface TestElement extends HTMLElement {
  setGeometry(values: { scrollHeight?: number; clientHeight?: number; scrollTop?: number }): void;
  setTop(top: number): void;
  readonly directWrites: number;
}

/**
 * A scroller whose trailing measurement collapses whenever a write lands on the
 * expanded true bottom, and is restored by the next virtualizer re-measure.
 * This is the bistable geometry captured in production scroll traces.
 */
interface BistableElement extends HTMLElement {
  remeasure(): void;
  setExpandedHeight(height: number): void;
  takeScrollEvent(): boolean;
  readonly directWrites: number;
}

interface TestHarness<TElement extends HTMLElement = TestElement> {
  element: TElement;
  controller: ChatScrollController;
  flushFrames(): void;
  flushNextFrame(): void;
  pendingFrames(): number;
  scrollCalls: Array<{ index: number; align: "start" | "end"; behavior: "auto" | "smooth" }>;
  pinReasons(): string[];
  states: string[];
  visualBottom: boolean[];
  debugEvents: Array<{ event: string; detail: Record<string, unknown> }>;
}

function createElement({
  scrollHeight = 1000,
  clientHeight = 500,
  scrollTop = 500,
}: {
  scrollHeight?: number;
  clientHeight?: number;
  scrollTop?: number;
} = {}): TestElement {
  const element = document.createElement("div") as TestElement;
  let top = 0;
  let writes = 0;
  let height = scrollHeight;
  let viewport = clientHeight;
  let position = scrollTop;
  Object.defineProperties(element, {
    clientHeight: { configurable: true, get: () => viewport },
    directWrites: { configurable: true, get: () => writes },
    scrollHeight: { configurable: true, get: () => height },
    scrollTop: {
      configurable: true,
      get: () => position,
      set: (next: number) => {
        writes += 1;
        position = next;
      },
    },
  });
  element.scrollTo = ({ top: nextTop }: ScrollToOptions) => {
    if (typeof nextTop === "number") element.scrollTop = nextTop;
  };
  element.getBoundingClientRect = () => new DOMRect(0, top, 100, viewport);
  element.setGeometry = (values) => {
    if (values.scrollHeight !== undefined) height = values.scrollHeight;
    if (values.clientHeight !== undefined) viewport = values.clientHeight;
    if (values.scrollTop !== undefined) position = values.scrollTop;
  };
  element.setTop = (nextTop) => {
    top = nextTop;
  };
  return element;
}

function createBistableElement({
  clientHeight = 891,
  expandedHeight = 1_416,
  collapsedGap = 20,
}: {
  clientHeight?: number;
  expandedHeight?: number;
  collapsedGap?: number;
} = {}): BistableElement {
  const element = document.createElement("div") as BistableElement;
  let expandedTotal = expandedHeight;
  let expanded = false;
  let writes = 0;
  let pendingScrollEvent = false;
  const totalHeight = (): number => (expanded ? expandedTotal : expandedTotal - collapsedGap);
  let position = totalHeight() - clientHeight;
  Object.defineProperties(element, {
    clientHeight: { configurable: true, get: () => clientHeight },
    directWrites: { configurable: true, get: () => writes },
    scrollHeight: { configurable: true, get: totalHeight },
    scrollTop: {
      configurable: true,
      get: () => position,
      set: (next: number) => {
        writes += 1;
        // Landing on the expanded true bottom drops the trailing measurement,
        // which shrinks the content and clamps the write the browser just made.
        if (expanded && next >= expandedTotal - clientHeight) {
          expanded = false;
          pendingScrollEvent = true;
        }
        position = Math.max(0, Math.min(next, totalHeight() - clientHeight));
      },
    },
  });
  element.scrollTo = ({ top }: ScrollToOptions) => {
    if (typeof top === "number") element.scrollTop = top;
  };
  element.getBoundingClientRect = () => new DOMRect(0, 0, 100, clientHeight);
  element.remeasure = () => {
    expanded = true;
  };
  element.setExpandedHeight = (height) => {
    expandedTotal = height;
    expanded = true;
  };
  element.takeScrollEvent = () => {
    const pending = pendingScrollEvent;
    pendingScrollEvent = false;
    return pending;
  };
  return element;
}

function createHarness<TElement extends HTMLElement = TestElement>(
  element: TElement = createElement() as unknown as TElement,
): TestHarness<TElement> {
  let nextFrame = 1;
  const frames = new Map<number, () => void>();
  const scrollCalls: TestHarness<TElement>["scrollCalls"] = [];
  const states: string[] = [];
  const visualBottom: boolean[] = [];
  const debugEvents: TestHarness<TElement>["debugEvents"] = [];
  const deps: ChatScrollControllerDeps = {
    cancelFrame: (id) => frames.delete(id),
    debugLog: (event, detail) => debugEvents.push({ event, detail }),
    getLastIndex: () => 9,
    getScrollElement: () => element,
    onStateChange: (state) => states.push(state),
    onVisualBottomChange: (atBottom) => visualBottom.push(atBottom),
    requestFrame: (callback) => {
      const id = nextFrame;
      nextFrame += 1;
      frames.set(id, callback);
      return id;
    },
    scrollToIndex: (options) => scrollCalls.push(options),
  };
  const controller = createChatScrollController(deps);
  return {
    controller,
    element,
    flushFrames: () => {
      while (frames.size > 0) {
        const queued = Array.from(frames.values());
        frames.clear();
        queued.forEach((callback) => callback());
      }
    },
    flushNextFrame: () => {
      const queued = Array.from(frames.values());
      frames.clear();
      queued.forEach((callback) => callback());
    },
    pendingFrames: () => frames.size,
    pinReasons: () => debugEvents
      .filter(({ event }) => event === "pin")
      .map(({ detail }) => String(detail.reason)),
    scrollCalls,
    states,
    visualBottom,
    debugEvents,
  };
}

function attach(harness: TestHarness<HTMLElement>): void {
  harness.controller.attach(harness.element);
  harness.flushFrames();
}

/**
 * Replays the captured feedback loop: a queued frame writes the bottom, the
 * clamped write emits a native scroll event, and the virtualizer restores its
 * measurement before the next frame observes the geometry.
 */
function runBistableCycles(
  harness: TestHarness<BistableElement>,
  element: BistableElement,
  cycles: number,
): void {
  for (let cycle = 0; cycle < cycles; cycle += 1) {
    harness.flushNextFrame();
    if (!element.takeScrollEvent()) continue;
    harness.controller.notifyScroll();
    element.remeasure();
  }
}

describe("ChatScrollController", () => {
  it("attaches with an initial bottom intent and executes the pin", () => {
    const harness = createHarness(createElement({ scrollTop: 0 }));

    attach(harness);

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.scrollCalls).toEqual([{ index: 9, align: "end", behavior: "auto" }]);
    expect(harness.element.scrollTop).toBe(500);
  });

  it("coalesces streaming growth into one pin per frame while pinned", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;
    harness.element.setGeometry({ scrollHeight: 1300 });

    harness.controller.notifyContentGrowth();
    harness.controller.notifyContentGrowth();
    harness.controller.notifyContentGrowth();

    expect(harness.pendingFrames()).toBe(1);
    harness.flushFrames();
    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.element.scrollTop).toBe(800);
  });

  it("keeps a queued content-growth pin through a stale bottom scroll event", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.notifyContentGrowth();
    harness.controller.notifyScroll();
    harness.element.setGeometry({ scrollHeight: 1_077 });
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.element.scrollTop).toBe(577);
  });

  it("keeps pinned growth alive through delayed virtualizer measurement", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.notifyContentGrowth();
    harness.flushNextFrame();
    harness.element.setGeometry({ scrollHeight: 1_200 });
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.element.scrollTop).toBe(700);
  });

  it("corrects a deferred virtualizer footer offset without replaying item alignment", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;
    harness.element.setGeometry({ scrollHeight: 1_200 });

    harness.controller.notifyContentGrowth();
    harness.flushNextFrame();
    harness.element.setGeometry({ scrollTop: 637 });
    harness.controller.notifyScroll();
    harness.flushFrames();

    expect(harness.element.scrollTop).toBe(700);
    expect(harness.scrollCalls).toHaveLength(0);
  });

  it("absorbs a short programmatic-pin replay instead of treating it as an away scroll", () => {
    const harness = createHarness();
    attach(harness);
    harness.element.setGeometry({ scrollHeight: 1300, scrollTop: 650 });

    harness.controller.notifyScroll();

    expect(harness.controller.getState()).toBe("pinned");
    harness.flushFrames();
    expect(harness.element.scrollTop).toBe(800);
  });

  it("keeps following an unattributed upward virtualizer correction while pinned", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;
    harness.element.setGeometry({ scrollHeight: 1200, scrollTop: 450 });

    harness.controller.notifyScroll();
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.element.scrollTop).toBe(700);
  });

  it("keeps a pinned request alive through delayed tail materialization", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.requestPin("streaming-started", "auto");
    harness.flushNextFrame();
    harness.element.setGeometry({ scrollHeight: 1200 });
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.scrollCalls).toHaveLength(1);
    expect(harness.element.scrollTop).toBe(700);
  });

  it("keeps following a re-measure correction that lands at the new true bottom", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;
    harness.element.setGeometry({ scrollHeight: 1200, scrollTop: 700 });

    harness.controller.notifyScroll();
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.scrollCalls).toHaveLength(0);
  });

  it("keeps pinned after a downward wheel at bottom with no native scroll event", () => {
    const harness = createHarness();
    attach(harness);

    harness.controller.notifyWheel(20, false);

    expect(harness.controller.getState()).toBe("pinned");
  });

  it("unfollows only upward wheel input and does not re-pin on later growth", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;
    harness.element.setGeometry({ scrollTop: 300, scrollHeight: 1200 });

    harness.controller.notifyWheel(-40, false);
    harness.controller.notifyContentGrowth();
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("free");
    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.visualBottom).toContain(false);
  });

  it("reports input and geometry-relevant controller state through the debug seam", () => {
    const harness = createHarness();
    attach(harness);
    harness.debugEvents.length = 0;
    harness.element.setGeometry({ scrollTop: 300 });

    harness.controller.notifyWheel(-40, false);
    harness.controller.notifyScroll();

    expect(harness.debugEvents).toEqual(expect.arrayContaining([
      {
        event: "wheel",
        detail: expect.objectContaining({
          state: "pinned",
          activeIntent: "bottom",
          deltaY: -40,
          isNestedScrollableTarget: false,
        }),
      },
      {
        event: "away-input",
        detail: expect.objectContaining({ state: "free", source: "wheel" }),
      },
      {
        event: "scroll",
        detail: expect.objectContaining({
          state: "free",
          current: 300,
          priorScrollTop: 500,
          movedUp: true,
        }),
      },
    ]));
  });

  it("re-follows only when a free reader reaches the true visual bottom", () => {
    const harness = createHarness();
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.element.setGeometry({ scrollTop: 500 });

    harness.controller.notifyScroll();

    expect(harness.controller.getState()).toBe("pinned");
  });

  it("keeps returning through growth and cancels the descent on away input", () => {
    const harness = createHarness(createElement({ scrollTop: 100 }));
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.element.setGeometry({ scrollTop: 100 });

    harness.controller.scrollToBottomClicked();
    expect(harness.controller.getState()).toBe("returning");
    harness.flushNextFrame();
    expect(harness.element.scrollTop).toBe(500);
    harness.element.setGeometry({ scrollHeight: 1300 });
    harness.controller.notifyContentGrowth();
    harness.flushFrames();
    expect(harness.element.scrollTop).toBe(800);
    expect(harness.controller.getState()).toBe("pinned");

    harness.controller.notifyWheel(-1, false);
    harness.controller.scrollToBottomClicked();
    harness.controller.notifyWheel(-1, false);
    const callsBefore = harness.scrollCalls.length;
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("free");
    expect(harness.scrollCalls).toHaveLength(callsBefore);
  });

  it("uses pointer-derived upward scrolling, not pointerdown alone, as away intent", () => {
    const harness = createHarness();
    attach(harness);

    harness.controller.notifyPointerDown();
    harness.controller.notifyPointerUp();
    expect(harness.controller.getState()).toBe("pinned");

    harness.controller.notifyPointerDown();
    harness.element.setGeometry({ scrollTop: 300 });
    harness.controller.notifyScroll();

    expect(harness.controller.getState()).toBe("free");
  });

  it("ignores nested scrollable wheel input", () => {
    const harness = createHarness();
    attach(harness);

    harness.controller.notifyWheel(-10, true);

    expect(harness.controller.getState()).toBe("pinned");
  });

  it("attributes prepend compensation and growth to the prepend epoch without writes", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.element.setGeometry({ scrollHeight: 1_100, scrollTop: 500 });
    harness.controller.notifyPrepend();
    harness.element.setGeometry({ scrollTop: 700, scrollHeight: 1400 });
    harness.controller.notifyScroll();
    harness.controller.notifyContentGrowth();
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.scrollCalls).toHaveLength(0);

    harness.controller.notifyContentGrowth();
    harness.flushFrames();
    expect(harness.scrollCalls).toHaveLength(0);
  });

  it("keeps pinned through a post-prepend Virtuoso clamp at the new true bottom", () => {
    const harness = createHarness();
    attach(harness);

    harness.controller.notifyPrepend();
    harness.flushNextFrame();
    harness.element.setGeometry({ scrollHeight: 900, scrollTop: 400 });
    harness.controller.notifyScroll();

    expect(harness.controller.getState()).toBe("pinned");
  });

  it("resumes a returning descent after prepend settlement cancels its queued pin", () => {
    const harness = createHarness(createElement({ scrollTop: 0 }));
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.element.setGeometry({ scrollTop: 0 });
    harness.controller.notifyScroll();
    harness.scrollCalls.length = 0;

    harness.controller.scrollToBottomClicked();
    harness.controller.notifyPrepend();
    expect(harness.pendingFrames()).toBe(1);
    harness.flushFrames();

    expect(harness.scrollCalls).toEqual([{ index: 9, align: "end", behavior: "auto" }]);
    expect(harness.element.scrollTop).toBe(500);
    expect(harness.controller.getState()).toBe("pinned");
  });

  it("closes an oscillating prepend epoch at the frame cap so growth can pin again", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.element.setGeometry({ scrollHeight: 1_100, scrollTop: 500 });
    harness.controller.notifyPrepend();
    for (let frame = 0; frame < 30; frame += 1) {
      harness.element.setGeometry({ scrollTop: 501 + frame });
      harness.flushNextFrame();
    }
    harness.element.setGeometry({ scrollHeight: 1_600 });
    harness.controller.notifyContentGrowth();
    harness.flushFrames();

    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.element.scrollTop).toBe(1_100);
  });

  it("ignores zero-size resize epochs and re-pins only a previously pinned controller", () => {
    const pinned = createHarness();
    attach(pinned);
    pinned.scrollCalls.length = 0;
    pinned.element.setGeometry({ clientHeight: 0 });
    pinned.controller.notifyContainerResize();
    pinned.element.setGeometry({ clientHeight: 400, scrollHeight: 1200, scrollTop: 0 });
    pinned.controller.notifyContainerResize();

    expect(pinned.scrollCalls).toHaveLength(1);
    expect(pinned.element.scrollTop).toBe(800);

    const free = createHarness();
    attach(free);
    free.scrollCalls.length = 0;
    free.controller.notifyWheel(-1, false);
    free.element.setGeometry({ clientHeight: 0 });
    free.controller.notifyContainerResize();
    free.element.setGeometry({ clientHeight: 400, scrollHeight: 1200, scrollTop: 0 });
    free.controller.notifyContainerResize();

    expect(free.controller.getState()).toBe("free");
    expect(free.scrollCalls).toHaveLength(0);
  });

  it("keeps timestamp jumps non-following", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.jumpToIndex(42);
    harness.element.setGeometry({ scrollTop: 500 });
    harness.controller.notifyScroll();

    expect(harness.controller.getState()).toBe("free");
    expect(harness.scrollCalls).toEqual([{ index: 42, align: "start", behavior: "auto" }]);
  });

  it("keeps a multi-frame jump correction free even when it lands at bottom", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.jumpToIndex(42);
    harness.element.setGeometry({ scrollTop: 300 });
    harness.controller.notifyScroll();
    harness.flushNextFrame();
    harness.element.setGeometry({ scrollTop: 500 });
    harness.controller.notifyScroll();
    harness.flushNextFrame();
    harness.flushNextFrame();

    expect(harness.controller.getState()).toBe("free");
    expect(harness.scrollCalls).toEqual([{ index: 42, align: "start", behavior: "auto" }]);

    harness.element.setGeometry({ scrollTop: 400 });
    harness.controller.notifyScroll();
    harness.element.setGeometry({ scrollTop: 500 });
    harness.controller.notifyScroll();

    expect(harness.controller.getState()).toBe("pinned");
  });

  it("releases jump suppression at the frame cap so bottom re-entry works", () => {
    const harness = createHarness();
    attach(harness);

    harness.controller.jumpToIndex(42);
    for (let frame = 0; frame < 30; frame += 1) {
      harness.element.setGeometry({ scrollTop: 100 + frame });
      harness.flushNextFrame();
    }
    harness.element.setGeometry({ scrollTop: 500 });
    harness.controller.notifyScroll();

    expect(harness.controller.getState()).toBe("pinned");
  });

  it("clears jump suppression after wheel input so bottom re-entry recovers", () => {
    const harness = createHarness();
    attach(harness);

    harness.controller.jumpToIndex(42);
    harness.controller.notifyWheel(-40, false);
    // Free-state wheel input is inert by design; the jump settle clears the
    // re-entry suppression once the post-jump position is frame-stable.
    harness.flushFrames();
    harness.element.setGeometry({ scrollTop: 500 });
    harness.controller.notifyScroll();

    expect(harness.controller.getState()).toBe("pinned");
  });

  it("returns a free reader to bottom for an explicit user-message pin", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;
    harness.controller.notifyWheel(-40, false);
    harness.element.setGeometry({ scrollTop: 300 });

    harness.controller.pinForUserIntent("new-user-message", "auto");
    expect(harness.controller.getState()).toBe("returning");
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.element.scrollTop).toBe(500);
    expect(harness.scrollCalls).toEqual([{ index: 9, align: "end", behavior: "auto" }]);
  });

  it("does not synchronously overwrite a smooth bottom pin", () => {
    const harness = createHarness();
    attach(harness);
    const scrollTo = vi.fn(({ top }: ScrollToOptions) => {
      if (typeof top === "number") harness.element.setGeometry({ scrollTop: top });
    });
    harness.element.scrollTo = scrollTo;
    const directWritesBeforePin = harness.element.directWrites;

    harness.element.setGeometry({ scrollHeight: 1200, scrollTop: 500 });
    harness.controller.requestPin("smooth-pin", "smooth");
    harness.flushFrames();

    expect(scrollTo).toHaveBeenCalledOnce();
    expect(scrollTo).toHaveBeenCalledWith({ top: 700, behavior: "smooth" });
    expect(harness.element.directWrites).toBe(directWritesBeforePin);
  });

  it("restores a captured element anchor and skips an unchanged anchor", () => {
    const harness = createHarness();
    const fallbackAnchor = document.createElement("button");
    const anchor = document.createElement("button");
    let anchorTop = 100;
    fallbackAnchor.setAttribute("data-chat-scroll-anchor", "");
    anchor.getBoundingClientRect = () => new DOMRect(0, anchorTop, 20, 20);
    fallbackAnchor.getBoundingClientRect = () => new DOMRect(0, 20, 20, 20);
    harness.element.append(fallbackAnchor);
    harness.element.append(anchor);
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.element.setGeometry({ scrollTop: 200 });

    harness.controller.captureAnchor(anchor);
    anchorTop = 160;
    harness.controller.restoreAnchor();

    expect(harness.element.scrollTop).toBe(260);
    const writesAfterMove = harness.element.directWrites;
    harness.controller.captureAnchor();
    harness.controller.restoreAnchor();
    expect(harness.element.directWrites).toBe(writesAfterMove);
  });

  it("re-pins when an anchor captured at visual bottom is restored", () => {
    const harness = createHarness();
    const anchor = document.createElement("button");
    anchor.setAttribute("data-chat-scroll-anchor", "");
    anchor.getBoundingClientRect = () => new DOMRect(0, 100, 20, 20);
    harness.element.append(anchor);
    attach(harness);
    harness.scrollCalls.length = 0;
    harness.element.setGeometry({ scrollHeight: 1200, scrollTop: 700 });

    harness.controller.captureAnchor();
    harness.element.setGeometry({ scrollHeight: 1400, scrollTop: 700 });
    harness.controller.restoreAnchor();
    harness.flushFrames();

    expect(harness.scrollCalls).toHaveLength(1);
    expect(harness.element.scrollTop).toBe(900);
  });

  it("reset restores pinned state and discards pending epochs and intents", () => {
    const harness = createHarness();
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.controller.notifyPrepend();

    harness.controller.reset();
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.element.scrollTop).toBe(500);
  });

  it("does not pin during reset while its attached scroller has zero height", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;
    const writesBeforeReset = harness.element.directWrites;
    harness.element.setGeometry({ clientHeight: 0 });

    harness.controller.reset();
    harness.flushFrames();

    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.element.directWrites).toBe(writesBeforeReset);

    harness.element.setGeometry({ clientHeight: 400, scrollHeight: 1_200, scrollTop: 0 });
    harness.controller.notifyContainerResize();

    expect(harness.scrollCalls).toHaveLength(1);
    expect(harness.element.scrollTop).toBe(800);
  });

  it("uses frame scheduling without creating timers", () => {
    vi.useFakeTimers();
    const harness = createHarness();

    attach(harness);
    harness.controller.notifyContentGrowth();

    expect(vi.getTimerCount()).toBe(0);
    vi.useRealTimers();
  });

  it("treats keyboard away input as cancelling a returning bottom intent", () => {
    const harness = createHarness(createElement({ scrollTop: 100 }));
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.scrollCalls.length = 0;

    harness.controller.scrollToBottomClicked();
    harness.controller.notifyKeyScroll("up");
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("free");
    expect(harness.scrollCalls).toHaveLength(0);
  });

  it("re-arms a returning descent when the true bottom moves before it settles", () => {
    const harness = createHarness(createElement({ scrollTop: 100 }));
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.scrollCalls.length = 0;

    harness.controller.scrollToBottomClicked();
    harness.flushNextFrame();
    harness.element.setGeometry({ scrollHeight: 1300 });
    harness.flushNextFrame();
    harness.flushNextFrame();

    expect(harness.controller.getState()).toBe("returning");
    expect(harness.scrollCalls).toEqual([
      { index: 9, align: "end", behavior: "auto" },
    ]);
    expect(harness.element.scrollTop).toBe(800);
  });

  it("keeps a manual bottom return alive through a delayed virtualizer measurement", () => {
    const harness = createHarness(createElement({ scrollTop: 100 }));
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.scrollCalls.length = 0;

    harness.controller.scrollToBottomClicked();
    harness.flushNextFrame();
    harness.flushNextFrame();
    harness.element.setGeometry({ scrollHeight: 1_300 });
    harness.flushFrames();

    expect(harness.element.scrollTop).toBe(800);
    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.scrollCalls).toHaveLength(1);
  });

  it("does not mistake an upward virtualizer correction during return for user-away intent", () => {
    const harness = createHarness(createElement({ scrollTop: 300 }));
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.element.setGeometry({ scrollTop: 300 });
    harness.scrollCalls.length = 0;

    harness.controller.scrollToBottomClicked();
    harness.flushNextFrame();
    harness.element.setGeometry({ scrollHeight: 1_300, scrollTop: 420 });
    harness.controller.notifyScroll();
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.element.scrollTop).toBe(800);
    expect(harness.scrollCalls).toHaveLength(1);
  });

  it("settles a delayed Virtuoso alignment correction without reissuing item alignment", () => {
    const harness = createHarness(createElement({
      clientHeight: 1_102,
      scrollHeight: 1_880,
      scrollTop: 598,
    }));
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.element.setGeometry({ scrollTop: 598 });
    harness.controller.notifyScroll();
    harness.scrollCalls.length = 0;

    harness.controller.scrollToBottomClicked();
    harness.flushNextFrame();
    expect(harness.scrollCalls).toHaveLength(1);
    expect(harness.element.scrollTop).toBe(778);

    // Replay the captured ordering: the queued Virtuoso item alignment lands
    // using stale geometry, then its measurement reports content growth.
    harness.element.setGeometry({ scrollHeight: 1_860, scrollTop: 598 });
    harness.controller.notifyScroll();
    harness.element.setGeometry({ scrollHeight: 1_931, scrollTop: 609 });
    harness.controller.notifyContentGrowth();
    harness.flushFrames();

    expect(harness.scrollCalls).toHaveLength(1);
    expect(harness.element.scrollTop).toBe(829);
    expect(harness.controller.getState()).toBe("pinned");
  });

  it("keeps a returning intent alive through late geometry after a bottom scroll", () => {
    const harness = createHarness(createElement({
      clientHeight: 1_102,
      scrollHeight: 1_923,
      scrollTop: 821,
    }));
    attach(harness);
    harness.debugEvents.length = 0;

    harness.controller.notifyContentGrowth();
    harness.flushNextFrame();
    const writesAfterInitialPin = harness.element.directWrites;

    // The virtualizer renormalizes its trailing geometry, moving scrollTop by
    // the same amount. This scroll event still acknowledges the true bottom.
    harness.element.setGeometry({ scrollHeight: 1_903, scrollTop: 801 });
    harness.controller.notifyScroll();

    expect(harness.controller.getState()).toBe("returning");
    expect(harness.pendingFrames()).toBeGreaterThan(0);

    // The next-frame measurement materializes another part of the tail after
    // the native scroll event briefly reported the old true bottom.
    harness.element.setGeometry({ scrollHeight: 1_923, scrollTop: 801 });
    harness.flushFrames();

    expect(harness.element.directWrites).toBeGreaterThan(writesAfterInitialPin);
    expect(harness.element.scrollTop).toBe(821);
    expect(harness.debugEvents).toEqual(expect.arrayContaining([
      {
        event: "pin",
        detail: expect.objectContaining({ reason: "returning-settle" }),
      },
    ]));
  });

  it("waits through changing prepend compensation before treating later growth as followable", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.notifyPrepend();
    harness.element.setGeometry({ scrollHeight: 1_400, scrollTop: 600 });
    harness.flushNextFrame();
    harness.element.setGeometry({ scrollHeight: 1_400, scrollTop: 700 });
    harness.flushNextFrame();
    harness.flushNextFrame();
    harness.controller.notifyContentGrowth();
    harness.flushFrames();

    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.element.scrollTop).toBe(900);
  });

  it("pins its timeline target safely when the scroll element disappears", () => {
    const element = createElement();
    const scrollToIndex = vi.fn();
    const controller = createChatScrollController({
      cancelFrame: vi.fn(),
      getLastIndex: () => 4,
      getScrollElement: () => null,
      requestFrame: (callback) => {
        callback();
        return 1;
      },
      scrollToIndex,
    });

    controller.requestPin("reconnect", "auto");

    expect(scrollToIndex).toHaveBeenCalledExactlyOnceWith({
      index: 4,
      align: "end",
      behavior: "auto",
    });
    expect(element.directWrites).toBe(0);
  });

  it("keeps a free reader free when an automatic pin is requested", () => {
    const harness = createHarness();
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.scrollCalls.length = 0;
    const writesBeforeRequest = harness.element.directWrites;

    harness.controller.requestPin("streaming-started", "auto");
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("free");
    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.element.directWrites).toBe(writesBeforeRequest);
  });

  it("coalesces repeated bottom-control clicks into one returning descent", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.scrollToBottomClicked();
    harness.controller.scrollToBottomClicked();
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("pinned");
    expect(harness.scrollCalls).toEqual([{ index: 9, align: "end", behavior: "auto" }]);
  });

  it("keeps a missing fallback anchor as a restore no-op", () => {
    const harness = createHarness();
    attach(harness);
    const writesBeforeRestore = harness.element.directWrites;

    harness.controller.captureAnchor();
    harness.controller.restoreAnchor();

    expect(harness.element.directWrites).toBe(writesBeforeRestore);
    expect(harness.controller.getState()).toBe("pinned");
  });

  it("keeps a returning anchor restore on intent when its scroll write replays synchronously", () => {
    const element = createElement();
    const harness = createHarness(element);
    const anchor = document.createElement("button");
    let anchorTop = 100;
    anchor.getBoundingClientRect = () => new DOMRect(0, anchorTop, 20, 20);
    element.append(anchor);
    attach(harness);
    harness.controller.notifyWheel(-1, false);
    harness.controller.scrollToBottomClicked();
    harness.flushNextFrame();
    element.setGeometry({ scrollTop: 400 });
    let replayTop = 400;
    Object.defineProperty(element, "scrollTop", {
      configurable: true,
      get: () => replayTop,
      set: (next: number) => {
        anchorTop -= next - replayTop;
        replayTop = next;
        harness.controller.notifyScroll();
      },
    });

    harness.controller.captureAnchor(anchor);
    anchorTop = 160;
    harness.controller.restoreAnchor();

    expect(harness.controller.getState()).toBe("returning");
    expect(element.scrollTop).toBe(460);
  });

  it("keeps detached controller notifications harmless and idempotent", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;
    harness.controller.detach();
    harness.controller.detach();
    const writesAfterDetach = harness.element.directWrites;

    harness.controller.notifyContainerResize();
    harness.controller.notifyContentGrowth();
    harness.controller.notifyPrepend();
    harness.controller.notifyScroll();
    harness.flushFrames();

    expect(harness.element.directWrites).toBe(writesAfterDetach);
    expect(harness.scrollCalls).toHaveLength(0);
  });

  // Captured in Chromium against real Virtuoso: an appended timeline item and
  // measurement growth coalesce into one pin, and the surviving item alignment
  // makes the virtualizer a second, asynchronous scroll writer. Its deferred
  // write targets the last item's end, which sits above the footer spacer, so
  // it drags the reader 200-600px back up the transcript.
  it("drops item alignment from a pin batch that measurement growth joined", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.requestPin("new-timeline-item-appended", "auto");
    harness.controller.notifyContentGrowth();
    harness.flushFrames();

    expect(harness.scrollCalls).toHaveLength(0);
    expect(harness.element.scrollTop).toBe(500);
  });

  it("keeps item alignment for a pin the virtualizer alone can resolve", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.requestPin("new-timeline-item-appended", "auto");
    harness.flushFrames();

    expect(harness.scrollCalls).toEqual([{ index: 9, align: "end", behavior: "auto" }]);
  });

  // The scroller's scrollHeight under-reads by hundreds of pixels while the
  // virtualizer's rendered range lags the scroll position. The browser already
  // clamps scrollTop down when content genuinely shrinks, so a bottom pin that
  // writes a target above the current position is always the transient read.
  it("never pins to a bottom above the current position", () => {
    const harness = createHarness();
    attach(harness);
    harness.element.setGeometry({ scrollHeight: 1_200 });
    harness.controller.notifyContentGrowth();
    harness.flushFrames();
    expect(harness.element.scrollTop).toBe(700);
    const writesBefore = harness.element.directWrites;

    harness.element.setGeometry({ scrollHeight: 900 });
    harness.controller.notifyContentGrowth();
    harness.flushFrames();

    expect(harness.element.scrollTop).toBe(700);
    expect(harness.element.directWrites).toBe(writesBefore);
  });

  // KNOWN OPEN DEFECT — expected to fail. This models the residual case the
  // controller cannot win: the extent collapses on the write itself, so every
  // restored measurement is genuine forward progress and the browser clamps it
  // straight back. Measured in Chromium, this is now the only remaining source
  // of upward jumps (1-4 per streaming turn, ~300px), and it needs the
  // virtualizer to stop republishing a stale extent, not another pin rule.
  it.fails("stops re-pinning a virtualizer whose measurement collapses on every bottom write", () => {
    const element = createBistableElement();
    const harness = createHarness(element);
    attach(harness);
    harness.debugEvents.length = 0;
    const writesAfterAttach = element.directWrites;

    // The virtualizer restores its trailing measurement, so the true bottom
    // moves 20px away from the position the previous clamp left behind.
    element.remeasure();
    harness.controller.notifyContentGrowth();
    runBistableCycles(harness, element, 24);

    expect(harness.pinReasons().length).toBeLessThanOrEqual(4);
    expect(element.directWrites - writesAfterAttach).toBeLessThanOrEqual(8);
    expect(harness.pendingFrames()).toBe(0);
    expect(harness.controller.getState()).toBe("pinned");
    // The reader is left on the tail of the measurement the scroller settled
    // into, and the bottom control was last told the transcript is followed.
    expect(element.scrollTop).toBe(505);
    expect(harness.visualBottom.at(-1) ?? true).toBe(true);
  });

  it("stops a pin loop whose true bottom rotates through republished measurements", () => {
    // Captured in Chromium against real Virtuoso: the pin target cycles through
    // three previously published bottoms instead of repeating a single one.
    const heights = [2_665, 3_169, 2_645];
    const harness = createHarness(createElement({ clientHeight: 891, scrollHeight: heights[0] }));
    attach(harness);
    harness.debugEvents.length = 0;

    for (let cycle = 0; cycle < 15; cycle += 1) {
      harness.element.setGeometry({ scrollHeight: heights[cycle % heights.length] });
      harness.controller.notifyScroll();
      harness.flushFrames();
    }

    expect(harness.pinReasons().length).toBeLessThanOrEqual(8);
  });

  it("keeps following streaming growth that advances the true bottom every pin", () => {
    const element = createBistableElement();
    const harness = createHarness(element);
    attach(harness);
    harness.debugEvents.length = 0;

    for (let block = 1; block <= 6; block += 1) {
      element.setExpandedHeight(1_416 + block * 40);
      harness.controller.notifyContentGrowth();
      harness.flushNextFrame();
      harness.flushNextFrame();
      if (element.takeScrollEvent()) harness.controller.notifyScroll();
    }

    expect(harness.pinReasons().length).toBeGreaterThanOrEqual(6);
    expect(element.scrollTop).toBe(1_416 + 6 * 40 - 20 - 891);
  });

  it("ignores stale post-detach prepends so a replacement scroller receives its attach pin", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;

    harness.controller.detach();
    harness.controller.notifyPrepend();
    harness.controller.attach(harness.element);
    harness.flushFrames();

    expect(harness.scrollCalls).toEqual([{ index: 9, align: "end", behavior: "auto" }]);
    expect(harness.element.scrollTop).toBe(500);
  });
});
