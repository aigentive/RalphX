import {
  isScrollElementVisuallyAtBottom,
  VISUAL_BOTTOM_EPSILON_PX,
} from "../ChatMessageList.scroll";

export type ChatScrollState = "pinned" | "free";

export interface ChatScrollControllerDeps {
  getScrollElement(): HTMLElement | null;
  /**
   * Virtuoso resolves `"LAST"` against its own unshifted `totalCount`, so bottom
   * follow never has to reason about `firstItemIndex`.
   */
  scrollToIndex(opts: {
    index: "LAST" | number;
    align: "start" | "end";
    behavior: "auto" | "smooth";
  }): void;
  /**
   * Arms Virtuoso's own post-growth follow window. It is a no-op while the
   * reader is already at the bottom and never writes `scrollTop` directly.
   */
  autoscrollToBottom(): void;
  requestFrame(cb: () => void): number;
  cancelFrame(id: number): void;
  onStateChange?(state: ChatScrollState): void;
  onVisualBottomChange?(atBottom: boolean): void;
  debugLog?(event: string, detail: Record<string, unknown>): void;
}

export interface ChatScrollController {
  attach(el: HTMLElement): void;
  detach(): void;
  reset(): void;
  notifyWheel(deltaY: number, isNestedScrollableTarget: boolean): void;
  notifyKeyScroll(direction: "up" | "down"): void;
  notifyPointerDown(): void;
  notifyPointerUp(): void;
  notifyScroll(): void;
  notifyContentGrowth(): void;
  notifyContainerResize(): void;
  notifyPrepend(): void;
  requestPin(reason: string, behavior: "auto" | "smooth"): void;
  pinForUserIntent(reason: string, behavior: "auto" | "smooth"): void;
  scrollToBottomClicked(): void;
  forwardWheel(deltaX: number, deltaY: number): void;
  jumpToIndex(index: number): void;
  captureAnchor(anchorElement?: HTMLElement): void;
  restoreAnchor(): void;
  getState(): ChatScrollState;
  isVisuallyAtBottom(): boolean;
}

type ScrollTarget = "bottom" | { offset: number } | { anchor: HTMLElement; offset: number };

interface ActiveIntent {
  target: ScrollTarget;
  epsilon: number;
}

interface CapturedAnchor {
  element: HTMLElement;
  offset: number;
  wasAtBottom: boolean;
}

const ANCHOR_SELECTOR = '[data-chat-scroll-anchor], [data-testid="tool-call-group-toggle"]';
const MAX_SETTLE_FRAMES = 30;

export function createChatScrollController(deps: ChatScrollControllerDeps): ChatScrollController {
  let state: ChatScrollState = "pinned";
  let attachedElement: HTMLElement | null = null;
  let activeIntent: ActiveIntent | null = null;
  let capturedAnchor: CapturedAnchor | null = null;
  let jumpSettleFrame: number | null = null;
  let jumpSettleScrollTop: number | null = null;
  let jumpSettleFrameCount = 0;
  let prependFrame: number | null = null;
  let prependSettleFrameCount = 0;
  let prependEpoch = false;
  let zeroSizeEpoch = false;
  let pointerSession = false;
  let previousScrollTop: number | null = null;
  let visualBottom: boolean | null = null;
  let suppressFreeBottomReentry = false;
  let correctionFrame: number | null = null;
  let correctionFrameCount = 0;
  let detached = false;

  const getElement = (): HTMLElement | null => deps.getScrollElement() ?? attachedElement;

  const debug = (event: string, detail: Record<string, unknown> = {}): void => {
    deps.debugLog?.(event, {
      state,
      activeIntent: activeIntent?.target === "bottom"
        ? "bottom"
        : activeIntent?.target
          ? "anchor" in activeIntent.target ? "anchor" : "offset"
          : null,
      pointerSession,
      prependEpoch,
      previousScrollTop,
      visualBottom,
      zeroSizeEpoch,
      ...detail,
    });
  };

  const updateVisualBottom = (): boolean => {
    const element = getElement();
    const next = element ? isScrollElementVisuallyAtBottom(element) : false;
    if (visualBottom !== next) {
      visualBottom = next;
      deps.onVisualBottomChange?.(next);
    }
    return next;
  };

  const setState = (next: ChatScrollState): void => {
    if (state === next) return;
    state = next;
    deps.onStateChange?.(next);
    debug("state-change");
  };

  const cancelFrame = (frame: number | null): void => {
    if (frame !== null) deps.cancelFrame(frame);
  };

  const cancelJumpSettle = (): void => {
    cancelFrame(jumpSettleFrame);
    jumpSettleFrame = null;
    jumpSettleScrollTop = null;
    jumpSettleFrameCount = 0;
    suppressFreeBottomReentry = false;
    if (
      activeIntent
      && typeof activeIntent.target === "object"
      && "offset" in activeIntent.target
      && !("anchor" in activeIntent.target)
    ) {
      activeIntent = null;
    }
  };

  const beginBottomIntent = (): void => {
    activeIntent = { target: "bottom", epsilon: VISUAL_BOTTOM_EPSILON_PX };
  };

  const cancelIntentAndFree = (source: string): void => {
    cancelJumpSettle();
    activeIntent = null;
    setState("free");
    debug("away-input", { source });
    updateVisualBottom();
  };

  /**
   * The single bottom-follow actuator. Virtuoso resolves `"LAST"` itself and
   * lands the item's end at the viewport bottom, which is exactly the composer
   * top now that the reserved inset lives inside that item. Nothing here writes
   * `scrollTop`: a raw corrective write was measured to destabilise Virtuoso's
   * own geometry and park the reader hundreds of pixels short.
   */
  const followBottom = (reason: string, behavior: "auto" | "smooth"): void => {
    if (detached || prependEpoch || zeroSizeEpoch) return;
    deps.scrollToIndex({ index: "LAST", align: "end", behavior });
    debug("follow-bottom", { behavior, reason });
    updateVisualBottom();
  };

  /**
   * Arms Virtuoso's post-growth follow window. Safe to call unconditionally
   * while following: it only acts when a size increase has actually pushed the
   * reader off the bottom, and does nothing when already there.
   */
  const armGrowthFollow = (): void => {
    if (detached || state === "free" || prependEpoch || zeroSizeEpoch) return;
    deps.autoscrollToBottom();
    debug("growth-follow-armed");
  };

  /**
   * One correction per frame. A height settle publishes many intermediate
   * totals, and re-issuing `scrollToIndex` on each of them restarts Virtuoso's
   * own scroll before it can land.
   */
  const scheduleFollowCorrection = (): void => {
    if (correctionFrame !== null) return;
    const scheduledScrollHeight = getElement()?.scrollHeight ?? null;
    correctionFrame = deps.requestFrame(() => {
      correctionFrame = null;
      if (detached || state === "free" || prependEpoch || zeroSizeEpoch) return;
      const element = getElement();
      if (!element) return;
      // Correct against settled geometry only. Mid-settle, Virtuoso's size tree
      // and the scroller's extent disagree by hundreds of pixels, and following
      // that torn measurement moves the reader *up* the transcript - the exact
      // symptom this change exists to remove.
      if (element.scrollHeight !== scheduledScrollHeight) {
        correctionFrameCount += 1;
        if (correctionFrameCount < MAX_SETTLE_FRAMES) scheduleFollowCorrection();
        return;
      }
      correctionFrameCount = 0;
      if (updateVisualBottom()) return;
      followBottom("content-growth-correction", "auto");
    });
  };

  const followBottomForUserIntent = (reason: string, behavior: "auto" | "smooth"): void => {
    cancelJumpSettle();
    setState("pinned");
    beginBottomIntent();
    followBottom(reason, behavior);
  };

  const schedulePrependSettle = (): void => {
    cancelFrame(prependFrame);
    prependFrame = deps.requestFrame(() => {
      prependFrame = null;
      if (!prependEpoch) return;
      const element = getElement();
      const current = element?.scrollTop ?? null;
      if (current !== previousScrollTop && prependSettleFrameCount + 1 < MAX_SETTLE_FRAMES) {
        previousScrollTop = current;
        prependSettleFrameCount += 1;
        schedulePrependSettle();
        return;
      }
      prependEpoch = false;
      prependSettleFrameCount = 0;
      if (state !== "free") beginBottomIntent();
      debug("prepend-settled");
      updateVisualBottom();
    });
  };

  const scheduleJumpSettle = (): void => {
    cancelFrame(jumpSettleFrame);
    jumpSettleFrame = deps.requestFrame(() => {
      jumpSettleFrame = null;
      if (!suppressFreeBottomReentry) return;
      const current = getElement()?.scrollTop ?? null;
      if (current !== jumpSettleScrollTop && jumpSettleFrameCount + 1 < MAX_SETTLE_FRAMES) {
        jumpSettleScrollTop = current;
        jumpSettleFrameCount += 1;
        scheduleJumpSettle();
        return;
      }
      jumpSettleScrollTop = null;
      jumpSettleFrameCount = 0;
      if (activeIntent && typeof activeIntent.target === "object" && "offset" in activeIntent.target) {
        activeIntent = null;
      }
      suppressFreeBottomReentry = false;
      debug("jump-settled");
    });
  };

  const classifyAgainstTargetInput = (isAway: boolean, source: string): void => {
    if (!isAway || state === "free") return;
    cancelIntentAndFree(source);
  };

  return {
    attach(el) {
      detached = false;
      cancelFrame(prependFrame);
      prependFrame = null;
      prependEpoch = false;
      prependSettleFrameCount = 0;
      attachedElement = el;
      previousScrollTop = el.scrollTop;
      zeroSizeEpoch = el.clientHeight === 0;
      beginBottomIntent();
      updateVisualBottom();
      // `initialTopMostItemIndex` owns the first position; an attach-time
      // follow would fight it against geometry that has not settled yet.
      debug("attach");
    },

    detach() {
      detached = true;
      cancelFrame(correctionFrame);
      correctionFrame = null;
      correctionFrameCount = 0;
      cancelJumpSettle();
      cancelFrame(prependFrame);
      prependFrame = null;
      prependEpoch = false;
      prependSettleFrameCount = 0;
      zeroSizeEpoch = false;
      pointerSession = false;
      capturedAnchor = null;
      attachedElement = null;
      activeIntent = null;
      previousScrollTop = null;
      visualBottom = null;
    },

    reset() {
      cancelJumpSettle();
      cancelFrame(prependFrame);
      prependFrame = null;
      prependEpoch = false;
      prependSettleFrameCount = 0;
      const element = getElement();
      zeroSizeEpoch = element ? element.clientHeight === 0 : false;
      pointerSession = false;
      capturedAnchor = null;
      setState("pinned");
      beginBottomIntent();
      previousScrollTop = element?.scrollTop ?? null;
      // A conversation switch remounts Virtuoso under a new key, so its
      // `initialTopMostItemIndex` lands the new transcript at its own bottom.
      debug("reset");
    },

    notifyWheel(deltaY, isNestedScrollableTarget) {
      debug("wheel", { deltaY, isNestedScrollableTarget });
      if (isNestedScrollableTarget) return;
      classifyAgainstTargetInput(deltaY < 0, "wheel");
    },

    notifyKeyScroll(direction) {
      debug("key-scroll", { direction });
      classifyAgainstTargetInput(direction === "up", "key");
    },

    notifyPointerDown() {
      pointerSession = true;
      previousScrollTop = getElement()?.scrollTop ?? previousScrollTop;
      debug("pointer-down");
    },

    notifyPointerUp() {
      pointerSession = false;
      debug("pointer-up");
    },

    notifyScroll() {
      const element = getElement();
      if (!element) return;
      const current = element.scrollTop;
      const priorScrollTop = previousScrollTop;
      const movedUp = priorScrollTop !== null && current < priorScrollTop - VISUAL_BOTTOM_EPSILON_PX;
      previousScrollTop = current;
      debug("scroll", { current, movedUp, priorScrollTop });
      if (prependEpoch) {
        schedulePrependSettle();
        updateVisualBottom();
        return;
      }
      if (pointerSession && movedUp) {
        cancelIntentAndFree("pointer-scroll");
        return;
      }
      const atBottom = updateVisualBottom();
      if (state === "free" && atBottom && !suppressFreeBottomReentry) {
        setState("pinned");
        beginBottomIntent();
        debug("bottom-reentry");
      }
      // A pinned reader sitting short of the reported extent is a transient
      // virtualizer measurement, not an unmet intent. Correcting it here is
      // what sustained the write/measure oscillation.
    },

    notifyContentGrowth() {
      debug("content-growth");
      if (prependEpoch) {
        previousScrollTop = getElement()?.scrollTop ?? previousScrollTop;
        schedulePrependSettle();
        return;
      }
      if (updateVisualBottom()) {
        armGrowthFollow();
        return;
      }
      // The transcript changed height and left a follower short: hydration can
      // collapse the extent for a frame so Virtuoso's own write clamps to 0,
      // and `autoscrollToBottom` will not recover it because its window only
      // acts on a SIZE_INCREASED that pushed the reader off the bottom.
      //
      // Re-issuing Virtuoso's actuator is safe where the old pin was not.
      // This is driven by content-height changes, never by scroll events, so a
      // follow cannot produce the signal that triggers the next correction -
      // that feedback loop is what sustained the jitter - and it writes through
      // the size tree rather than raw scrollTop.
      scheduleFollowCorrection();
    },

    notifyContainerResize() {
      const element = getElement();
      debug("container-resize", { clientHeight: element?.clientHeight ?? null });
      if (!element || element.clientHeight === 0) {
        zeroSizeEpoch = true;
        cancelJumpSettle();
        return;
      }
      if (zeroSizeEpoch) {
        zeroSizeEpoch = false;
        previousScrollTop = element.scrollTop;
      }
      updateVisualBottom();
      armGrowthFollow();
    },

    notifyPrepend() {
      if (detached) return;
      prependEpoch = true;
      prependSettleFrameCount = 0;
      cancelJumpSettle();
      previousScrollTop = getElement()?.scrollTop ?? previousScrollTop;
      schedulePrependSettle();
      debug("prepend-started");
    },

    requestPin(reason, behavior) {
      debug("pin-request", { behavior, reason });
      if (state === "free") {
        debug("pin-ignored-free", { reason });
        return;
      }
      followBottom(reason, behavior);
    },

    pinForUserIntent(reason, behavior) {
      debug("user-pin-request", { behavior, reason });
      followBottomForUserIntent(reason, behavior);
    },

    scrollToBottomClicked() {
      debug("scroll-to-bottom-click");
      followBottomForUserIntent("scroll-to-bottom-click", "auto");
    },

    forwardWheel(deltaX, deltaY) {
      const element = getElement();
      if (!element) return;
      if (typeof element.scrollBy === "function") {
        element.scrollBy({ left: deltaX, top: deltaY, behavior: "auto" });
      } else {
        element.scrollTo({
          left: element.scrollLeft + deltaX,
          top: element.scrollTop + deltaY,
          behavior: "auto",
        });
      }
      previousScrollTop = element.scrollTop;
      updateVisualBottom();
    },

    jumpToIndex(index) {
      cancelJumpSettle();
      setState("free");
      const offset = getElement()?.scrollTop ?? 0;
      activeIntent = { target: { offset }, epsilon: VISUAL_BOTTOM_EPSILON_PX };
      suppressFreeBottomReentry = true;
      jumpSettleScrollTop = offset;
      deps.scrollToIndex({ index, align: "start", behavior: "auto" });
      scheduleJumpSettle();
      debug("jump-to-index", { index });
    },

    captureAnchor(anchorElement) {
      const element = getElement();
      if (!element) return;
      const anchor = anchorElement ?? element.querySelector<HTMLElement>(ANCHOR_SELECTOR);
      if (!anchor) {
        capturedAnchor = null;
        return;
      }
      capturedAnchor = {
        element: anchor,
        offset: anchor.getBoundingClientRect().top - element.getBoundingClientRect().top,
        wasAtBottom: isScrollElementVisuallyAtBottom(element),
      };
      debug("anchor-captured", {
        offset: capturedAnchor.offset,
        wasAtBottom: capturedAnchor.wasAtBottom,
      });
    },

    restoreAnchor() {
      const element = getElement();
      const anchor = capturedAnchor;
      capturedAnchor = null;
      if (!element || !anchor) return;
      if (anchor.wasAtBottom) {
        // The toggle changed the transcript's height under a follower; let
        // Virtuoso re-land on the last item rather than writing scrollTop.
        armGrowthFollow();
        return;
      }
      const currentOffset = anchor.element.getBoundingClientRect().top - element.getBoundingClientRect().top;
      const adjustment = currentOffset - anchor.offset;
      if (Math.abs(adjustment) <= VISUAL_BOTTOM_EPSILON_PX) return;
      activeIntent = { target: { anchor: anchor.element, offset: anchor.offset }, epsilon: VISUAL_BOTTOM_EPSILON_PX };
      element.scrollTop += adjustment;
      previousScrollTop = element.scrollTop;
      activeIntent = null;
      updateVisualBottom();
      debug("anchor-restored", { adjustment });
    },

    getState() {
      return state;
    },

    isVisuallyAtBottom() {
      return updateVisualBottom();
    },
  };
}
