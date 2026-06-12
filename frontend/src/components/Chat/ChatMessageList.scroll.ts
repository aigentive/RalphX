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
