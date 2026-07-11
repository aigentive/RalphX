export const VISUAL_BOTTOM_EPSILON_PX = 2;

export function getTrueBottomScrollTop(
  element: Pick<HTMLElement, "scrollHeight" | "clientHeight">,
): number {
  return Math.max(0, element.scrollHeight - element.clientHeight);
}

export function isScrollElementVisuallyAtBottom(
  element: Pick<HTMLElement, "scrollHeight" | "clientHeight" | "scrollTop">,
): boolean {
  return getTrueBottomScrollTop(element) - element.scrollTop <= VISUAL_BOTTOM_EPSILON_PX;
}
