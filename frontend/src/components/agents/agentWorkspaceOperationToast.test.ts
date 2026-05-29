import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  agentWorkspaceOperationToastId,
  publishPipelineToastLabel,
  startAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";

const { toastErrorMock, toastInfoMock, toastLoadingMock, toastSuccessMock } = vi.hoisted(() => ({
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastSuccessMock: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    loading: (...args: unknown[]) => toastLoadingMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

describe("agentWorkspaceOperationToast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    toastErrorMock.mockClear();
    toastInfoMock.mockClear();
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
      conversationTitle: "Agent conversation",
      detail: "From main",
      id: agentWorkspaceOperationToastId("conversation-1", "update-from-base"),
      title: "Updating branch",
    });

    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Updating branch",
      expect.objectContaining({
        description: "Agent conversation • From main • 0s",
        duration: Infinity,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      }),
    );

    vi.advanceTimersByTime(5_000);

    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Updating branch",
      expect.objectContaining({
        description: "Agent conversation • From main • 5s",
        duration: Infinity,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      }),
    );

    progress.success("Updated from main");

    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Updated from main",
      {
        description: "Agent conversation • From main",
        duration: 8_000,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      },
    );
    const loadingCallCount = toastLoadingMock.mock.calls.length;

    vi.advanceTimersByTime(1_000);

    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
  });

  it("updates the persistent toast before ignoring updates after settlement", () => {
    const progress = startAgentWorkspaceOperationToast({
      conversationTitle: "Agent conversation",
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
      "Publishing workspace",
      expect.objectContaining({
        description: "Agent conversation • Push branch • 2s",
        duration: Infinity,
        id: "agent-workspace-operation:conversation-2:publish",
      }),
    );

    const loadingCallCount = toastLoadingMock.mock.calls.length;

    progress.success("Published branch");
    progress.update({ detail: "Open draft PR" });

    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Published branch",
      {
        description: "Agent conversation • Push branch",
        duration: 8_000,
        id: "agent-workspace-operation:conversation-2:publish",
      },
    );
    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
  });

  it("replaces the persistent loading toast with an error result", () => {
    const progress = startAgentWorkspaceOperationToast({
      conversationTitle: "Agent conversation",
      id: agentWorkspaceOperationToastId("conversation-1", "rebase"),
      title: "Rebasing branch",
    });

    progress.error("Rebase failed", { detail: "Merge conflicts detected" });

    expect(toastErrorMock).toHaveBeenCalledWith(
      "Rebase failed",
      {
        closeButton: true,
        description: "Agent conversation • Merge conflicts detected",
        dismissible: true,
        duration: 12_000,
        id: "agent-workspace-operation:conversation-1:rebase",
      },
    );
  });

  it("allows manual dismissal without the elapsed timer resurrecting the loading toast", () => {
    const progress = startAgentWorkspaceOperationToast({
      conversationTitle: "Agent conversation",
      detail: "From main",
      id: agentWorkspaceOperationToastId("conversation-1", "update-from-base"),
      title: "Updating branch",
    });

    const loadingOptions = toastLoadingMock.mock.calls.at(-1)?.[1] as
      | {
          closeButton?: boolean;
          dismissible?: boolean;
          onDismiss?: () => void;
        }
      | undefined;

    expect(loadingOptions).toEqual(
      expect.objectContaining({
        closeButton: true,
        dismissible: true,
      }),
    );

    loadingOptions?.onDismiss?.();
    const loadingCallCount = toastLoadingMock.mock.calls.length;

    vi.advanceTimersByTime(3_000);

    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);

    progress.error("Update failed", { detail: "Merge conflicts detected" });

    expect(toastErrorMock).toHaveBeenCalledWith(
      "Update failed",
      {
        closeButton: true,
        description: "Agent conversation • Merge conflicts detected",
        dismissible: true,
        duration: 12_000,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      },
    );
  });

  it("replaces the persistent loading toast with an auto-dismissing info result", () => {
    const progress = startAgentWorkspaceOperationToast({
      conversationTitle: "Agent conversation",
      id: agentWorkspaceOperationToastId("conversation-1", "update-from-base"),
      title: "Updating branch",
    });

    progress.info("Repair started", { detail: "Merge conflicts detected" });

    expect(toastInfoMock).toHaveBeenCalledWith(
      "Repair started",
      {
        description: "Agent conversation • Merge conflicts detected",
        dismissible: true,
        duration: 8_000,
        id: "agent-workspace-operation:conversation-1:update-from-base",
      },
    );
  });
});
