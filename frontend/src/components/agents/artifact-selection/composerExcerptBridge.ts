import {
  normalizeComposerExcerptReferences,
  type ComposerExcerptReference,
} from "./artifactSelection.types";

type Listener = (reference: ComposerExcerptReference) => void;

const listeners = new Map<string, Set<Listener>>();
const pending = new Map<string, ComposerExcerptReference[]>();

export function stageComposerExcerptReference(
  conversationId: string,
  reference: ComposerExcerptReference,
): void {
  const [normalized] = normalizeComposerExcerptReferences([reference]);
  if (!conversationId.trim() || !normalized) return;

  const activeListeners = listeners.get(conversationId);
  if (activeListeners && activeListeners.size > 0) {
    for (const listener of activeListeners) listener(normalized);
    return;
  }

  pending.set(
    conversationId,
    normalizeComposerExcerptReferences([
      ...(pending.get(conversationId) ?? []),
      normalized,
    ]),
  );
}

export function subscribeToComposerExcerptReferences(
  conversationId: string | null,
  listener: Listener,
): () => void {
  if (!conversationId) return () => undefined;

  const conversationListeners = listeners.get(conversationId) ?? new Set();
  conversationListeners.add(listener);
  listeners.set(conversationId, conversationListeners);

  const queued = pending.get(conversationId) ?? [];
  pending.delete(conversationId);
  for (const reference of queued) listener(reference);

  return () => {
    const current = listeners.get(conversationId);
    current?.delete(listener);
    if (current?.size === 0) listeners.delete(conversationId);
  };
}
