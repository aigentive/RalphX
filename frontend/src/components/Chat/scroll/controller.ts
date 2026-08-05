import {
  getTrueBottomScrollTop,
  isScrollElementVisuallyAtBottom,
  VISUAL_BOTTOM_EPSILON_PX,
} from "../ChatMessageList.scroll";

export type ChatScrollState = "pinned" | "free" | "returning";

export interface ChatScrollControllerDeps {
  getScrollElement(): HTMLElement | null;
  scrollToIndex(opts: {
    index: number;
    align: "start" | "end";
    behavior: "auto" | "smooth";
  }): void;
  getLastIndex(): number;
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
  let pinFrame: number | null = null;
  let settleFrame: number | null = null;
  let returningSettleFrameCount = 0;
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
  let pinBehavior: "auto" | "smooth" = "auto";
  let pinReasons: string[] = [];
  let pinNeedsIndexAlignment = false;
  let suppressFreeBottomReentry = false;
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

  const cancelPendingPin = (): void => {
    cancelFrame(pinFrame);
    pinFrame = null;
    pinReasons = [];
    pinNeedsIndexAlignment = false;
  };

  const cancelSettle = (): void => {
    cancelFrame(settleFrame);
    settleFrame = null;
    returningSettleFrameCount = 0;
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

  const scrollToTrueBottom = (element: HTMLElement, behavior: "auto" | "smooth"): number => {
    const target = getTrueBottomScrollTop(element);
    if (element.scrollTop !== target) {
      element.scrollTo({ top: target, behavior });
    }
    if (behavior !== "smooth" && element.scrollTop !== target) {
      element.scrollTop = target;
    }
    return target;
  };

  const hasActiveBottomIntent = (): boolean => activeIntent?.target === "bottom";

  const isWithinActiveIntent = (element: HTMLElement): boolean => {
    if (!activeIntent) return false;
    const { target, epsilon } = activeIntent;
    if (target === "bottom") {
      return Math.abs(getTrueBottomScrollTop(element) - element.scrollTop) <= epsilon;
    }
    if (typeof target === "object" && "anchor" in target) {
      const offset = target.anchor.getBoundingClientRect().top - element.getBoundingClientRect().top;
      return Math.abs(offset - target.offset) <= epsilon;
    }
    if (typeof target === "object" && "offset" in target) {
      return Math.abs(element.scrollTop - target.offset) <= epsilon;
    }
    return false;
  };

  const cancelIntentAndFree = (source: string): void => {
    cancelPendingPin();
    cancelSettle();
    cancelJumpSettle();
    activeIntent = null;
    setState("free");
    debug("away-input", { source });
    updateVisualBottom();
  };

  const settleBottomIntent = (source: "frame" | "scroll"): void => {
    cancelPendingPin();
    cancelSettle();
    setState("pinned");
    beginBottomIntent();
    updateVisualBottom();
    debug("bottom-intent-settled", { source });
  };

  const scheduleReturningSettle = (): void => {
    if (state !== "returning" || settleFrame !== null || prependEpoch || zeroSizeEpoch) return;
    settleFrame = deps.requestFrame(() => {
      settleFrame = null;
      const element = getElement();
      if (!element || state !== "returning" || prependEpoch || zeroSizeEpoch) return;
      returningSettleFrameCount += 1;
      if (isWithinActiveIntent(element)) {
        if (returningSettleFrameCount >= MAX_SETTLE_FRAMES) {
          settleBottomIntent("frame");
        } else {
          scheduleReturningSettle();
        }
        return;
      }
      schedulePin("returning-settle", "auto", false);
    });
  };

  const performPin = (
    reason: string,
    behavior: "auto" | "smooth",
    alignLastItem = true,
  ): void => {
    if (detached || state === "free" || prependEpoch || zeroSizeEpoch) return;
    const element = getElement();
    beginBottomIntent();
    if (alignLastItem) {
      deps.scrollToIndex({ index: deps.getLastIndex(), align: "end", behavior });
    }
    if (element) {
      const target = scrollToTrueBottom(element, behavior);
      previousScrollTop = element.scrollTop;
      debug("pin", { alignLastItem, behavior, reason, target });
    } else {
      debug("pin", { alignLastItem, behavior, reason, target: null });
    }
    updateVisualBottom();
    scheduleReturningSettle();
  };

  const schedulePin = (
    reason: string,
    behavior: "auto" | "smooth",
    alignLastItem = true,
  ): void => {
    if (detached || state === "free" || prependEpoch || zeroSizeEpoch) return;
    pinBehavior = behavior;
    pinReasons.push(reason);
    pinNeedsIndexAlignment ||= alignLastItem;
    if (pinFrame !== null) return;
    pinFrame = deps.requestFrame(() => {
      pinFrame = null;
      const reasons = pinReasons;
      const shouldAlignLastItem = pinNeedsIndexAlignment;
      pinReasons = [];
      pinNeedsIndexAlignment = false;
      performPin(reasons.join(","), pinBehavior, shouldAlignLastItem);
    });
  };

  const startReturningBottomIntent = (
    reason: string,
    behavior: "auto" | "smooth",
    alignLastItem = true,
  ): void => {
    cancelJumpSettle();
    cancelSettle();
    setState("returning");
    beginBottomIntent();
    schedulePin(reason, behavior, alignLastItem);
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
      if (state === "returning") schedulePin("prepend-settled-returning", "auto");
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

  const rebaselineAfterZeroSize = (): void => {
    previousScrollTop = getElement()?.scrollTop ?? null;
    updateVisualBottom();
    if (state !== "free" && !prependEpoch) {
      performPin("zero-size-rebaseline", "auto");
    }
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
      if (!zeroSizeEpoch) schedulePin("attach", "auto");
    },

    detach() {
      detached = true;
      cancelPendingPin();
      cancelSettle();
      cancelJumpSettle();
      cancelFrame(prependFrame);
      pinReasons = [];
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
      cancelPendingPin();
      cancelSettle();
      cancelJumpSettle();
      cancelFrame(prependFrame);
      pinReasons = [];
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
      if (attachedElement && !zeroSizeEpoch) schedulePin("reset", "auto");
    },

    notifyWheel(deltaY, isNestedScrollableTarget) {
      debug("wheel", { deltaY, isNestedScrollableTarget });
      if (isNestedScrollableTarget) return;
      classifyAgainstTargetInput(deltaY < 0, "wheel");
      if (deltaY > 0 && state === "returning") schedulePin("wheel-affirmation", "auto");
    },

    notifyKeyScroll(direction) {
      debug("key-scroll", { direction });
      classifyAgainstTargetInput(direction === "up", "key");
      if (direction === "down" && state === "returning") schedulePin("key-affirmation", "auto");
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
      if (state === "free") {
        if (atBottom && !suppressFreeBottomReentry) {
          setState("pinned");
          beginBottomIntent();
          debug("bottom-reentry");
        }
        return;
      }
      if (hasActiveBottomIntent() && !isWithinActiveIntent(element)) {
        schedulePin("scroll-intent-correction", "auto", false);
      }
      scheduleReturningSettle();
    },

    notifyContentGrowth() {
      debug("content-growth");
      if (prependEpoch) {
        previousScrollTop = getElement()?.scrollTop ?? previousScrollTop;
        schedulePrependSettle();
        return;
      }
      updateVisualBottom();
      if (state !== "free") {
        // The semantic request that introduced the tail already aligned the
        // virtual item. Measurement-driven growth must only reconcile the
        // native scroller; another async item alignment can replay stale
        // Virtuoso geometry and fight the true-bottom write indefinitely.
        startReturningBottomIntent("content-growth", "auto", false);
      }
    },

    notifyContainerResize() {
      const element = getElement();
      debug("container-resize", { clientHeight: element?.clientHeight ?? null });
      if (!element || element.clientHeight === 0) {
        zeroSizeEpoch = true;
        cancelPendingPin();
        cancelSettle();
        cancelJumpSettle();
        return;
      }
      if (zeroSizeEpoch) {
        zeroSizeEpoch = false;
        rebaselineAfterZeroSize();
        return;
      }
      updateVisualBottom();
      if (state !== "free" && !prependEpoch) {
        cancelPendingPin();
        performPin("container-resize", "auto", false);
      }
    },

    notifyPrepend() {
      if (detached) return;
      prependEpoch = true;
      prependSettleFrameCount = 0;
      cancelPendingPin();
      cancelSettle();
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
      startReturningBottomIntent(reason, behavior);
    },

    pinForUserIntent(reason, behavior) {
      debug("user-pin-request", { behavior, reason });
      startReturningBottomIntent(reason, behavior);
    },

    scrollToBottomClicked() {
      debug("scroll-to-bottom-click");
      startReturningBottomIntent("scroll-to-bottom-click", "auto");
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
      cancelPendingPin();
      cancelSettle();
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
        if (state !== "free") schedulePin("anchor-bottom-restore", "auto");
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
