import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AgentsPublishProgressToast } from "./AgentsPublishProgressToast";

const {
  toastDismissMock,
  toastErrorMock,
  toastLoadingMock,
  toastSuccessMock,
} = vi.hoisted(() => ({
  toastDismissMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastSuccessMock: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    dismiss: (...args: unknown[]) => toastDismissMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    loading: (...args: unknown[]) => toastLoadingMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

describe("AgentsPublishProgressToast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);
    toastDismissMock.mockClear();
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

    expect(toastDismissMock).toHaveBeenCalledWith(
      "agent-workspace-operation:conversation-1:publish",
    );
    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
  });

  it("does not update toast title when switching to a different conversation", () => {
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
        id: "agent-workspace-operation:conversation-1:publish",
      }),
    );

    const callCountAfterCreate = toastLoadingMock.mock.calls.length;

    rerender(
      <AgentsPublishProgressToast
        active
        conversationTitle="Other conversation"
        conversationId="conversation-2"
        startedAtMs={10_000}
        status="pushing"
      />,
    );

    expect(toastLoadingMock).toHaveBeenCalledTimes(callCountAfterCreate);

    const allDescriptions = toastLoadingMock.mock.calls.map(
      (call: unknown[]) => (call[1] as { description?: string })?.description ?? "",
    );
    for (const desc of allDescriptions) {
      expect(desc).not.toContain("Other conversation");
    }
  });

  it("dismisses toast correctly after conversation switch", () => {
    const { rerender } = render(
      <AgentsPublishProgressToast
        active
        conversationTitle="Checkout flow fix"
        conversationId="conversation-1"
        startedAtMs={10_000}
        status={null}
      />,
    );

    expect(toastLoadingMock).toHaveBeenCalled();

    rerender(
      <AgentsPublishProgressToast
        active
        conversationTitle="Other conversation"
        conversationId="conversation-2"
        startedAtMs={10_000}
        status="pushing"
      />,
    );

    rerender(
      <AgentsPublishProgressToast
        active={false}
        conversationTitle="Other conversation"
        conversationId="conversation-2"
        startedAtMs={10_000}
        status="pushing"
      />,
    );

    expect(toastDismissMock).toHaveBeenCalledWith(
      "agent-workspace-operation:conversation-1:publish",
    );
  });
});
