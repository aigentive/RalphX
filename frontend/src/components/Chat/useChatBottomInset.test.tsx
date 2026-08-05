import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useChatBottomInset } from "./useChatBottomInset";

type ResizeObserverHarness = {
  callback: ResizeObserverCallback;
  disconnect: ReturnType<typeof vi.fn>;
  observe: ReturnType<typeof vi.fn>;
};

const observers: ResizeObserverHarness[] = [];

function TestHarness({
  onRender,
  withSpacer = true,
}: {
  onRender?: () => void;
  withSpacer?: boolean;
}) {
  onRender?.();
  const { chromeRef, containerRef, registerTranscriptSpacer } =
    useChatBottomInset();

  return (
    <div ref={containerRef} data-testid="container">
      <div ref={chromeRef} data-testid="chrome" />
      {withSpacer ? (
        <div ref={registerTranscriptSpacer} data-testid="spacer" />
      ) : null}
    </div>
  );
}

function resizeChrome(height: number): void {
  const observer = observers.at(-1);
  if (!observer) throw new Error("ResizeObserver was not created");

  const chrome = document.querySelector<HTMLElement>("[data-testid='chrome']");
  if (!chrome) throw new Error("Chrome element was not rendered");

  act(() => {
    observer.callback(
      [
        {
          target: chrome,
          borderBoxSize: [{ blockSize: height, inlineSize: 100 }],
          contentRect: { height },
        } as unknown as ResizeObserverEntry,
      ],
      {} as ResizeObserver,
    );
  });
}

describe("useChatBottomInset", () => {
  beforeEach(() => {
    observers.length = 0;
    vi.stubGlobal(
      "ResizeObserver",
      class MockResizeObserver {
        readonly harness: ResizeObserverHarness;

        constructor(callback: ResizeObserverCallback) {
          this.harness = {
            callback,
            disconnect: vi.fn(),
            observe: vi.fn(),
          };
          observers.push(this.harness);
        }

        observe = (...args: Parameters<ResizeObserver["observe"]>) =>
          this.harness.observe(...args);
        disconnect = () => this.harness.disconnect();
        unobserve = vi.fn();
      },
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("writes the measured chrome height to the spacer and container variable", () => {
    const { getByTestId } = render(<TestHarness />);

    resizeChrome(72.2);

    expect(getByTestId("spacer")).toHaveStyle({ height: "73px" });
    expect(getByTestId("container").style.getPropertyValue("--chat-bottom-inset"))
      .toBe("73px");
  });

  it("ignores identical measurements and applies the last inset to a remounted spacer", () => {
    const { getByTestId, rerender } = render(<TestHarness />);
    resizeChrome(64);
    const firstSpacer = getByTestId("spacer");
    const heightSetter = vi.spyOn(firstSpacer.style, "height", "set");

    resizeChrome(64);
    expect(heightSetter).not.toHaveBeenCalled();

    rerender(<TestHarness withSpacer={false} />);
    rerender(<TestHarness />);
    expect(getByTestId("spacer")).toHaveStyle({ height: "64px" });
  });

  it("updates the container safely while the transcript spacer is absent", () => {
    const { getByTestId } = render(<TestHarness withSpacer={false} />);

    expect(() => resizeChrome(48)).not.toThrow();
    expect(getByTestId("container").style.getPropertyValue("--chat-bottom-inset"))
      .toBe("48px");
  });

  it("writes resize changes without triggering a React render", () => {
    const onRender = vi.fn();
    render(<TestHarness onRender={onRender} />);

    resizeChrome(56);

    expect(onRender).toHaveBeenCalledTimes(1);
  });

  it("no-ops safely when ResizeObserver is unavailable", () => {
    vi.stubGlobal("ResizeObserver", undefined);

    expect(() => render(<TestHarness />)).not.toThrow();
  });
});
