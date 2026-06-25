import { useEffect, useState } from "react";

export function useAfterPaint(active: boolean): boolean {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (!active) {
      setReady(false);
      return undefined;
    }

    let timeoutId: ReturnType<typeof setTimeout> | null = null;
    const frameId = window.requestAnimationFrame(() => {
      timeoutId = setTimeout(() => setReady(true), 0);
    });

    return () => {
      window.cancelAnimationFrame(frameId);
      if (timeoutId !== null) {
        clearTimeout(timeoutId);
      }
    };
  }, [active]);

  return ready;
}
