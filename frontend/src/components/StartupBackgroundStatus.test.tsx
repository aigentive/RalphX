import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { StartupStatus } from "@/api/startup";
import { useUiStore } from "@/stores/uiStore";
import {
  STARTUP_BACKGROUND_OPERATION_TOAST_ID,
  StartupBackgroundStatus,
} from "./StartupBackgroundStatus";

const { toastDismissMock, toastLoadingMock, toastWarningMock } = vi.hoisted(() => ({
  toastDismissMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastWarningMock: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    dismiss: toastDismissMock,
    loading: toastLoadingMock,
    warning: toastWarningMock,
  },
}));

function startupStatus(overrides: Partial<StartupStatus> = {}): StartupStatus {
  return {
    bootId: "boot-1",
    attemptId: 1,
    stage: "background_recovery",
    startedAt: "2026-07-24T09:00:00Z",
    stageStartedAt: "2026-07-24T09:00:01Z",
    completedAt: null,
    appStateReady: true,
    runtimeReady: true,
    backgroundComplete: false,
    retryAllowed: false,
    progress: null,
    messageCode: "startup_restoring_interrupted_work",
    failureCode: null,
    diagnosticSummary: null,
    ...overrides,
  };
}

describe("StartupBackgroundStatus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUiStore.setState({ currentView: "agents" });
  });

  it("keeps one non-blocking startup operation toast while background recovery is pending", () => {
    render(<StartupBackgroundStatus active status={startupStatus()} />);

    expect(toastLoadingMock).toHaveBeenCalledWith(
      "Restoring background work…",
      expect.objectContaining({
        duration: Infinity,
        id: STARTUP_BACKGROUND_OPERATION_TOAST_ID,
      }),
    );
  });

  it("dismisses the operation status only when the startup reaches ready", () => {
    const { rerender } = render(
      <StartupBackgroundStatus active status={startupStatus()} />,
    );

    rerender(
      <StartupBackgroundStatus
        active
        status={startupStatus({
          backgroundComplete: true,
          stage: "ready",
        })}
      />,
    );

    expect(toastDismissMock).toHaveBeenCalledWith(
      STARTUP_BACKGROUND_OPERATION_TOAST_ID,
    );
  });

  it("retains an actionable warning when background recovery is degraded", () => {
    render(
      <StartupBackgroundStatus
        active
        status={startupStatus({
          backgroundComplete: true,
          stage: "degraded",
        })}
      />,
    );

    const [, options] = toastWarningMock.mock.calls[0] ?? [];
    expect(options).toEqual(
      expect.objectContaining({
        duration: Infinity,
        id: STARTUP_BACKGROUND_OPERATION_TOAST_ID,
      }),
    );

    const action = (options as { action: { onClick: () => void } }).action;
    action.onClick();

    expect(useUiStore.getState().currentView).toBe("activity");
  });
});
