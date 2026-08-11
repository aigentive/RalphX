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

interface TestHarness<TElement extends HTMLElement = TestElement> {
  element: TElement;
  controller: ChatScrollController;
  flushFrames(): void;
  flushNextFrame(): void;
  pendingFrames(): number;
  scrollCalls: Array<{ index: "LAST" | number; align: "start" | "end"; behavior: "auto" | "smooth" }>;
  autoscrollCalls(): number;
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

function createHarness<TElement extends HTMLElement = TestElement>(
  element: TElement = createElement() as unknown as TElement,
): TestHarness<TElement> {
  let nextFrame = 1;
  let autoscrolls = 0;
  const frames = new Map<number, () => void>();
  const scrollCalls: TestHarness<TElement>["scrollCalls"] = [];
  const states: string[] = [];
  const visualBottom: boolean[] = [];
  const debugEvents: TestHarness<TElement>["debugEvents"] = [];
  const deps: ChatScrollControllerDeps = {
    autoscrollToBottom: () => {
      autoscrolls += 1;
    },
    cancelFrame: (id) => frames.delete(id),
    debugLog: (event, detail) => debugEvents.push({ event, detail }),
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
    autoscrollCalls: () => autoscrolls,
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

const FOLLOW_LAST = { index: "LAST", align: "end", behavior: "auto" } as const;

describe("ChatScrollController", () => {
  describe("bottom follow is delegated, never written", () => {
    it("attaches at pinned without writing scroll or following", () => {
      const harness = createHarness(createElement({ scrollTop: 0 }));

      attach(harness);

      expect(harness.controller.getState()).toBe("pinned");
      expect(harness.scrollCalls).toHaveLength(0);
      expect(harness.autoscrollCalls()).toBe(0);
      expect(harness.element.scrollTop).toBe(0);
      expect(harness.element.directWrites).toBe(0);
    });

    it("resets to pinned without writing scroll or following", () => {
      const harness = createHarness();
      attach(harness);
      harness.controller.notifyWheel(-1, false);
      const writesBeforeReset = harness.element.directWrites;

      harness.controller.reset();
      harness.flushFrames();

      expect(harness.controller.getState()).toBe("pinned");
      expect(harness.scrollCalls).toHaveLength(0);
      expect(harness.autoscrollCalls()).toBe(0);
      expect(harness.element.directWrites).toBe(writesBeforeReset);
    });

    it("arms the virtualizer's own follow window on content growth at the bottom", () => {
      const harness = createHarness();
      attach(harness);
      const writesBeforeGrowth = harness.element.directWrites;

      harness.controller.notifyContentGrowth();
      harness.flushFrames();

      expect(harness.autoscrollCalls()).toBe(1);
      expect(harness.scrollCalls).toHaveLength(0);
      expect(harness.element.directWrites).toBe(writesBeforeGrowth);
      expect(harness.element.scrollTop).toBe(500);
    });

    // Hydration can collapse the extent for a frame, so Virtuoso's own write
    // clamps to 0 and its post-growth window - which only acts on a size
    // increase that pushed the reader off the bottom - never recovers it.
    it("re-issues the follow when growth left the follower short", () => {
      const harness = createHarness();
      attach(harness);
      harness.element.setGeometry({ scrollHeight: 1_300 });
      const writesBeforeGrowth = harness.element.directWrites;

      harness.controller.notifyContentGrowth();
      harness.flushFrames();

      expect(harness.scrollCalls).toEqual([FOLLOW_LAST]);
      expect(harness.autoscrollCalls()).toBe(0);
      expect(harness.element.directWrites).toBe(writesBeforeGrowth);
    });

    it("coalesces a burst of short-landing growth into one correction", () => {
      const harness = createHarness();
      attach(harness);
      harness.element.setGeometry({ scrollHeight: 1_300 });

      harness.controller.notifyContentGrowth();
      harness.controller.notifyContentGrowth();
      harness.controller.notifyContentGrowth();
      harness.flushFrames();

      expect(harness.scrollCalls).toEqual([FOLLOW_LAST]);
    });

    it("waits for a settled extent before correcting", () => {
      const harness = createHarness();
      attach(harness);
      harness.element.setGeometry({ scrollHeight: 1_300 });

      harness.controller.notifyContentGrowth();
      // The measurement is still moving, so following it would land the reader
      // against torn geometry - the shape that moved readers back up.
      harness.element.setGeometry({ scrollHeight: 1_500 });
      harness.flushNextFrame();
      expect(harness.scrollCalls).toHaveLength(0);

      harness.flushNextFrame();

      expect(harness.scrollCalls).toEqual([FOLLOW_LAST]);
    });

    it("does not correct a free reader who is short of the bottom", () => {
      const harness = createHarness();
      attach(harness);
      harness.controller.notifyWheel(-40, false);
      harness.element.setGeometry({ scrollHeight: 1_300 });

      harness.controller.notifyContentGrowth();
      harness.flushFrames();

      expect(harness.scrollCalls).toHaveLength(0);
      expect(harness.autoscrollCalls()).toBe(0);
    });

    it("arms one follow per growth signal so overlapping windows cover slow streaming", () => {
      const harness = createHarness();
      attach(harness);

      harness.controller.notifyContentGrowth();
      harness.controller.notifyContentGrowth();
      harness.controller.notifyContentGrowth();
      harness.flushFrames();

      expect(harness.autoscrollCalls()).toBe(3);
      expect(harness.element.directWrites).toBe(0);
    });

    it("arms the follow window on container resize without writing scroll", () => {
      const harness = createHarness();
      attach(harness);
      const writesBeforeResize = harness.element.directWrites;

      harness.controller.notifyContainerResize();
      harness.flushFrames();

      expect(harness.autoscrollCalls()).toBe(1);
      expect(harness.scrollCalls).toHaveLength(0);
      expect(harness.element.directWrites).toBe(writesBeforeResize);
    });

    it("never schedules a correction write for an unmet bottom", () => {
      const harness = createHarness();
      attach(harness);
      // A 1000px gap to the reported true bottom: the pin loop used to treat
      // this as an unmet intent and write scrollTop every frame.
      harness.element.setGeometry({ scrollHeight: 2_000 });
      const writesBeforeScroll = harness.element.directWrites;

      harness.controller.notifyScroll();
      harness.flushFrames();

      expect(harness.pendingFrames()).toBe(0);
      expect(harness.element.directWrites).toBe(writesBeforeScroll);
      expect(harness.scrollCalls).toHaveLength(0);
      expect(harness.autoscrollCalls()).toBe(0);
    });

    it("reports armed follow through the debug seam", () => {
      const harness = createHarness();
      attach(harness);
      harness.debugEvents.length = 0;

      harness.controller.notifyContentGrowth();

      expect(harness.debugEvents).toEqual(expect.arrayContaining([
        expect.objectContaining({ event: "content-growth" }),
        expect.objectContaining({ event: "growth-follow-armed" }),
      ]));
    });
  });

  describe("user intent follows the last item", () => {
    it("returns a free reader to the last item for an explicit user-message pin", () => {
      const harness = createHarness();
      attach(harness);
      harness.controller.notifyWheel(-40, false);
      harness.element.setGeometry({ scrollTop: 300 });
      const writesBeforePin = harness.element.directWrites;

      harness.controller.pinForUserIntent("new-user-message", "auto");

      expect(harness.controller.getState()).toBe("pinned");
      expect(harness.scrollCalls).toEqual([FOLLOW_LAST]);
      expect(harness.element.directWrites).toBe(writesBeforePin);
    });

    it("follows the last item on a bottom-control click", () => {
      const harness = createHarness();
      attach(harness);
      harness.controller.notifyWheel(-40, false);
      harness.element.setGeometry({ scrollTop: 300 });
      const writesBeforeClick = harness.element.directWrites;

      harness.controller.scrollToBottomClicked();

      expect(harness.controller.getState()).toBe("pinned");
      expect(harness.scrollCalls).toEqual([FOLLOW_LAST]);
      expect(harness.element.directWrites).toBe(writesBeforeClick);
    });

    it("honours a smooth behavior request without a synchronous write", () => {
      const harness = createHarness();
      attach(harness);
      const writesBeforePin = harness.element.directWrites;

      harness.controller.requestPin("smooth-pin", "smooth");

      expect(harness.scrollCalls).toEqual([
        { index: "LAST", align: "end", behavior: "smooth" },
      ]);
      expect(harness.element.directWrites).toBe(writesBeforePin);
    });

    it("keeps a free reader free when an automatic pin is requested", () => {
      const harness = createHarness();
      attach(harness);
      harness.controller.notifyWheel(-1, false);
      const writesBeforeRequest = harness.element.directWrites;

      harness.controller.requestPin("streaming-started", "auto");
      harness.flushFrames();

      expect(harness.controller.getState()).toBe("free");
      expect(harness.scrollCalls).toHaveLength(0);
      expect(harness.autoscrollCalls()).toBe(0);
      expect(harness.element.directWrites).toBe(writesBeforeRequest);
    });

    it("does not arm growth follow for a free reader", () => {
      const harness = createHarness();
      attach(harness);
      harness.element.setGeometry({ scrollTop: 300, scrollHeight: 1_200 });

      harness.controller.notifyWheel(-40, false);
      harness.controller.notifyContentGrowth();
      harness.flushFrames();

      expect(harness.controller.getState()).toBe("free");
      expect(harness.autoscrollCalls()).toBe(0);
      expect(harness.scrollCalls).toHaveLength(0);
      expect(harness.visualBottom).toContain(false);
    });

    it("follows the last item when the scroll element has disappeared", () => {
      const scrollToIndex = vi.fn();
      const autoscrollToBottom = vi.fn();
      const controller = createChatScrollController({
        autoscrollToBottom,
        cancelFrame: vi.fn(),
        getScrollElement: () => null,
        requestFrame: (callback) => {
          callback();
          return 1;
        },
        scrollToIndex,
      });

      controller.requestPin("reconnect", "auto");

      expect(scrollToIndex).toHaveBeenCalledExactlyOnceWith(FOLLOW_LAST);
    });
  });

  describe("away intent classification", () => {
    it("unfollows on upward wheel input", () => {
      const harness = createHarness();
      attach(harness);

      harness.controller.notifyWheel(-40, false);

      expect(harness.controller.getState()).toBe("free");
    });

    it("keeps pinned after a downward wheel at bottom with no native scroll event", () => {
      const harness = createHarness();
      attach(harness);

      harness.controller.notifyWheel(20, false);

      expect(harness.controller.getState()).toBe("pinned");
    });

    it("ignores nested scrollable wheel input", () => {
      const harness = createHarness();
      attach(harness);

      harness.controller.notifyWheel(-10, true);

      expect(harness.controller.getState()).toBe("pinned");
    });

    it("treats keyboard up input as away intent", () => {
      const harness = createHarness();
      attach(harness);

      harness.controller.notifyKeyScroll("up");

      expect(harness.controller.getState()).toBe("free");
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

    it("re-follows only when a free reader reaches the true visual bottom", () => {
      const harness = createHarness();
      attach(harness);
      harness.controller.notifyWheel(-1, false);
      harness.element.setGeometry({ scrollTop: 300 });
      harness.controller.notifyScroll();
      expect(harness.controller.getState()).toBe("free");

      harness.element.setGeometry({ scrollTop: 500 });
      harness.controller.notifyScroll();

      expect(harness.controller.getState()).toBe("pinned");
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
  });

  describe("prepend and zero-size epochs suppress follow", () => {
    it("attributes prepend compensation and growth to the prepend epoch without following", () => {
      const harness = createHarness();
      attach(harness);

      harness.element.setGeometry({ scrollHeight: 1_100, scrollTop: 500 });
      harness.controller.notifyPrepend();
      harness.element.setGeometry({ scrollTop: 700, scrollHeight: 1_400 });
      harness.controller.notifyScroll();
      harness.controller.notifyContentGrowth();
      harness.flushNextFrame();

      expect(harness.controller.getState()).toBe("pinned");
      expect(harness.autoscrollCalls()).toBe(0);
      expect(harness.scrollCalls).toHaveLength(0);
    });

    it("closes an oscillating prepend epoch at the frame cap so growth can follow again", () => {
      const harness = createHarness();
      attach(harness);

      harness.element.setGeometry({ scrollHeight: 1_100, scrollTop: 500 });
      harness.controller.notifyPrepend();
      for (let frame = 0; frame < 30; frame += 1) {
        harness.element.setGeometry({ scrollTop: 501 + frame });
        harness.flushNextFrame();
      }
      harness.element.setGeometry({ scrollTop: 600, scrollHeight: 1_100 });
      harness.controller.notifyContentGrowth();

      expect(harness.autoscrollCalls()).toBe(1);
    });

    it("ignores stale post-detach prepends", () => {
      const harness = createHarness();
      attach(harness);

      harness.controller.detach();
      harness.controller.notifyPrepend();
      harness.controller.attach(harness.element);
      harness.controller.notifyContentGrowth();

      expect(harness.autoscrollCalls()).toBe(1);
    });

    it("suppresses follow while zero-size and re-arms only a pinned controller", () => {
      const pinned = createHarness();
      attach(pinned);
      pinned.element.setGeometry({ clientHeight: 0 });
      pinned.controller.notifyContainerResize();
      expect(pinned.autoscrollCalls()).toBe(0);

      pinned.element.setGeometry({ clientHeight: 400, scrollHeight: 1_200, scrollTop: 0 });
      pinned.controller.notifyContainerResize();

      expect(pinned.autoscrollCalls()).toBe(1);
      expect(pinned.element.directWrites).toBe(0);

      const free = createHarness();
      attach(free);
      free.controller.notifyWheel(-1, false);
      free.element.setGeometry({ clientHeight: 0 });
      free.controller.notifyContainerResize();
      free.element.setGeometry({ clientHeight: 400, scrollHeight: 1_200, scrollTop: 0 });
      free.controller.notifyContainerResize();

      expect(free.controller.getState()).toBe("free");
      expect(free.autoscrollCalls()).toBe(0);
    });

    it("keeps detached controller notifications harmless and idempotent", () => {
      const harness = createHarness();
      attach(harness);
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
      expect(harness.autoscrollCalls()).toBe(0);
    });

    it("uses frame scheduling without creating timers", () => {
      vi.useFakeTimers();
      const harness = createHarness();

      attach(harness);
      harness.controller.notifyPrepend();

      expect(vi.getTimerCount()).toBe(0);
      vi.useRealTimers();
    });
  });

  describe("timestamp jumps", () => {
    it("keeps timestamp jumps non-following at a shifted index", () => {
      const harness = createHarness();
      attach(harness);

      harness.controller.jumpToIndex(42);
      harness.element.setGeometry({ scrollTop: 500 });
      harness.controller.notifyScroll();

      expect(harness.controller.getState()).toBe("free");
      expect(harness.scrollCalls).toEqual([{ index: 42, align: "start", behavior: "auto" }]);
    });

    it("keeps a multi-frame jump correction free even when it lands at bottom", () => {
      const harness = createHarness();
      attach(harness);

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

    it("clears jump suppression for an explicit user follow", () => {
      const harness = createHarness();
      attach(harness);

      harness.controller.jumpToIndex(42);
      harness.scrollCalls.length = 0;
      harness.controller.pinForUserIntent("new-user-message", "auto");

      expect(harness.controller.getState()).toBe("pinned");
      expect(harness.scrollCalls).toEqual([FOLLOW_LAST]);
    });
  });

  describe("anchor restore", () => {
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

    it("arms follow instead of writing when an anchor captured at bottom is restored", () => {
      const harness = createHarness();
      const anchor = document.createElement("button");
      anchor.setAttribute("data-chat-scroll-anchor", "");
      anchor.getBoundingClientRect = () => new DOMRect(0, 100, 20, 20);
      harness.element.append(anchor);
      attach(harness);
      const writesBeforeRestore = harness.element.directWrites;

      harness.controller.captureAnchor();
      harness.element.setGeometry({ scrollHeight: 1_400 });
      harness.controller.restoreAnchor();
      harness.flushFrames();

      expect(harness.autoscrollCalls()).toBe(1);
      expect(harness.scrollCalls).toHaveLength(0);
      expect(harness.element.directWrites).toBe(writesBeforeRestore);
    });

    it("keeps a missing fallback anchor as a restore no-op", () => {
      const harness = createHarness();
      attach(harness);
      const writesBeforeRestore = harness.element.directWrites;

      harness.controller.captureAnchor();
      harness.controller.restoreAnchor();

      expect(harness.element.directWrites).toBe(writesBeforeRestore);
      expect(harness.autoscrollCalls()).toBe(0);
      expect(harness.controller.getState()).toBe("pinned");
    });
  });
});
