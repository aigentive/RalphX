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

interface TestHarness {
  element: TestElement;
  controller: ChatScrollController;
  flushFrames(): void;
  flushNextFrame(): void;
  pendingFrames(): number;
  scrollCalls: Array<{ index: number; align: "start" | "end"; behavior: "auto" | "smooth" }>;
  states: string[];
  visualBottom: boolean[];
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

function createHarness(element = createElement()): TestHarness {
  let nextFrame = 1;
  const frames = new Map<number, () => void>();
  const scrollCalls: TestHarness["scrollCalls"] = [];
  const states: string[] = [];
  const visualBottom: boolean[] = [];
  const deps: ChatScrollControllerDeps = {
    cancelFrame: (id) => frames.delete(id),
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
    scrollCalls,
    states,
    visualBottom,
  };
}

function attach(harness: TestHarness): void {
  harness.controller.attach(harness.element);
  harness.flushFrames();
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
    expect(harness.scrollCalls).toHaveLength(1);
    expect(harness.element.scrollTop).toBe(800);
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

  it("unfollows an unattributed upward scrollbar drag without scheduling a correction", () => {
    const harness = createHarness();
    attach(harness);
    harness.scrollCalls.length = 0;
    harness.element.setGeometry({ scrollTop: 300 });

    harness.controller.notifyScroll();
    harness.flushFrames();

    expect(harness.controller.getState()).toBe("free");
    expect(harness.scrollCalls).toHaveLength(0);
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
    expect(harness.scrollCalls).toHaveLength(1);
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

    expect(harness.scrollCalls).toHaveLength(1);
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
    const scrollTo = vi.fn();
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
    expect(harness.scrollCalls).toHaveLength(2);
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

    expect(harness.scrollCalls).toHaveLength(1);
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
