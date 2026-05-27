import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  agentWorkspaceOperationToastId,
  publishPipelineToastLabel,
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

  it.each([
    ["committing", "Commit changes"],
    ["refreshing", "Refresh branch"],
    ["refreshed", "Refresh branch"],
    ["describing", "Draft PR description"],
    ["pushing", "Push branch"],
    ["pushed", "Open draft PR"],
    ["published", "Open draft PR"],
    ["description_failed", "PR description failed"],
    ["needs_agent", "Repair needed"],
    ["failed", "Publish failed"],
    [null, "Check workspace"],
    ["unknown", "Check workspace"],
  ])("maps publish status %s to %s", (status, label) => {
    expect(publishPipelineToastLabel(status)).toBe(label);
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

  it("updates the persistent toast before ignoring updates after settlement", () => {
    const progress = startAgentWorkspaceOperationToast({
      detail: "Check workspace",
      id: agentWorkspaceOperationToastId("conversation-1", "publish"),
      startedAtMs: 0,
      title: "Publishing workspace",
    });

    vi.setSystemTime(3_000);
    progress.update({
      detail: "Push branch",
      id: agentWorkspaceOperationToastId("conversation-2", "publish"),
      startedAtMs: 1_000,
    });

    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Publishing workspace - Push branch - 2s",
      {
        duration: Infinity,
        id: "agent-workspace-operation:conversation-2:publish",
      },
    );

    const loadingCallCount = toastLoadingMock.mock.calls.length;

    progress.success("Published branch");
    progress.update({ detail: "Open draft PR" });

    expect(toastSuccessMock).toHaveBeenCalledWith("Published branch", {
      id: "agent-workspace-operation:conversation-2:publish",
    });
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
