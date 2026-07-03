const AGENT_SESSION_ROW_SELECTOR = '[data-testid^="agents-session-"]';

export function shouldPreserveExecutionPopoverForTarget(
  target: EventTarget | null,
): boolean {
  return target instanceof HTMLElement
    ? Boolean(target.closest(AGENT_SESSION_ROW_SELECTOR))
    : false;
}
