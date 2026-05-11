import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { markPostUpdatePreparing } from "@/lib/postUpdatePreparing";
import { usePostUpdatePreparing } from "./usePostUpdatePreparing";

describe("usePostUpdatePreparing", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    localStorage.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
    localStorage.clear();
  });

  it("waits for two animation frames before finishing after readiness", async () => {
    const requestAnimationFrameDescriptor = Object.getOwnPropertyDescriptor(
      window,
      "requestAnimationFrame",
    );
    const cancelAnimationFrameDescriptor = Object.getOwnPropertyDescriptor(
      window,
      "cancelAnimationFrame",
    );
    const frameCallbacks: FrameRequestCallback[] = [];
    const cancelAnimationFrame = vi.fn();
    Object.defineProperty(window, "requestAnimationFrame", {
      configurable: true,
      value: vi.fn((callback: FrameRequestCallback) => {
        frameCallbacks.push(callback);
        return frameCallbacks.length;
      }),
    });
    Object.defineProperty(window, "cancelAnimationFrame", {
      configurable: true,
      value: cancelAnimationFrame,
    });

    try {
      markPostUpdatePreparing("0.12.3");

      const { result } = renderHook(() =>
        usePostUpdatePreparing(true, { minDurationMs: 0, maxDurationMs: 1_000 }),
      );

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });

      expect(result.current).toBe(true);
      expect(frameCallbacks).toHaveLength(1);

      act(() => {
        frameCallbacks.shift()?.(0);
      });

      expect(result.current).toBe(true);
      expect(frameCallbacks).toHaveLength(1);

      act(() => {
        frameCallbacks.shift()?.(16);
      });

      expect(result.current).toBe(false);
      expect(cancelAnimationFrame).toHaveBeenCalled();
    } finally {
      if (requestAnimationFrameDescriptor) {
        Object.defineProperty(window, "requestAnimationFrame", requestAnimationFrameDescriptor);
      }
      if (cancelAnimationFrameDescriptor) {
        Object.defineProperty(window, "cancelAnimationFrame", cancelAnimationFrameDescriptor);
      }
    }
  });

  it("finishes after a macrotask when requestAnimationFrame is unavailable", async () => {
    const requestAnimationFrameDescriptor = Object.getOwnPropertyDescriptor(
      window,
      "requestAnimationFrame",
    );
    const cancelAnimationFrameDescriptor = Object.getOwnPropertyDescriptor(
      window,
      "cancelAnimationFrame",
    );
    Object.defineProperty(window, "requestAnimationFrame", {
      configurable: true,
      value: undefined,
    });
    Object.defineProperty(window, "cancelAnimationFrame", {
      configurable: true,
      value: undefined,
    });

    try {
      markPostUpdatePreparing("0.12.3");

      const { result } = renderHook(() =>
        usePostUpdatePreparing(true, { minDurationMs: 0, maxDurationMs: 1_000 }),
      );

      expect(result.current).toBe(true);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });

      expect(result.current).toBe(false);
    } finally {
      if (requestAnimationFrameDescriptor) {
        Object.defineProperty(window, "requestAnimationFrame", requestAnimationFrameDescriptor);
      }
      if (cancelAnimationFrameDescriptor) {
        Object.defineProperty(window, "cancelAnimationFrame", cancelAnimationFrameDescriptor);
      }
    }
  });
});
