import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AgentsPublishProgressToast } from "./AgentsPublishProgressToast";

const { toastErrorMock, toastLoadingMock, toastSuccessMock } = vi.hoisted(() => ({
  toastErrorMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastSuccessMock: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    loading: (...args: unknown[]) => toastLoadingMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

describe("AgentsPublishProgressToast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);
    toastErrorMock.mockClear();
    toastLoadingMock.mockClear();
    toastSuccessMock.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("updates one persistent publish toast as pipeline status changes", () => {
    const { rerender } = render(
      <AgentsPublishProgressToast
        active
        conversationTitle="Checkout flow fix"
        conversationId="conversation-1"
        startedAtMs={10_000}
        status={null}
      />,
    );

    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Publishing workspace",
      expect.objectContaining({
        description: "Checkout flow fix • Check workspace • 0s",
        duration: Infinity,
        id: "agent-workspace-operation:conversation-1:publish",
      }),
    );

    vi.setSystemTime(12_000);
    rerender(
      <AgentsPublishProgressToast
        active
        conversationTitle="Checkout flow fix"
        conversationId="conversation-1"
        startedAtMs={10_000}
        status="pushing"
      />,
    );

    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Publishing workspace",
      expect.objectContaining({
        description: "Checkout flow fix • Push branch • 2s",
        duration: Infinity,
        id: "agent-workspace-operation:conversation-1:publish",
      }),
    );

    const loadingCallCount = toastLoadingMock.mock.calls.length;

    rerender(
      <AgentsPublishProgressToast
        active={false}
        conversationTitle="Checkout flow fix"
        conversationId="conversation-1"
        startedAtMs={10_000}
        status="pushing"
      />,
    );
    vi.advanceTimersByTime(1_000);

    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
  });
});
