import { useCallback, useEffect, useRef } from "react";

import { recordChatScrollTrace } from "./scroll/diagnostics";

export interface ChatBottomInset {
  chromeRef: (element: HTMLElement | null) => void;
  containerRef: (element: HTMLElement | null) => void;
  registerTranscriptSpacer: (element: HTMLElement | null) => void;
}

function getBorderBoxHeight(entry: ResizeObserverEntry): number {
  const borderBoxSize = entry.borderBoxSize as
    | readonly ResizeObserverSize[]
    | ResizeObserverSize
    | undefined;
  const firstSize = Array.isArray(borderBoxSize)
    ? borderBoxSize[0]
    : borderBoxSize;

  return firstSize?.blockSize ?? entry.contentRect.height;
}

export function useChatBottomInset(): ChatBottomInset {
  const chromeElRef = useRef<HTMLElement | null>(null);
  const containerElRef = useRef<HTMLElement | null>(null);
  const spacerElRef = useRef<HTMLElement | null>(null);
  const resizeObserverRef = useRef<ResizeObserver | null>(null);
  const lastInsetPxRef = useRef(0);

  const writeInset = useCallback((insetPx: number) => {
    if (insetPx === lastInsetPxRef.current) {
      return;
    }

    const previousInsetPx = lastInsetPxRef.current;
    lastInsetPxRef.current = insetPx;
    recordChatScrollTrace({
      conversationId: null,
      source: "layout",
      event: "chrome-inset-write",
      element: null,
      detail: { previousInsetPx, insetPx },
    });
    // The transcript spacer sizes itself from the inherited custom property.
    // Writing its height here as well would leave the inset unreserved for a
    // frame every time the last item - which now owns the spacer - remounts.
    containerElRef.current?.style.setProperty(
      "--chat-bottom-inset",
      `${insetPx}px`,
    );
  }, []);

  const chromeRef = useCallback((element: HTMLElement | null) => {
    resizeObserverRef.current?.disconnect();
    resizeObserverRef.current = null;
    chromeElRef.current = element;

    if (!element || typeof ResizeObserver === "undefined") {
      return;
    }

    const observer = new ResizeObserver((entries) => {
      const entry = entries.find(({ target }) => target === element);
      if (!entry) {
        return;
      }
      writeInset(Math.ceil(getBorderBoxHeight(entry)));
    });
    observer.observe(element);
    resizeObserverRef.current = observer;
  }, [writeInset]);

  const containerRef = useCallback((element: HTMLElement | null) => {
    containerElRef.current = element;
    element?.style.setProperty(
      "--chat-bottom-inset",
      `${lastInsetPxRef.current}px`,
    );
  }, []);

  const registerTranscriptSpacer = useCallback(
    (element: HTMLElement | null) => {
      spacerElRef.current = element;
    },
    [],
  );

  useEffect(() => () => {
    resizeObserverRef.current?.disconnect();
    resizeObserverRef.current = null;
    chromeElRef.current = null;
    containerElRef.current = null;
    spacerElRef.current = null;
  }, []);

  return { chromeRef, containerRef, registerTranscriptSpacer };
}
