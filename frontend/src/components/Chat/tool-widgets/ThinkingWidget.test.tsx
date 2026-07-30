import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import { ThinkingWidget } from "./ThinkingWidget";

describe("ThinkingWidget", () => {
  let rafCallbacks: Array<FrameRequestCallback>;
  let originalRaf: typeof requestAnimationFrame;

  beforeEach(() => {
    vi.useFakeTimers();
    rafCallbacks = [];
    originalRaf = globalThis.requestAnimationFrame;
    globalThis.requestAnimationFrame = (cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    };
  });

  afterEach(() => {
    globalThis.requestAnimationFrame = originalRaf;
    vi.useRealTimers();
  });

  function flushHydration() {
    for (const cb of rafCallbacks.splice(0)) cb(performance.now());
    vi.advanceTimersByTime(0);
  }

  it("renders a shell placeholder before hydration", () => {
    render(<ThinkingWidget text="thinking content" />);
    expect(screen.getByTestId("thinking-widget-shell")).toBeInTheDocument();
    expect(screen.queryByTestId("thinking-scroll-body")).not.toBeInTheDocument();
  });

  it("hydrates content after rAF + setTimeout boundary", async () => {
    render(<ThinkingWidget text="thinking content" />);
    expect(screen.getByTestId("thinking-widget-shell")).toBeInTheDocument();

    await act(() => flushHydration());

    expect(screen.getByTestId("thinking-scroll-body")).toBeInTheDocument();
    expect(screen.getByText("thinking content")).toBeInTheDocument();
  });

  it("uses smaller font size when compact is true", async () => {
    render(<ThinkingWidget text="compact text" compact />);

    await act(() => flushHydration());

    const body = screen.getByTestId("thinking-scroll-body");
    expect(body.style.fontSize).toBe("10px");
  });

  it("uses default font size when compact is false", async () => {
    render(<ThinkingWidget text="normal text" />);

    await act(() => flushHydration());

    const body = screen.getByTestId("thinking-scroll-body");
    expect(body.style.fontSize).toBe("11px");
  });

  it("keeps scrolling contained within its own overflow node", async () => {
    render(<div data-testid="transcript-scroller"><ThinkingWidget text="scrollable thought" /></div>);
    await act(() => flushHydration());

    const transcript = screen.getByTestId("transcript-scroller");
    const body = screen.getByTestId("thinking-scroll-body");
    Object.defineProperties(body, {
      clientHeight: { configurable: true, value: 10 },
      scrollHeight: { configurable: true, value: 100 },
    });
    Object.defineProperty(transcript, "scrollTop", {
      configurable: true,
      get: () => 0,
      set: () => { throw new Error("thinking must not scroll the transcript"); },
    });

    fireEvent.scroll(body, { target: { scrollTop: 40 } });

    expect(body.scrollTop).toBe(40);
  });
});
