import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  agentWorkspaceOperationErrorDetail,
  agentWorkspaceMaintenanceOperationToastId,
  agentWorkspaceOperationResultDetail,
  agentWorkspaceOperationToastId,
  isAgentWorkspaceOperationToastDismissed,
  publishPipelineToastLabel,
  maintenanceOperationToastLabel,
  resetAgentWorkspaceOperationToastStateForTests,
  startAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";

const {
  toastDismissMock,
  toastErrorMock,
  toastInfoMock,
  toastLoadingMock,
  toastSuccessMock,
  navigateToAgentConversationMock,
  visibleAgentScopeMock,
} = vi.hoisted(() => ({
  toastDismissMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  navigateToAgentConversationMock: vi.fn(),
  visibleAgentScopeMock: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    dismiss: (...args: unknown[]) => toastDismissMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    loading: (...args: unknown[]) => toastLoadingMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

vi.mock("@/lib/navigation", () => ({
  navigateToAgentConversation: (...args: unknown[]) =>
    navigateToAgentConversationMock(...args),
}));

vi.mock("@/stores/agentSessionStore", () => ({
  useAgentSessionStore: {
    getState: () => ({ visibleAgentScope: visibleAgentScopeMock() }),
  },
}));

describe("agentWorkspaceOperationToast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    toastDismissMock.mockClear();
    toastErrorMock.mockClear();
    toastInfoMock.mockClear();
    toastLoadingMock.mockClear();
    toastSuccessMock.mockClear();
    navigateToAgentConversationMock.mockClear();
    visibleAgentScopeMock.mockReturnValue(null);
  });

  afterEach(() => {
    resetAgentWorkspaceOperationToastStateForTests();
    vi.useRealTimers();
  });

  it("gives local commits their own stable operation toast identity", () => {
    expect(agentWorkspaceOperationToastId("conversation-1", "local-commit")).toBe(
      "agent-workspace-operation:conversation-1:local-commit",
    );
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

  it("uses one durable toast id and stage label for a maintenance operation", () => {
    expect(
      agentWorkspaceMaintenanceOperationToastId("conversation-1", "operation-1"),
    ).toBe("agent-workspace-maintenance:conversation-1:operation-1");
    expect(maintenanceOperationToastLabel("repairing")).toBe("Repairing workspace");
    expect(maintenanceOperationToastLabel("ready")).toBe(
      "Base updated — ready to publish",
    );
    expect(maintenanceOperationToastLabel("blocked")).toBe("Repair blocked");
    expect(maintenanceOperationToastLabel("future-stage")).toBe(
      "Continuing workspace operation",
    );
  });

  it("strips raw agent output from operation error details", () => {
    expect(
      agentWorkspaceOperationErrorDetail(
        new Error(
          "\u001B[31mPR describer agent completed without submitting a PR description.\u001B[0m Raw output: ## Summary\n\n" +
            "A".repeat(2_000),
        ),
        "Failed to publish branch",
      ),
    ).toBe("PR describer agent completed without submitting a PR description.");
  });

  it("compacts long operation error details", () => {
    const detail = agentWorkspaceOperationErrorDetail(
      "Publish failed: " + "x".repeat(500),
      "Failed to publish branch",
    );

    expect(detail).toHaveLength(240);
    expect(detail.endsWith("...")).toBe(true);
  });

  it("keeps terminal result details short enough for toast descriptions", () => {
    const detail = agentWorkspaceOperationResultDetail(
      [
        "\u001B[1mGuard 1: pre-commit design token guards\u001B[0m",
        "src/components/ui/notice-banner.tsx:21: backgroundColor: var(--status-warning-muted, rgba(224, 179, 65, 0.1))",
        "src/components/automations/automationRunView.ts:497: backgroundColor: var(--status-success-muted, rgba(63, 191, 127, 0.08))",
      ].join("\n"),
    );

    expect(detail).toBe("Full output is available in the workspace.");
  });

  it("preserves concise terminal result details", () => {
    expect(agentWorkspaceOperationResultDetail("Typecheck failed")).toBe(
      "Typecheck failed",
    );
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

  it("adds no action to an operation toast without a target conversation", () => {
    startAgentWorkspaceOperationToast({
      id: agentWorkspaceOperationToastId("conversation-1", "update-from-base"),
      title: "Updating branch",
    });

    const loadingOptions = toastLoadingMock.mock.calls.at(-1)?.[1] as
      | { action?: unknown }
      | undefined;
    expect(loadingOptions?.action).toBeUndefined();
  });

  it("opens the target conversation from loading and result toast actions", () => {
    const progress = startAgentWorkspaceOperationToast({
      id: agentWorkspaceOperationToastId("conversation-1", "publish"),
      targetConversation: {
        conversationId: "conversation-1",
        projectId: "project-1",
      },
      title: "Publishing workspace",
    });

    const loadingOptions = toastLoadingMock.mock.calls.at(-1)?.[1] as
      | { action?: { label?: string; onClick?: () => void } }
      | undefined;
    expect(loadingOptions?.action).toEqual(
      expect.objectContaining({ label: "Open conversation" }),
    );
    loadingOptions?.action?.onClick?.();
    expect(navigateToAgentConversationMock).toHaveBeenCalledWith(
      "project-1",
      "conversation-1",
    );

    progress.success("Published branch");
    const resultOptions = toastSuccessMock.mock.calls.at(-1)?.[1] as
      | { action?: { label?: string; onClick?: () => void } }
      | undefined;
    expect(resultOptions?.action).toEqual(
      expect.objectContaining({ label: "Open conversation" }),
    );
    resultOptions?.action?.onClick?.();
    expect(navigateToAgentConversationMock).toHaveBeenLastCalledWith(
      "project-1",
      "conversation-1",
    );
  });

  it("truncates verbose progress detail without truncating terminal detail", () => {
    startAgentWorkspaceOperationToast({
      conversationTitle: "Agent conversation",
      detail: "x".repeat(100),
      id: agentWorkspaceOperationToastId("conversation-1", "update-from-base"),
      title: "Updating branch",
    });

    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Updating branch",
      expect.objectContaining({
        description: `Agent conversation • ${"x".repeat(79)}… • 0s`,
      }),
    );
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

    expect(toastDismissMock).toHaveBeenCalledWith(
      "agent-workspace-operation:conversation-1:publish",
    );
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
    progress.success("Duplicate terminal result");

    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Published branch",
      {
        description: "Agent conversation • Push branch",
        duration: 8_000,
        id: "agent-workspace-operation:conversation-2:publish",
      },
    );
    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
    expect(toastSuccessMock).toHaveBeenCalledTimes(1);
  });

  it("dismisses an obsolete active loading toast without blocking later results", () => {
    const toastId = agentWorkspaceOperationToastId(
      "conversation-1",
      "update-from-base",
    );
    const progress = startAgentWorkspaceOperationToast({
      conversationTitle: "Agent conversation",
      detail: "From main",
      id: toastId,
      title: "Updating branch",
    });
    const loadingCallCount = toastLoadingMock.mock.calls.length;

    progress.dismiss();
    progress.update({ detail: "Still running" });
    vi.advanceTimersByTime(3_000);

    expect(toastDismissMock).toHaveBeenCalledWith(toastId);
    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);

    progress.success("Updated from main");

    expect(toastSuccessMock).toHaveBeenCalledWith("Updated from main", {
      description: "Agent conversation • From main",
      duration: 8_000,
      id: toastId,
    });
    expect(isAgentWorkspaceOperationToastDismissed(toastId)).toBe(false);
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
    progress.update({ detail: "Still running" });
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
    loadingOptions?.onDismiss?.();
  });

  it("keeps a user dismissal until settlement clears the shared registry", () => {
    const toastId = agentWorkspaceMaintenanceOperationToastId(
      "conversation-1",
      "operation-1",
    );
    const first = startAgentWorkspaceOperationToast({
      id: toastId,
      targetConversation: {
        conversationId: "conversation-1",
        projectId: "project-1",
      },
      title: "First writer",
    });

    const loadingOptions = toastLoadingMock.mock.calls.at(-1)?.[1] as
      | { onDismiss?: () => void }
      | undefined;
    loadingOptions?.onDismiss?.();
    expect(isAgentWorkspaceOperationToastDismissed(toastId)).toBe(true);

    const loadingCallCount = toastLoadingMock.mock.calls.length;
    const replacement = startAgentWorkspaceOperationToast({
      id: toastId,
      targetConversation: {
        conversationId: "conversation-1",
        projectId: "project-1",
      },
      title: "Replacement writer",
    });
    vi.advanceTimersByTime(1_000);

    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
    replacement.error("Replacement must stay hidden");
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(isAgentWorkspaceOperationToastDismissed(toastId)).toBe(false);

    first.success("Dismissed operation must stay hidden");
    expect(toastSuccessMock).not.toHaveBeenCalled();

    const nextOperation = startAgentWorkspaceOperationToast({
      id: toastId,
      targetConversation: {
        conversationId: "conversation-1",
        projectId: "project-1",
      },
      title: "Next operation",
    });
    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Next operation",
      expect.objectContaining({ id: toastId }),
    );
    nextOperation.dismiss();
  });

  it("suppresses loading and settlement while its target conversation is visible", () => {
    visibleAgentScopeMock.mockReturnValue({
      visibleConversationId: "conversation-1",
      workspaceConversationId: "conversation-1",
    });
    const progress = startAgentWorkspaceOperationToast({
      id: agentWorkspaceOperationToastId("conversation-1", "publish"),
      targetConversation: {
        conversationId: "conversation-1",
        projectId: "project-1",
      },
      title: "Publishing workspace",
    });

    expect(toastLoadingMock).not.toHaveBeenCalled();
    progress.success("Published branch");
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("re-shows after reversible visibility suppression without recording a dismissal", () => {
    const toastId = agentWorkspaceMaintenanceOperationToastId(
      "conversation-1",
      "operation-1",
    );
    visibleAgentScopeMock.mockReturnValue({
      visibleConversationId: "conversation-1",
      workspaceConversationId: "conversation-1",
    });
    startAgentWorkspaceOperationToast({
      id: toastId,
      targetConversation: {
        conversationId: "conversation-1",
        projectId: "project-1",
      },
      title: "Repairing workspace",
    });

    expect(toastLoadingMock).not.toHaveBeenCalled();

    visibleAgentScopeMock.mockReturnValue({
      visibleConversationId: "conversation-2",
      workspaceConversationId: "conversation-2",
    });
    vi.advanceTimersByTime(1_000);
    const loadingOptions = toastLoadingMock.mock.calls.at(-1)?.[1] as
      | { onDismiss?: () => void }
      | undefined;
    expect(toastLoadingMock).toHaveBeenCalledTimes(1);

    visibleAgentScopeMock.mockReturnValue({
      visibleConversationId: "conversation-1",
      workspaceConversationId: "conversation-1",
    });
    vi.advanceTimersByTime(1_000);
    expect(toastDismissMock).toHaveBeenCalledWith(toastId);
    loadingOptions?.onDismiss?.();
    expect(isAgentWorkspaceOperationToastDismissed(toastId)).toBe(false);

    visibleAgentScopeMock.mockReturnValue({
      visibleConversationId: "conversation-2",
      workspaceConversationId: "conversation-2",
    });
    vi.advanceTimersByTime(1_000);
    expect(toastLoadingMock).toHaveBeenCalledTimes(2);
  });

  it("rechecks target visibility when an operation converts to a new toast id", () => {
    const firstId = agentWorkspaceOperationToastId("conversation-1", "publish");
    const secondId = agentWorkspaceMaintenanceOperationToastId(
      "conversation-2",
      "operation-1",
    );
    const progress = startAgentWorkspaceOperationToast({
      id: firstId,
      title: "Publishing workspace",
    });
    visibleAgentScopeMock.mockReturnValue({
      visibleConversationId: "conversation-2",
      workspaceConversationId: "conversation-2",
    });

    progress.update({
      id: secondId,
      targetConversation: {
        conversationId: "conversation-2",
        projectId: "project-1",
      },
    });

    expect(toastDismissMock).toHaveBeenCalledWith(firstId);
    expect(toastLoadingMock).toHaveBeenCalledTimes(1);
    progress.error("Repair blocked");
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("does not treat an internal id conversion as a user dismissal", () => {
    const firstId = agentWorkspaceOperationToastId("conversation-1", "publish");
    const secondId = agentWorkspaceOperationToastId("conversation-2", "publish");
    const progress = startAgentWorkspaceOperationToast({
      id: firstId,
      title: "Publishing workspace",
    });
    const firstLoadingOptions = toastLoadingMock.mock.calls.at(-1)?.[1] as
      | { onDismiss?: () => void }
      | undefined;

    progress.update({ id: secondId });
    firstLoadingOptions?.onDismiss?.();
    const loadingCallCount = toastLoadingMock.mock.calls.length;
    vi.advanceTimersByTime(1_000);

    expect(toastDismissMock).toHaveBeenCalledWith(firstId);
    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount + 1);
    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Publishing workspace",
      expect.objectContaining({ id: secondId }),
    );
  });

  it("supersedes an existing controller for the same toast id", () => {
    const toastId = agentWorkspaceOperationToastId(
      "conversation-1",
      "update-from-base",
    );
    const superseded = startAgentWorkspaceOperationToast({
      id: toastId,
      title: "First writer",
    });
    const current = startAgentWorkspaceOperationToast({
      id: toastId,
      title: "Second writer",
    });

    vi.advanceTimersByTime(1_000);

    expect(toastLoadingMock).toHaveBeenCalledTimes(3);
    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Second writer",
      expect.objectContaining({ id: toastId }),
    );
    superseded.success("Old writer must not settle the active toast");
    expect(toastSuccessMock).not.toHaveBeenCalled();

    const loadingOptions = toastLoadingMock.mock.calls.at(-1)?.[1] as
      | { onDismiss?: () => void }
      | undefined;
    loadingOptions?.onDismiss?.();
    const loadingCallCount = toastLoadingMock.mock.calls.length;

    current.update({ title: "Must not resurrect" });
    vi.advanceTimersByTime(2_000);

    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
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
