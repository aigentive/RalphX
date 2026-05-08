import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type RefObject,
} from "react";

interface PersistentSidebarResizeOptions {
  maxWidth: number;
  minWidth: number;
  storageKey: string;
}

function clampSidebarWidth(width: number, minWidth: number, maxWidth: number): number {
  return Math.max(minWidth, Math.min(maxWidth, width));
}

function loadStoredSidebarWidth({
  maxWidth,
  minWidth,
  storageKey,
}: PersistentSidebarResizeOptions): number | null {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    const saved = window.localStorage.getItem(storageKey);
    if (!saved) {
      return null;
    }
    const parsed = Number.parseInt(saved, 10);
    if (!Number.isFinite(parsed)) {
      return null;
    }
    return clampSidebarWidth(parsed, minWidth, maxWidth);
  } catch {
    return null;
  }
}

export function usePersistentSidebarResize(
  sidebarRef: RefObject<HTMLDivElement | null>,
  options: PersistentSidebarResizeOptions,
) {
  const { maxWidth, minWidth, storageKey } = options;
  const [userSidebarWidth, setUserSidebarWidth] = useState<number | null>(() =>
    loadStoredSidebarWidth(options),
  );
  const [isSidebarResizing, setIsSidebarResizing] = useState(false);
  const sidebarResizeFrameRef = useRef<number | null>(null);
  const pendingSidebarWidthRef = useRef<number | null>(null);
  const sidebarResizeBoundsRef = useRef<{ left: number } | null>(null);

  const handleSidebarResizeStart = useCallback(
    (event: ReactMouseEvent) => {
      event.preventDefault();
      const sidebar = sidebarRef.current;
      if (sidebar) {
        const rect = sidebar.getBoundingClientRect();
        sidebarResizeBoundsRef.current = { left: rect.left };
      } else {
        sidebarResizeBoundsRef.current = null;
      }
      pendingSidebarWidthRef.current = null;
      setIsSidebarResizing(true);
    },
    [sidebarRef],
  );

  const handleSidebarResizeReset = useCallback((event: ReactMouseEvent) => {
    event.preventDefault();
    if (sidebarResizeFrameRef.current !== null) {
      window.cancelAnimationFrame(sidebarResizeFrameRef.current);
      sidebarResizeFrameRef.current = null;
    }
    pendingSidebarWidthRef.current = null;
    sidebarResizeBoundsRef.current = null;
    setUserSidebarWidth(null);
  }, []);

  const flushPendingSidebarWidth = useCallback(() => {
    if (sidebarResizeFrameRef.current !== null) {
      window.cancelAnimationFrame(sidebarResizeFrameRef.current);
      sidebarResizeFrameRef.current = null;
    }
    const pending = pendingSidebarWidthRef.current;
    pendingSidebarWidthRef.current = null;
    if (pending !== null) {
      setUserSidebarWidth(pending);
    }
  }, []);

  const scheduleSidebarWidth = useCallback((nextWidth: number) => {
    pendingSidebarWidthRef.current = nextWidth;
    if (sidebarResizeFrameRef.current !== null) {
      return;
    }
    sidebarResizeFrameRef.current = window.requestAnimationFrame(() => {
      sidebarResizeFrameRef.current = null;
      const pending = pendingSidebarWidthRef.current;
      pendingSidebarWidthRef.current = null;
      if (pending !== null) {
        setUserSidebarWidth(pending);
      }
    });
  }, []);

  useEffect(
    () => () => {
      if (sidebarResizeFrameRef.current !== null) {
        window.cancelAnimationFrame(sidebarResizeFrameRef.current);
      }
    },
    [],
  );

  useEffect(() => {
    if (!isSidebarResizing) {
      return;
    }

    const handleMouseMove = (event: MouseEvent) => {
      const sidebar = sidebarRef.current;
      if (!sidebar) {
        return;
      }
      const bounds =
        sidebarResizeBoundsRef.current ??
        (() => {
          const rect = sidebar.getBoundingClientRect();
          const next = { left: rect.left };
          sidebarResizeBoundsRef.current = next;
          return next;
        })();
      const nextWidth = event.clientX - bounds.left;
      scheduleSidebarWidth(clampSidebarWidth(nextWidth, minWidth, maxWidth));
    };

    const handleMouseUp = () => {
      flushPendingSidebarWidth();
      sidebarResizeBoundsRef.current = null;
      setIsSidebarResizing(false);
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, [
    flushPendingSidebarWidth,
    isSidebarResizing,
    maxWidth,
    minWidth,
    scheduleSidebarWidth,
    sidebarRef,
  ]);

  useEffect(() => {
    try {
      if (userSidebarWidth !== null) {
        window.localStorage.setItem(storageKey, String(userSidebarWidth));
        return;
      }
      window.localStorage.removeItem(storageKey);
    } catch {
      // Ignore unavailable storage.
    }
  }, [storageKey, userSidebarWidth]);

  return {
    handleSidebarResizeReset,
    handleSidebarResizeStart,
    isSidebarResizing,
    userSidebarWidth,
  };
}
