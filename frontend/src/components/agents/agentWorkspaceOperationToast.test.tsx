import { act, fireEvent, render, screen } from "@testing-library/react";
import { isValidElement, type ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
  AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
  agentWorkspaceOperationErrorDetail,
  agentWorkspaceMaintenanceOperationToastId,
  agentWorkspaceOperationResultDetail,
  agentWorkspaceOperationToastId,
  dismissAgentWorkspaceOperationToast,
  maintenanceOperationToastLabel,
  publishPipelineToastLabel,
  renderAgentWorkspaceOperationToast,
  resetAgentWorkspaceOperationToastStateForTests,
  startAgentWorkspaceOperationToast,
  type AgentWorkspaceOperationToastView,
} from "./agentWorkspaceOperationToast";

const {
  toastDismissMock,
  toastErrorMock,
  toastInfoMock,
  toastLoadingMock,
  toastSuccessMock,
  navigateToAgentConversationMock,
} = vi.hoisted(() => ({
  toastDismissMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  navigateToAgentConversationMock: vi.fn(),
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

function buildView(
  overrides: Partial<AgentWorkspaceOperationToastView> = {},
): AgentWorkspaceOperationToastView {
  return {
    id: "toast-1",
    dismissalKey: "toast-1",
    title: "Publishing workspace",
    description: undefined,
    startedAtMs: null,
    targetConversation: undefined,
    tone: "loading",
    durationMs: Infinity,
    ...overrides,
  };
}

function lastLoadingContent(): ReactElement {
  return toastLoadingMock.mock.calls.at(-1)?.[0] as ReactElement;
}

function lastLoadingOptions(): Record<string, unknown> {
  return toastLoadingMock.mock.calls.at(-1)?.[1] as Record<string, unknown>;
}

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

  describe("renderAgentWorkspaceOperationToast", () => {
    it("calls toast.loading with JSX content and an infinite duration for the loading tone", () => {
      renderAgentWorkspaceOperationToast(buildView(), { onDismiss: vi.fn() });

      expect(toastLoadingMock).toHaveBeenCalledTimes(1);
      expect(isValidElement(lastLoadingContent())).toBe(true);
      expect(lastLoadingOptions()).toEqual(
        expect.objectContaining({ id: "toast-1", duration: Infinity }),
      );
    });

    it("never passes a Sonner action option or a native close button", () => {
      renderAgentWorkspaceOperationToast(buildView(), { onDismiss: vi.fn() });

      const options = lastLoadingOptions();
      expect(options.action).toBeUndefined();
      expect(options.closeButton).toBe(false);
    });

    it("invokes handlers.onDismiss and dismisses the Sonner toast when Dismiss is clicked", () => {
      const onDismiss = vi.fn();
      renderAgentWorkspaceOperationToast(buildView({ id: "toast-1" }), { onDismiss });

      render(lastLoadingContent());
      fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));

      expect(onDismiss).toHaveBeenCalledTimes(1);
      expect(toastDismissMock).toHaveBeenCalledWith("toast-1");
    });

    it("only shows Open conversation when a target conversation is set, and it navigates with the right args", () => {
      renderAgentWorkspaceOperationToast(buildView(), { onDismiss: vi.fn() });
      const { unmount } = render(lastLoadingContent());
      expect(
        screen.queryByRole("button", { name: "Open conversation" }),
      ).not.toBeInTheDocument();
      unmount();

      renderAgentWorkspaceOperationToast(
        buildView({
          id: "toast-2",
          targetConversation: { conversationId: "conversation-1", projectId: "project-1" },
        }),
        { onDismiss: vi.fn() },
      );
      render(lastLoadingContent());
      fireEvent.click(screen.getByRole("button", { name: "Open conversation" }));

      expect(navigateToAgentConversationMock).toHaveBeenCalledWith(
        "project-1",
        "conversation-1",
      );
    });

    it("routes success/error/info tones to the matching Sonner call with the given duration", () => {
      renderAgentWorkspaceOperationToast(
        buildView({ id: "toast-success", tone: "success", durationMs: 8_000 }),
        { onDismiss: vi.fn() },
      );
      expect(toastSuccessMock).toHaveBeenCalledTimes(1);
      expect(toastSuccessMock.mock.calls[0]?.[1]).toEqual(
        expect.objectContaining({ id: "toast-success", duration: 8_000 }),
      );

      renderAgentWorkspaceOperationToast(
        buildView({ id: "toast-error", tone: "error", durationMs: 12_000 }),
        { onDismiss: vi.fn() },
      );
      expect(toastErrorMock).toHaveBeenCalledTimes(1);
      expect(toastErrorMock.mock.calls[0]?.[1]).toEqual(
        expect.objectContaining({ id: "toast-error", duration: 12_000 }),
      );

      renderAgentWorkspaceOperationToast(
        buildView({ id: "toast-info", tone: "info", durationMs: 8_000 }),
        { onDismiss: vi.fn() },
      );
      expect(toastInfoMock).toHaveBeenCalledTimes(1);
      expect(toastInfoMock.mock.calls[0]?.[1]).toEqual(
        expect.objectContaining({ id: "toast-info", duration: 8_000 }),
      );
    });

    it("renders the elapsed meter converted from milliseconds and ticks without another toast.* call", () => {
      vi.setSystemTime(90_000);
      renderAgentWorkspaceOperationToast(buildView({ startedAtMs: 0 }), {
        onDismiss: vi.fn(),
      });
      render(lastLoadingContent());

      expect(screen.getByText("1m 30s")).toBeVisible();
      const loadingCallCount = toastLoadingMock.mock.calls.length;

      act(() => {
        vi.advanceTimersByTime(1_000);
      });

      expect(screen.getByText("1m 31s")).toBeVisible();
      expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
    });

    it("omits the elapsed meter entirely when startedAtMs is null", () => {
      renderAgentWorkspaceOperationToast(buildView({ startedAtMs: null }), {
        onDismiss: vi.fn(),
      });
      render(lastLoadingContent());

      expect(screen.queryByText(/^\d/)).not.toBeInTheDocument();
    });

    it("dismissAgentWorkspaceOperationToast dismisses the Sonner toast by id", () => {
      dismissAgentWorkspaceOperationToast("toast-1");
      expect(toastDismissMock).toHaveBeenCalledWith("toast-1");
    });
  });

  describe("startAgentWorkspaceOperationToast", () => {
    it("renders a loading toast immediately with an elapsed meter", () => {
      const controller = startAgentWorkspaceOperationToast({
        detail: "Checking the current review target",
        id: agentWorkspaceOperationToastId("conversation-1", "workspace-review"),
        title: "Starting Workspace Review",
      });

      expect(toastLoadingMock).toHaveBeenCalledTimes(1);
      expect(lastLoadingOptions()).toEqual(
        expect.objectContaining({ duration: Infinity }),
      );
      render(lastLoadingContent());
      expect(screen.getByText("Starting Workspace Review")).toBeVisible();
      expect(screen.getByText("Checking the current review target")).toBeVisible();

      controller.dismiss();
    });

    it("update() re-renders the loading toast with merged options", () => {
      const controller = startAgentWorkspaceOperationToast({
        id: "toast-review",
        title: "Starting Workspace Review",
      });

      controller.update({ detail: "Submitting the current review receipt" });

      expect(toastLoadingMock).toHaveBeenCalledTimes(2);
      render(lastLoadingContent());
      expect(
        screen.getByText("Submitting the current review receipt"),
      ).toBeVisible();
    });

    it("settles to success and ignores further calls", () => {
      const controller = startAgentWorkspaceOperationToast({
        id: "toast-settle-success",
        title: "Starting Workspace Review",
      });

      controller.success("Workspace Review started");

      expect(toastSuccessMock).toHaveBeenCalledTimes(1);
      expect(toastSuccessMock.mock.calls[0]?.[1]).toEqual(
        expect.objectContaining({
          id: "toast-settle-success",
          duration: AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
        }),
      );

      controller.error("Should be ignored");
      expect(toastErrorMock).not.toHaveBeenCalled();

      const loadingCallCount = toastLoadingMock.mock.calls.length;
      controller.update({ title: "Should be ignored" });
      expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
    });

    it("settles to error/info with detail and duration overrides", () => {
      const errorController = startAgentWorkspaceOperationToast({
        id: "toast-settle-error",
        title: "Starting Workspace Review",
      });
      errorController.error("Workspace Review did not start", {
        detail: "Network error",
        duration: 5_000,
      });
      expect(toastErrorMock).toHaveBeenCalledWith(
        expect.anything(),
        expect.objectContaining({ id: "toast-settle-error", duration: 5_000 }),
      );

      const infoController = startAgentWorkspaceOperationToast({
        id: "toast-settle-info",
        title: "Starting Workspace Review",
      });
      infoController.info("Review details changed", { detail: "Refreshing" });
      expect(toastInfoMock).toHaveBeenCalledWith(
        expect.anything(),
        expect.objectContaining({
          id: "toast-settle-info",
          duration: AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
        }),
      );
    });

    it("uses the error duration default when none is supplied", () => {
      const controller = startAgentWorkspaceOperationToast({
        id: "toast-error-default",
        title: "Starting Workspace Review",
      });
      controller.error("Workspace Review did not start");
      expect(toastErrorMock).toHaveBeenCalledWith(
        expect.anything(),
        expect.objectContaining({
          id: "toast-error-default",
          duration: AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
        }),
      );
    });

    it("dismiss() stops the controller without a durable dismissal key", () => {
      const controller = startAgentWorkspaceOperationToast({
        id: "toast-dismiss",
        title: "Starting Workspace Review",
      });

      controller.dismiss();
      controller.success("Should not render");

      expect(toastSuccessMock).not.toHaveBeenCalled();
    });
  });
});
