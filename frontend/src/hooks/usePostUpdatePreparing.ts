import { useCallback, useEffect, useRef, useState } from "react";
import {
  clearPostUpdatePreparing,
  readFreshPostUpdatePreparingMarker,
} from "@/lib/postUpdatePreparing";

const DEFAULT_MIN_DURATION_MS = 3_200;
const DEFAULT_MAX_DURATION_MS = 12_000;

interface PostUpdatePreparingOptions {
  minDurationMs?: number;
  maxDurationMs?: number;
}

function afterPaint(callback: () => void): () => void {
  if (typeof window === "undefined" || !window.requestAnimationFrame) {
    const timeoutId = globalThis.setTimeout(callback, 0);
    return () => globalThis.clearTimeout(timeoutId);
  }

  let secondFrame = 0;
  const firstFrame = window.requestAnimationFrame(() => {
    secondFrame = window.requestAnimationFrame(callback);
  });

  return () => {
    window.cancelAnimationFrame(firstFrame);
    if (secondFrame !== 0) {
      window.cancelAnimationFrame(secondFrame);
    }
  };
}

export function usePostUpdatePreparing(
  isAppReady: boolean,
  options: PostUpdatePreparingOptions = {},
): boolean {
  const minDurationMs = options.minDurationMs ?? DEFAULT_MIN_DURATION_MS;
  const maxDurationMs = options.maxDurationMs ?? DEFAULT_MAX_DURATION_MS;
  const [isPreparing, setIsPreparing] = useState(
    () => readFreshPostUpdatePreparingMarker() !== null,
  );
  const mountedAt = useRef<number | null>(null);

  const finishPreparing = useCallback(() => {
    clearPostUpdatePreparing();
    setIsPreparing(false);
  }, []);

  useEffect(() => {
    if (isPreparing && mountedAt.current === null) {
      mountedAt.current = Date.now();
    }
  }, [isPreparing]);

  useEffect(() => {
    if (!isPreparing) {
      return;
    }

    const timeoutId = window.setTimeout(finishPreparing, maxDurationMs);
    return () => window.clearTimeout(timeoutId);
  }, [finishPreparing, isPreparing, maxDurationMs]);

  useEffect(() => {
    if (!isPreparing || !isAppReady) {
      return;
    }

    let cancelPaint: (() => void) | undefined;
    const elapsed = mountedAt.current === null ? 0 : Date.now() - mountedAt.current;
    const delayMs = Math.max(minDurationMs - elapsed, 0);
    const timeoutId = window.setTimeout(() => {
      cancelPaint = afterPaint(finishPreparing);
    }, delayMs);

    return () => {
      window.clearTimeout(timeoutId);
      cancelPaint?.();
    };
  }, [finishPreparing, isAppReady, isPreparing, minDurationMs]);

  return isPreparing;
}
