import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAfterPaintMounted } from "./agentDeferredFrame";

function DeferredFrameHarness({ isVisible }: { isVisible: boolean }) {
  const isMounted = useAfterPaintMounted(isVisible);

  return <span data-testid="deferred-frame-state">{isMounted ? "mounted" : "deferred"}</span>;
}

describe("useAfterPaintMounted", () => {
  const frameCallbacks = new Map<number, FrameRequestCallback>();
  let nextFrameId = 1;

  beforeEach(() => {
    frameCallbacks.clear();
    nextFrameId = 1;
    vi.useFakeTimers();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      const frameId = nextFrameId++;
      frameCallbacks.set(frameId, callback);
      return frameId;
    });
    vi.stubGlobal("cancelAnimationFrame", (frameId: number) => {
      frameCallbacks.delete(frameId);
    });
  });

  afterEach(() => {
    frameCallbacks.clear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("cancels pending hydration when the frame implementation changes before cleanup", () => {
    const { rerender } = render(<DeferredFrameHarness isVisible />);
    expect(screen.getByTestId("deferred-frame-state")).toHaveTextContent("deferred");

    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    rerender(<DeferredFrameHarness isVisible={false} />);

    act(() => {
      for (const [frameId, callback] of frameCallbacks) {
        frameCallbacks.delete(frameId);
        callback(performance.now());
      }
      vi.runAllTimers();
    });
    const stateAfterStaleFrame = screen.getByTestId("deferred-frame-state").textContent;

    act(() => {
      for (const [frameId, callback] of frameCallbacks) {
        frameCallbacks.delete(frameId);
        callback(performance.now());
      }
      vi.runAllTimers();
    });

    expect(stateAfterStaleFrame).toBe("deferred");
  });
});
