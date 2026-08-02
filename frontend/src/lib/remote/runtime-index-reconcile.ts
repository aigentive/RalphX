/**
 * Runtime-index reconcile SIGNAL.
 *
 * A run that started while this client was disconnected produced no lifecycle event we
 * ever saw, and a run completed while we were away produced no event either. Replaying
 * the stream fixes neither on a cold hydrate: lifecycle state is read from the registered
 * runtime index. So every (re)connect — warm or cold — has to re-ask the host what is live
 * and treat the answer as authoritative.
 *
 * ## Why a signal rather than a direct write
 *
 * Conversation liveness is owned by the global lifecycle hook through the existing
 * runtime-status reconciliation seam. The composition root is a module, not a component,
 * and must not become a second writer. It therefore announces "reconcile now" and the
 * OWNER does the fetch — one writer for chat status, unchanged.
 *
 * ## Ordering
 *
 * Fired from the bus's post-`goLive` seam, so the sequence is: replay done → env-scoped
 * query sweep → gate reconcile → runtime-index reconcile. Liveness is re-read after the
 * cache and memory-backed gates are already refreshing, preserving one connect vintage.
 *
 * A-5: this schedules nothing. It is an announcement on an already-completed connect.
 */

export const RUNTIME_INDEX_RECONCILE_EVENT = "ralphx:runtime-index-reconcile";

export interface RuntimeIndexReconcileDetail {
  /** The environment whose connect triggered this. Listeners scope on it. */
  readonly environmentId: string;
}

/**
 * Announces that `environmentId` just (re)connected.
 *
 * No-op outside a DOM (tests importing the runtime headlessly), because a missing
 * `window` means there is no chat UI to reconcile either.
 */
export function requestRuntimeIndexReconcile(environmentId: string): void {
  if (typeof window === "undefined") {
    return;
  }
  window.dispatchEvent(
    new CustomEvent<RuntimeIndexReconcileDetail>(RUNTIME_INDEX_RECONCILE_EVENT, {
      detail: { environmentId },
    }),
  );
}

/**
 * Subscribes to reconcile announcements. Returns the detach function.
 *
 * SCOPE hygiene (big-PR EVENT class): the listener receives the announcing environment
 * id and is expected to ignore anything that is not the one it renders for. A background
 * environment's connect must never rewrite the active environment's chat liveness UI.
 */
export function onRuntimeIndexReconcile(
  listener: (detail: RuntimeIndexReconcileDetail) => void,
): () => void {
  if (typeof window === "undefined") {
    return () => {};
  }
  const handler = (event: Event): void => {
    if (!(event instanceof CustomEvent)) {
      return;
    }
    const detail = event.detail as RuntimeIndexReconcileDetail | undefined;
    if (detail === undefined || typeof detail.environmentId !== "string") {
      return;
    }
    listener(detail);
  };
  window.addEventListener(RUNTIME_INDEX_RECONCILE_EVENT, handler);
  return () => window.removeEventListener(RUNTIME_INDEX_RECONCILE_EVENT, handler);
}
