export const VISUAL_BOTTOM_EPSILON_PX = 2;

export function getTrueBottomScrollTop(
  element: Pick<HTMLElement, "scrollHeight" | "clientHeight">
): number {
  return Math.max(0, element.scrollHeight - element.clientHeight);
}

export function getScrollBottomDelta(
  element: Pick<HTMLElement, "scrollHeight" | "clientHeight" | "scrollTop">
): number {
  return Math.max(0, getTrueBottomScrollTop(element) - element.scrollTop);
}

export function isScrollElementVisuallyAtBottom(
  element: Pick<HTMLElement, "scrollHeight" | "clientHeight" | "scrollTop">
): boolean {
  return getScrollBottomDelta(element) <= VISUAL_BOTTOM_EPSILON_PX;
}

type ScrollableElement = Pick<HTMLElement, "scrollHeight" | "clientHeight" | "scrollTop"> & {
  scrollTo?: (options: ScrollToOptions) => void;
};

export function scrollElementToTrueBottom(
  element: ScrollableElement,
  behavior: ScrollBehavior = "auto"
): number {
  const target = getTrueBottomScrollTop(element);
  if (element.scrollTop !== target) {
    if (typeof element.scrollTo === "function") {
      element.scrollTo({ top: target, behavior });
    }
  }
  if (element.scrollTop !== target) {
    element.scrollTop = target;
  }
  return target;
}

export interface BottomStickState {
  scrollToTimestamp: string | null | undefined;
  isAtBottom: boolean;
  isVisuallyAtBottom: boolean;
}

export function shouldStickToBottom({
  scrollToTimestamp,
  isAtBottom,
  isVisuallyAtBottom,
}: BottomStickState): boolean {
  if (scrollToTimestamp) {
    return false;
  }
  return isAtBottom || isVisuallyAtBottom;
}

export interface ScrollAwaySignalState {
  hasUserScrollInput: boolean;
  previousScrollTop: number | null;
  currentScrollTop: number;
  isVisuallyAtBottom: boolean;
}

export function shouldTreatScrollTopDecreaseAsUserAway({
  hasUserScrollInput,
  previousScrollTop,
  currentScrollTop,
  isVisuallyAtBottom,
}: ScrollAwaySignalState): boolean {
  if (!hasUserScrollInput || previousScrollTop === null || isVisuallyAtBottom) {
    return false;
  }
  return currentScrollTop < previousScrollTop;
}

export interface ScheduledBottomPinState {
  scheduledAwayVersion: number;
  currentAwayVersion: number;
  requireLastItemVisible: boolean;
  isLastItemVisible: boolean;
}

export function shouldRunScheduledBottomPin({
  scheduledAwayVersion,
  currentAwayVersion,
  requireLastItemVisible,
  isLastItemVisible,
}: ScheduledBottomPinState): boolean {
  if (currentAwayVersion !== scheduledAwayVersion) {
    return false;
  }
  return !requireLastItemVisible || isLastItemVisible;
}

export type ManualWheelScrollIntent = "away" | "bottom" | "none";

export interface ManualWheelScrollIntentState {
  deltaY: number;
  bottomDelta: number | null;
  trueBottomSettleThresholdPx: number;
}

export function getManualWheelScrollIntent({
  deltaY,
  bottomDelta,
  trueBottomSettleThresholdPx,
}: ManualWheelScrollIntentState): ManualWheelScrollIntent {
  if (deltaY < 0) {
    return "away";
  }

  if (deltaY <= 0) {
    return "none";
  }

  if (bottomDelta !== null && bottomDelta <= trueBottomSettleThresholdPx) {
    return "bottom";
  }

  return "none";
}

export interface ScrollDriftRecoveryState {
  scrollToTimestamp: string | null | undefined;
  bottomDelta: number;
  isVisuallyAtBottom: boolean;
  isUserScrollingAwayFromBottom: boolean;
  hasRecentBottomScrollIntent: boolean;
  stickyBottomThresholdPx: number;
  hasUserScrollInput: boolean;
  isAtBottom: boolean;
  wasVisuallyAtBottom: boolean;
}

export function shouldRecoverScrollDriftToBottom({
  scrollToTimestamp,
  bottomDelta,
  isVisuallyAtBottom,
  isUserScrollingAwayFromBottom,
  hasRecentBottomScrollIntent,
  stickyBottomThresholdPx,
  hasUserScrollInput,
  isAtBottom,
  wasVisuallyAtBottom,
}: ScrollDriftRecoveryState): boolean {
  if (
    scrollToTimestamp ||
    isVisuallyAtBottom ||
    isUserScrollingAwayFromBottom ||
    bottomDelta <= VISUAL_BOTTOM_EPSILON_PX
  ) {
    return false;
  }

  if (hasRecentBottomScrollIntent && bottomDelta >= stickyBottomThresholdPx) {
    return true;
  }

  if (bottomDelta < stickyBottomThresholdPx) {
    return false;
  }

  return !hasUserScrollInput && (isAtBottom || wasVisuallyAtBottom);
}

export interface ScrollToBottomControlState {
  timelineLength: number;
  scrollToTimestamp: string | null | undefined;
  hasScrollerElement: boolean;
  hasScrollableOverflow: boolean;
  isAtBottom: boolean;
  isVisuallyAtBottom: boolean;
  isLastItemVisible: boolean | null;
}

export function shouldShowScrollToBottomControl({
  timelineLength,
  scrollToTimestamp,
  hasScrollerElement,
  hasScrollableOverflow,
  isAtBottom,
  isVisuallyAtBottom,
  isLastItemVisible,
}: ScrollToBottomControlState): boolean {
  if (scrollToTimestamp || timelineLength === 0) {
    return false;
  }

  if (hasScrollerElement && !hasScrollableOverflow && isLastItemVisible !== false) {
    return false;
  }

  if (isLastItemVisible === false) {
    return true;
  }

  return hasScrollerElement
    ? !shouldStickToBottom({ scrollToTimestamp, isAtBottom, isVisuallyAtBottom })
    : !isAtBottom;
}
