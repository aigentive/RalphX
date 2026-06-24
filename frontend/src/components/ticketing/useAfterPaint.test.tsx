import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAfterPaint } from "./useAfterPaint";

describe("useAfterPaint", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    // Drive requestAnimationFrame via the fake timer queue so the paint
    // boundary is deterministic in tests.
    vi.stubGlobal(
      "requestAnimationFrame",
      (cb: FrameRequestCallback): number =>
        setTimeout(() => cb(performance.now()), 0) as unknown as number,
    );
    vi.stubGlobal("cancelAnimationFrame", (handle: number): void => {
      clearTimeout(handle as unknown as ReturnType<typeof setTimeout>);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("stays false until a frame and macrotask have elapsed", () => {
    const { result } = renderHook(() => useAfterPaint(true));
    expect(result.current).toBe(false);

    act(() => {
      // First timer fires the rAF callback, second fires the setTimeout(0).
      vi.runAllTimers();
    });
    expect(result.current).toBe(true);
  });

  it("never becomes ready while inactive", () => {
    const { result } = renderHook(() => useAfterPaint(false));
    act(() => {
      vi.runAllTimers();
    });
    expect(result.current).toBe(false);
  });

  it("resets to false when toggled from active back to inactive", () => {
    const { result, rerender } = renderHook(({ active }) => useAfterPaint(active), {
      initialProps: { active: true },
    });
    act(() => {
      vi.runAllTimers();
    });
    expect(result.current).toBe(true);

    rerender({ active: false });
    expect(result.current).toBe(false);
  });

  it("cancels the pending frame when the effect is torn down before paint", () => {
    const { result, unmount } = renderHook(() => useAfterPaint(true));
    expect(result.current).toBe(false);
    unmount();
    // Running timers after unmount must not throw or set state on an unmounted hook.
    act(() => {
      vi.runAllTimers();
    });
    expect(result.current).toBe(false);
  });
});
