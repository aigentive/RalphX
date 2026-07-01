const VIRTUOSO_REVEAL_BOTTOM_SPACER_EPSILON_PX = 2;

function parsePixelValue(value: string): number {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function hasUnrenderedVirtuosoBottomTail(list: HTMLElement): boolean {
  const paddingBottom = parsePixelValue(window.getComputedStyle(list).paddingBottom);
  return paddingBottom > VIRTUOSO_REVEAL_BOTTOM_SPACER_EPSILON_PX;
}

export function isTranscriptRootReadyForReveal(root: ParentNode | null): boolean {
  if (!root) {
    return false;
  }

  const virtuosoList = root.querySelector<HTMLElement>('[data-testid="virtuoso-item-list"]');
  if (virtuosoList && window.getComputedStyle(virtuosoList).visibility === "hidden") {
    return false;
  }
  if (virtuosoList && hasUnrenderedVirtuosoBottomTail(virtuosoList)) {
    return false;
  }
  if (virtuosoList) {
    return virtuosoList.children.length > 0;
  }

  return Boolean(root.querySelector('[data-chat-message-item="true"]'));
}
