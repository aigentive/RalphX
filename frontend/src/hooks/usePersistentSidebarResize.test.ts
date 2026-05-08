import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RefObject, MouseEvent as ReactMouseEvent } from "react";

import { usePersistentSidebarResize } from "./usePersistentSidebarResize";

const STORAGE_KEY = "test-sidebar-width";

function createMouseEvent(): ReactMouseEvent {
  return {
    preventDefault: vi.fn(),
  } as unknown as ReactMouseEvent;
}

function createSidebar(left: number): HTMLDivElement {
  const sidebar = document.createElement("div");
  vi.spyOn(sidebar, "getBoundingClientRect").mockReturnValue({
    bottom: 720,
    height: 720,
    left,
    right: left + 340,
    top: 0,
    width: 340,
    x: left,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect);
  return sidebar;
}

function renderResizeHook(sidebarRef: RefObject<HTMLDivElement | null>) {
  return renderHook(() =>
    usePersistentSidebarResize(sidebarRef, {
      maxWidth: 520,
      minWidth: 220,
      storageKey: STORAGE_KEY,
    }),
  );
}

type RafCallback = FrameRequestCallback;

let rafCallbacks: Map<number, RafCallback>;
let nextRafId: number;
let cancelAnimationFrameSpy: ReturnType<typeof vi.spyOn>;

function flushAnimationFrames() {
  const callbacks = Array.from(rafCallbacks.entries());
  rafCallbacks.clear();
  callbacks.forEach(([id, callback]) => {
    callback(id);
  });
}

beforeEach(() => {
  localStorage.clear();
  rafCallbacks = new Map();
  nextRafId = 1;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    const id = nextRafId;
    nextRafId += 1;
    rafCallbacks.set(id, callback);
    return id;
  });
  cancelAnimationFrameSpy = vi
    .spyOn(window, "cancelAnimationFrame")
    .mockImplementation((id) => {
      rafCallbacks.delete(id);
    });
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("usePersistentSidebarResize", () => {
  it("loads and clamps persisted widths", () => {
    const sidebarRef = { current: createSidebar(0) };

    localStorage.setItem(STORAGE_KEY, "900");
    const oversized = renderResizeHook(sidebarRef);
    expect(oversized.result.current.userSidebarWidth).toBe(520);
    oversized.unmount();

    localStorage.setItem(STORAGE_KEY, "120");
    const undersized = renderResizeHook(sidebarRef);
    expect(undersized.result.current.userSidebarWidth).toBe(220);
    undersized.unmount();

    localStorage.setItem(STORAGE_KEY, "not-a-width");
    const malformed = renderResizeHook(sidebarRef);
    expect(malformed.result.current.userSidebarWidth).toBeNull();
    malformed.unmount();
  });

  it("ignores unavailable localStorage while loading", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("storage unavailable");
    });

    const sidebarRef = { current: createSidebar(0) };
    const { result } = renderResizeHook(sidebarRef);

    expect(result.current.userSidebarWidth).toBeNull();
  });

  it("throttles drag updates, clamps width, persists width, and resets", () => {
    const sidebarRef = { current: createSidebar(10) };
    const { result } = renderResizeHook(sidebarRef);

    act(() => {
      result.current.handleSidebarResizeStart(createMouseEvent());
    });

    expect(result.current.isSidebarResizing).toBe(true);

    act(() => {
      document.dispatchEvent(new MouseEvent("mousemove", { clientX: 150 }));
      document.dispatchEvent(new MouseEvent("mousemove", { clientX: 900 }));
    });

    expect(result.current.userSidebarWidth).toBeNull();

    act(() => {
      flushAnimationFrames();
    });

    expect(result.current.userSidebarWidth).toBe(520);
    expect(localStorage.getItem(STORAGE_KEY)).toBe("520");

    act(() => {
      document.dispatchEvent(new MouseEvent("mouseup"));
    });

    expect(result.current.isSidebarResizing).toBe(false);

    act(() => {
      result.current.handleSidebarResizeReset(createMouseEvent());
    });

    expect(result.current.userSidebarWidth).toBeNull();
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull();
  });

  it("falls back to measuring the sidebar when it appears after drag start", () => {
    const sidebarRef: RefObject<HTMLDivElement | null> = { current: null };
    const { result } = renderResizeHook(sidebarRef);

    act(() => {
      result.current.handleSidebarResizeStart(createMouseEvent());
    });

    sidebarRef.current = createSidebar(25);

    act(() => {
      document.dispatchEvent(new MouseEvent("mousemove", { clientX: 340 }));
      document.dispatchEvent(new MouseEvent("mouseup"));
    });

    expect(result.current.userSidebarWidth).toBe(315);
    expect(result.current.isSidebarResizing).toBe(false);
  });

  it("ignores drag movement while no sidebar element is mounted", () => {
    const sidebarRef: RefObject<HTMLDivElement | null> = { current: null };
    const { result } = renderResizeHook(sidebarRef);

    act(() => {
      result.current.handleSidebarResizeStart(createMouseEvent());
    });
    act(() => {
      document.dispatchEvent(new MouseEvent("mousemove", { clientX: 340 }));
      document.dispatchEvent(new MouseEvent("mouseup"));
    });

    expect(result.current.userSidebarWidth).toBeNull();
    expect(result.current.isSidebarResizing).toBe(false);
  });

  it("cancels pending animation frames on reset and unmount", () => {
    const sidebarRef = { current: createSidebar(0) };
    const { result, unmount } = renderResizeHook(sidebarRef);

    act(() => {
      result.current.handleSidebarResizeStart(createMouseEvent());
    });
    act(() => {
      document.dispatchEvent(new MouseEvent("mousemove", { clientX: 360 }));
    });

    expect(rafCallbacks.size).toBe(1);

    act(() => {
      result.current.handleSidebarResizeReset(createMouseEvent());
    });

    expect(cancelAnimationFrameSpy).toHaveBeenCalledTimes(1);
    expect(rafCallbacks.size).toBe(0);

    act(() => {
      result.current.handleSidebarResizeStart(createMouseEvent());
    });
    act(() => {
      document.dispatchEvent(new MouseEvent("mousemove", { clientX: 420 }));
    });

    expect(rafCallbacks.size).toBe(1);

    unmount();

    expect(cancelAnimationFrameSpy).toHaveBeenCalledTimes(2);
    expect(rafCallbacks.size).toBe(0);
  });
});
