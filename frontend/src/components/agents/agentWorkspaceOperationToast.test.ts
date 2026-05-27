import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  agentWorkspaceOperationToastId,
  startAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";

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

describe("agentWorkspaceOperationToast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    toastErrorMock.mockClear();
    toastLoadingMock.mockClear();
    toastSuccessMock.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps one persistent loading toast updated with elapsed time", () => {
    const progress = startAgentWorkspaceOperationToast({
      detail: "From main",
      id: agentWorkspaceOperationToastId("conversation-1", "update-from-base"),
      title: "Updating branch",
    });

    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Updating branch - From main - 0s",
      {
        duration: Infinity,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      },
    );

    vi.advanceTimersByTime(5_000);

    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Updating branch - From main - 5s",
      {
        duration: Infinity,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      },
    );

    progress.success("Updated from main");

    expect(toastSuccessMock).toHaveBeenCalledWith("Updated from main", {
      id: "agent-workspace-operation:conversation-1:update-from-base",
    });
    const loadingCallCount = toastLoadingMock.mock.calls.length;

    vi.advanceTimersByTime(1_000);

    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
  });

  it("replaces the persistent loading toast with an error result", () => {
    const progress = startAgentWorkspaceOperationToast({
      id: agentWorkspaceOperationToastId("conversation-1", "rebase"),
      title: "Rebasing branch",
    });

    progress.error("Rebase failed");

    expect(toastErrorMock).toHaveBeenCalledWith("Rebase failed", {
      id: "agent-workspace-operation:conversation-1:rebase",
    });
  });
});
