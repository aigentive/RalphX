import { useCallback, useEffect, useRef } from "react";

const READ_DELAY_MS = 1_000;
const BATCH_WINDOW_MS = 100;

interface UseViewportReadBatchOptions {
  enabled: boolean;
  onMarkRead: (ids: readonly string[]) => void;
}

/** Queues unread rows that remain visible for a second into one short batch window. */
export function useViewportReadBatch({ enabled, onMarkRead }: UseViewportReadBatchOptions) {
  const observerRef = useRef<IntersectionObserver | null>(null);
  const elementsRef = useRef(new Map<string, Element>());
  const visibilityTimersRef = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const pendingRef = useRef(new Set<string>());
  const markedRef = useRef(new Set<string>());
  const batchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onMarkReadRef = useRef(onMarkRead);

  useEffect(() => {
    onMarkReadRef.current = onMarkRead;
  }, [onMarkRead]);

  const clearVisibilityTimer = useCallback((id: string) => {
    const timer = visibilityTimersRef.current.get(id);
    if (timer !== undefined) clearTimeout(timer);
    visibilityTimersRef.current.delete(id);
  }, []);

  const flush = useCallback(() => {
    batchTimerRef.current = null;
    const ids = [...pendingRef.current];
    pendingRef.current.clear();
    if (ids.length > 0) onMarkReadRef.current(ids);
  }, []);

  useEffect(() => {
    if (!enabled || typeof IntersectionObserver === "undefined") return undefined;

    const firstElement = elementsRef.current.values().next().value as Element | undefined;
    const scrollViewport = firstElement?.closest("[data-radix-scroll-area-viewport]") ?? null;
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const id = [...elementsRef.current.entries()].find(([, element]) => element === entry.target)?.[0];
        if (!id || markedRef.current.has(id)) continue;
        if (!entry.isIntersecting) {
          clearVisibilityTimer(id);
          continue;
        }
        if (visibilityTimersRef.current.has(id)) continue;
        visibilityTimersRef.current.set(id, setTimeout(() => {
          visibilityTimersRef.current.delete(id);
          markedRef.current.add(id);
          pendingRef.current.add(id);
          if (batchTimerRef.current === null) {
            batchTimerRef.current = setTimeout(flush, BATCH_WINDOW_MS);
          }
        }, READ_DELAY_MS));
      }
    }, { root: scrollViewport, threshold: 0.5 });
    observerRef.current = observer;
    elementsRef.current.forEach((element) => observer.observe(element));

    return () => {
      observer.disconnect();
      observerRef.current = null;
      visibilityTimersRef.current.forEach((timer) => clearTimeout(timer));
      visibilityTimersRef.current.clear();
      if (batchTimerRef.current !== null) clearTimeout(batchTimerRef.current);
      batchTimerRef.current = null;
      pendingRef.current.clear();
    };
  }, [clearVisibilityTimer, enabled, flush]);

  return useCallback((id: string, readAt: string | undefined) => (element: HTMLElement | null) => {
    const previousElement = elementsRef.current.get(id);
    if (previousElement && previousElement !== element) observerRef.current?.unobserve(previousElement);
    if (!element || readAt !== undefined) {
      elementsRef.current.delete(id);
      clearVisibilityTimer(id);
      return;
    }
    elementsRef.current.set(id, element);
    observerRef.current?.observe(element);
  }, [clearVisibilityTimer]);
}
