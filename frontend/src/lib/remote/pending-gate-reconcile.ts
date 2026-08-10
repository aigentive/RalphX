/**
 * P-21 client half: the reconcile SIGNAL (PR 2.7-c).
 *
 * A permission or question gate raised while this client was disconnected produced no
 * event we ever saw, and a gate resolved while we were away produced no event either.
 * Replaying the stream fixes neither: gates live in backend memory, not in the event
 * log. So every (re)connect — warm or cold — has to re-ask the host what is pending and
 * treat the answer as authoritative.
 *
 * ## Why a signal rather than a direct write
 *
 * Gate state is owned by the surfaces that render it (`PermissionDialog`'s queue, the
 * `uiStore` question map). The composition root is a module, not a component, and must
 * not become a second writer of either. It therefore announces "reconcile now" and the
 * OWNERS do the fetch — one writer per gate concern, unchanged.
 *
 * ## Ordering
 *
 * Fired from the bus's post-`goLive` seam, so the sequence is: replay done → env-scoped
 * query sweep → gate reconcile. Gates are re-read after the cache is already refreshing,
 * so a gate and the task it belongs to cannot be reconciled against different vintages.
 *
 * A-5: this schedules nothing. It is an announcement on an already-completed connect.
 */

export const PENDING_GATE_RECONCILE_EVENT = "ralphx:pending-gate-reconcile";

export interface PendingGateReconcileDetail {
  /** The environment whose connect triggered this. Listeners scope on it. */
  readonly environmentId: string;
}

/**
 * Announces that `environmentId` just (re)connected.
 *
 * No-op outside a DOM (tests importing the runtime headlessly), because a missing
 * `window` means there is no gate UI to reconcile either.
 */
export function requestPendingGateReconcile(environmentId: string): void {
  if (typeof window === "undefined") {
    return;
  }
  window.dispatchEvent(
    new CustomEvent<PendingGateReconcileDetail>(PENDING_GATE_RECONCILE_EVENT, {
      detail: { environmentId },
    })
  );
}

/**
 * Subscribes to reconcile announcements. Returns the detach function.
 *
 * SCOPE hygiene (big-PR EVENT class): the listener receives the announcing environment
 * id and is expected to ignore anything that is not the one it renders for. A background
 * environment's connect must never rewrite the active environment's gate UI.
 */
export function onPendingGateReconcile(
  listener: (detail: PendingGateReconcileDetail) => void
): () => void {
  if (typeof window === "undefined") {
    return () => {};
  }
  const handler = (event: Event): void => {
    if (!(event instanceof CustomEvent)) {
      return;
    }
    const detail = event.detail as PendingGateReconcileDetail | undefined;
    if (detail === undefined || typeof detail.environmentId !== "string") {
      return;
    }
    listener(detail);
  };
  window.addEventListener(PENDING_GATE_RECONCILE_EVENT, handler);
  return () => window.removeEventListener(PENDING_GATE_RECONCILE_EVENT, handler);
}
