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

  it("writes the measured chrome height to the container variable only", () => {
    const { getByTestId } = render(<TestHarness />);

    resizeChrome(72.2);

    expect(getByTestId("container").style.getPropertyValue("--chat-bottom-inset"))
      .toBe("73px");
  });

  // The spacer now lives inside the last transcript item and sizes itself from
  // the inherited custom property, so the hook must never be a second writer of
  // its height - that would leave the inset unreserved whenever the item that
  // owns the spacer remounts on append.
  it("never writes the registered spacer's height", () => {
    const { getByTestId } = render(<TestHarness />);
    const heightSetter = vi.spyOn(getByTestId("spacer").style, "height", "set");

    resizeChrome(64);
    resizeChrome(80);

    expect(heightSetter).not.toHaveBeenCalled();
    expect(getByTestId("spacer").style.height).toBe("");
  });

  it("ignores identical measurements", () => {
    const { getByTestId } = render(<TestHarness />);
    resizeChrome(64);
    const container = getByTestId("container");
    const propertySetter = vi.spyOn(container.style, "setProperty");

    resizeChrome(64);

    expect(propertySetter).not.toHaveBeenCalled();
    expect(container.style.getPropertyValue("--chat-bottom-inset")).toBe("64px");
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
