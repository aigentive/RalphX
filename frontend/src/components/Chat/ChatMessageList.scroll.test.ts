import { describe, expect, it, vi } from "vitest";

import {
  getManualWheelScrollIntent,
  getTrueBottomScrollTop,
  isScrollElementVisuallyAtBottom,
  scrollElementToTrueBottom,
  shouldRecoverScrollDriftToBottom,
  shouldRunScheduledBottomPin,
  shouldShowScrollToBottomControl,
  shouldStickToBottom,
  shouldTreatScrollTopDecreaseAsUserAway,
  VISUAL_BOTTOM_EPSILON_PX,
} from "./ChatMessageList.scroll";

function scrollElement({
  scrollHeight,
  clientHeight,
  scrollTop,
}: {
  scrollHeight: number;
  clientHeight: number;
  scrollTop: number;
}): HTMLElement {
  const element = document.createElement("div");
  Object.defineProperties(element, {
    scrollHeight: { configurable: true, value: scrollHeight },
    clientHeight: { configurable: true, value: clientHeight },
    scrollTop: { configurable: true, writable: true, value: scrollTop },
  });
  return element;
}

describe("ChatMessageList scroll math", () => {
  it("does not treat the near-bottom threshold as visually at bottom", () => {
    const element = scrollElement({
      scrollHeight: 1000,
      clientHeight: 500,
      scrollTop: 400,
    });

    expect(isScrollElementVisuallyAtBottom(element)).toBe(false);
  });

  it("treats only the exact bottom plus a tiny subpixel epsilon as visually at bottom", () => {
    const target = 500;

    expect(
      isScrollElementVisuallyAtBottom(
        scrollElement({
          scrollHeight: 1000,
          clientHeight: 500,
          scrollTop: target,
        })
      )
    ).toBe(true);
    expect(
      isScrollElementVisuallyAtBottom(
        scrollElement({
          scrollHeight: 1000,
          clientHeight: 500,
          scrollTop: target - VISUAL_BOTTOM_EPSILON_PX + 0.25,
        })
      )
    ).toBe(true);
    expect(
      isScrollElementVisuallyAtBottom(
        scrollElement({
          scrollHeight: 1000,
          clientHeight: 500,
          scrollTop: target - VISUAL_BOTTOM_EPSILON_PX - 0.25,
        })
      )
    ).toBe(false);
  });

  it("targets the scroll container's absolute bottom", () => {
    expect(
      getTrueBottomScrollTop(
        scrollElement({
          scrollHeight: 1320,
          clientHeight: 500,
          scrollTop: 0,
        })
      )
    ).toBe(820);
    expect(
      getTrueBottomScrollTop(
        scrollElement({
          scrollHeight: 320,
          clientHeight: 500,
          scrollTop: 0,
        })
      )
    ).toBe(0);
  });

  it("shows the scroll-to-bottom control when Virtuoso range is above the last item even if bottom state is stale", () => {
    expect(
      shouldShowScrollToBottomControl({
        hasScrollerElement: true,
        hasScrollableOverflow: true,
        isAtBottom: true,
        isLastItemVisible: false,
        isVisuallyAtBottom: true,
        scrollToTimestamp: null,
        timelineLength: 10,
      })
    ).toBe(true);
  });

  it("hides the scroll-to-bottom control at the visual bottom", () => {
    expect(
      shouldShowScrollToBottomControl({
        hasScrollerElement: true,
        hasScrollableOverflow: true,
        isAtBottom: true,
        isLastItemVisible: true,
        isVisuallyAtBottom: true,
        scrollToTimestamp: null,
        timelineLength: 10,
      })
    ).toBe(false);
  });

  it("keeps existing non-scroller fallback visibility behavior", () => {
    expect(
      shouldShowScrollToBottomControl({
        hasScrollerElement: false,
        hasScrollableOverflow: false,
        isAtBottom: false,
        isLastItemVisible: null,
        isVisuallyAtBottom: true,
        scrollToTimestamp: null,
        timelineLength: 10,
      })
    ).toBe(true);
  });

  it("hides the scroll-to-bottom control while the user is still inside the sticky bottom threshold", () => {
    expect(
      shouldShowScrollToBottomControl({
        hasScrollerElement: true,
        hasScrollableOverflow: true,
        isAtBottom: true,
        isLastItemVisible: true,
        isVisuallyAtBottom: false,
        scrollToTimestamp: null,
        timelineLength: 2,
      })
    ).toBe(false);
  });

  it("shows the scroll-to-bottom control when overflowed content is outside the sticky bottom threshold", () => {
    expect(
      shouldShowScrollToBottomControl({
        hasScrollerElement: true,
        hasScrollableOverflow: true,
        isAtBottom: false,
        isLastItemVisible: true,
        isVisuallyAtBottom: false,
        scrollToTimestamp: null,
        timelineLength: 2,
      })
    ).toBe(true);
  });

  it("hides the scroll-to-bottom control when the scroller has no overflow", () => {
    expect(
      shouldShowScrollToBottomControl({
        hasScrollerElement: true,
        hasScrollableOverflow: false,
        isAtBottom: false,
        isLastItemVisible: true,
        isVisuallyAtBottom: false,
        scrollToTimestamp: null,
        timelineLength: 2,
      })
    ).toBe(false);
  });

  it("keeps sticky follow active for near-bottom streaming layout growth", () => {
    expect(
      shouldStickToBottom({
        isAtBottom: true,
        isVisuallyAtBottom: false,
        scrollToTimestamp: null,
      })
    ).toBe(true);
  });

  it("does not keep sticky follow active after the user scrolls outside the bottom threshold", () => {
    expect(
      shouldStickToBottom({
        isAtBottom: false,
        isVisuallyAtBottom: false,
        scrollToTimestamp: null,
      })
    ).toBe(false);
  });

  it("does not keep sticky follow active in history mode", () => {
    expect(
      shouldStickToBottom({
        isAtBottom: true,
        isVisuallyAtBottom: true,
        scrollToTimestamp: "2026-01-01T12:00:00.000Z",
      })
    ).toBe(false);
  });

  it("does not treat layout-clamped scrollTop decreases at visual bottom as user scroll-away", () => {
    expect(
      shouldTreatScrollTopDecreaseAsUserAway({
        hasUserScrollInput: true,
        previousScrollTop: 620,
        currentScrollTop: 580,
        isVisuallyAtBottom: true,
      })
    ).toBe(false);
  });

  it("treats scrollTop decreases away from visual bottom as user scroll-away after input", () => {
    expect(
      shouldTreatScrollTopDecreaseAsUserAway({
        hasUserScrollInput: true,
        previousScrollTop: 620,
        currentScrollTop: 580,
        isVisuallyAtBottom: false,
      })
    ).toBe(true);
  });

  it("ignores programmatic scrollTop decreases without prior user input", () => {
    expect(
      shouldTreatScrollTopDecreaseAsUserAway({
        hasUserScrollInput: false,
        previousScrollTop: 620,
        currentScrollTop: 580,
        isVisuallyAtBottom: false,
      })
    ).toBe(false);
  });

  it("cancels scheduled bottom pins when the user-away version changes", () => {
    expect(
      shouldRunScheduledBottomPin({
        scheduledAwayVersion: 2,
        currentAwayVersion: 3,
        requireLastItemVisible: false,
        isLastItemVisible: false,
      })
    ).toBe(false);
  });

  it("requires the last item to be visible for guarded scheduled bottom pins", () => {
    expect(
      shouldRunScheduledBottomPin({
        scheduledAwayVersion: 2,
        currentAwayVersion: 2,
        requireLastItemVisible: true,
        isLastItemVisible: false,
      })
    ).toBe(false);

    expect(
      shouldRunScheduledBottomPin({
        scheduledAwayVersion: 2,
        currentAwayVersion: 2,
        requireLastItemVisible: false,
        isLastItemVisible: false,
      })
    ).toBe(true);
  });

  it("classifies downward wheel input in the final-pixel zone as bottom intent", () => {
    expect(
      getManualWheelScrollIntent({
        deltaY: 120,
        bottomDelta: 24,
        trueBottomSettleThresholdPx: 32,
      })
    ).toBe("bottom");
  });

  it("classifies upward wheel input as scroll-away intent", () => {
    expect(
      getManualWheelScrollIntent({
        deltaY: -1,
        bottomDelta: 0,
        trueBottomSettleThresholdPx: 32,
      })
    ).toBe("away");
  });

  it("defers downward wheel-away classification until the resulting scroll position is known", () => {
    expect(
      getManualWheelScrollIntent({
        deltaY: 120,
        bottomDelta: 60,
        trueBottomSettleThresholdPx: 32,
      })
    ).toBe("none");
  });

  it("recovers large late layout drift after an explicit bottom intent", () => {
    expect(
      shouldRecoverScrollDriftToBottom({
        bottomDelta: 180,
        hasRecentBottomScrollIntent: true,
        hasUserScrollInput: true,
        isAtBottom: false,
        isUserScrollingAwayFromBottom: false,
        isVisuallyAtBottom: false,
        scrollToTimestamp: null,
        stickyBottomThresholdPx: 150,
        wasVisuallyAtBottom: false,
      })
    ).toBe(true);
  });

  it("does not recover large drift after ordinary user input without bottom intent", () => {
    expect(
      shouldRecoverScrollDriftToBottom({
        bottomDelta: 180,
        hasRecentBottomScrollIntent: false,
        hasUserScrollInput: true,
        isAtBottom: true,
        isUserScrollingAwayFromBottom: false,
        isVisuallyAtBottom: false,
        scrollToTimestamp: null,
        stickyBottomThresholdPx: 150,
        wasVisuallyAtBottom: true,
      })
    ).toBe(false);
  });

  it("does not recover drift during history targeting or manual scroll-away", () => {
    expect(
      shouldRecoverScrollDriftToBottom({
        bottomDelta: 200,
        hasRecentBottomScrollIntent: true,
        hasUserScrollInput: false,
        isAtBottom: true,
        isUserScrollingAwayFromBottom: false,
        isVisuallyAtBottom: false,
        scrollToTimestamp: "2026-01-01T12:00:00.000Z",
        stickyBottomThresholdPx: 150,
        wasVisuallyAtBottom: true,
      })
    ).toBe(false);

    expect(
      shouldRecoverScrollDriftToBottom({
        bottomDelta: 200,
        hasRecentBottomScrollIntent: true,
        hasUserScrollInput: false,
        isAtBottom: true,
        isUserScrollingAwayFromBottom: true,
        isVisuallyAtBottom: false,
        scrollToTimestamp: null,
        stickyBottomThresholdPx: 150,
        wasVisuallyAtBottom: true,
      })
    ).toBe(false);
  });

  it("forces the DOM scroll position to the true bottom synchronously", () => {
    const element = scrollElement({
      scrollHeight: 1320,
      clientHeight: 500,
      scrollTop: 300,
    });
    const scrollTo = vi.fn();
    Object.defineProperty(element, "scrollTo", {
      configurable: true,
      value: scrollTo,
    });

    expect(scrollElementToTrueBottom(element, "smooth")).toBe(820);
    expect(scrollTo).toHaveBeenCalledWith({ top: 820, behavior: "smooth" });
    expect(element.scrollTop).toBe(820);
  });

  it("does not issue a native scroll call when already at the true bottom", () => {
    const element = scrollElement({
      scrollHeight: 1320,
      clientHeight: 500,
      scrollTop: 820,
    });
    const scrollTo = vi.fn();
    Object.defineProperty(element, "scrollTo", {
      configurable: true,
      value: scrollTo,
    });

    expect(scrollElementToTrueBottom(element, "smooth")).toBe(820);
    expect(scrollTo).not.toHaveBeenCalled();
    expect(element.scrollTop).toBe(820);
  });
});
