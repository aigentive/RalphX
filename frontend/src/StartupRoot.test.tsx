import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { StartupStatus } from "@/api/startup";
import { startupApi } from "@/api/startup";
import { markPostUpdatePreparing } from "@/lib/postUpdatePreparing";
import { useStartupStatus } from "@/hooks/useStartupStatus";
import { StartupRoot } from "./StartupRoot";

const { appModuleLoadMock, toastDismissMock, toastLoadingMock, toastWarningMock } = vi.hoisted(() => ({
  appModuleLoadMock: vi.fn(),
  toastDismissMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastWarningMock: vi.fn(),
}));

vi.mock("@/hooks/useStartupStatus", () => ({
  useStartupStatus: vi.fn(),
}));

vi.mock("./App", () => {
  appModuleLoadMock();
  return {
    default: () => (
      <>
        <div data-testid="real-app">Real App</div>
        <button
          onClick={(event) => {
            event.currentTarget.textContent = "Workspace opened";
          }}
          type="button"
        >
          Open workspace
        </button>
      </>
    ),
  };
});

vi.mock("sonner", () => ({
  toast: {
    dismiss: toastDismissMock,
    loading: toastLoadingMock,
    warning: toastWarningMock,
  },
}));

vi.mock("@/api/startup", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/startup")>();
  return {
    ...actual,
    startupApi: {
      ...actual.startupApi,
      getDiagnostics: vi.fn(),
      openLogs: vi.fn(),
      reportFrontendMilestone: vi.fn(),
    },
  };
});

function startupStatus(overrides: Partial<StartupStatus> = {}): StartupStatus {
  return {
    bootId: "boot-1",
    attemptId: 1,
    stage: "safety_recovery",
    startedAt: "2026-07-24T09:00:00Z",
    stageStartedAt: "2026-07-24T09:00:01Z",
    completedAt: null,
    appStateReady: false,
    runtimeReady: false,
    backgroundComplete: false,
    retryAllowed: false,
    progress: null,
    messageCode: "checking_interrupted_work",
    failureCode: null,
    diagnosticSummary: null,
    ...overrides,
  };
}

let currentStatus = startupStatus();

function mockStartupStatus() {
  vi.mocked(useStartupStatus).mockReturnValue({
    status: currentStatus,
    canMountApp: currentStatus.runtimeReady,
    isLoading: false,
    isTerminalFailure: currentStatus.stage === "failed",
    isBackgroundSettled: currentStatus.backgroundComplete,
    statusError: null,
    isStatusError: false,
    refetch: vi.fn(),
    retry: vi.fn(),
    isRetrying: false,
    retryError: null,
  });
}

describe("StartupRoot", () => {
  beforeEach(() => {
    currentStatus = startupStatus();
    localStorage.clear();
    vi.clearAllMocks();
    vi.mocked(startupApi.getDiagnostics).mockResolvedValue({
      attemptId: 1,
      stage: "failed",
      messageCode: "startup_failed",
      failureCode: "local_runtime_bind",
      canRetry: false,
    });
    vi.mocked(startupApi.openLogs).mockResolvedValue();
    vi.mocked(startupApi.reportFrontendMilestone).mockResolvedValue();
    mockStartupStatus();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not preload or mount the real app before app-state readiness", () => {
    render(<StartupRoot />);

    expect(appModuleLoadMock).not.toHaveBeenCalled();
    expect(screen.getByTestId("startup-screen")).toBeInTheDocument();
  });

  it("preloads after app-state readiness but mounts only after runtime readiness", async () => {
    vi.useFakeTimers();
    const { rerender } = render(<StartupRoot />);

    currentStatus = startupStatus({ appStateReady: true });
    mockStartupStatus();
    rerender(<StartupRoot />);

    await act(async () => {
      await Promise.resolve();
    });
    expect(appModuleLoadMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60 * 60 * 1_000);
    });
    expect(screen.queryByTestId("real-app")).not.toBeInTheDocument();

    currentStatus = startupStatus({
      appStateReady: true,
      runtimeReady: true,
      stage: "runtime_ready",
    });
    mockStartupStatus();
    rerender(<StartupRoot />);

    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
      await Promise.resolve();
    });
    expect(screen.getByTestId("real-app")).toBeInTheDocument();
  });

  it("keeps shell navigation clickable while background restoration remains pending", async () => {
    const user = userEvent.setup();
    currentStatus = startupStatus({
      appStateReady: true,
    });
    mockStartupStatus();
    const { rerender } = render(<StartupRoot />);

    await act(async () => {
      await Promise.resolve();
    });
    currentStatus = startupStatus({
      appStateReady: true,
      runtimeReady: true,
      stage: "runtime_ready",
    });
    mockStartupStatus();
    rerender(<StartupRoot />);

    const openWorkspaceButton = await screen.findByRole("button", {
      name: "Open workspace",
    });
    await waitFor(() => {
      expect(screen.queryByTestId("startup-screen")).not.toBeInTheDocument();
      expect(toastLoadingMock).toHaveBeenCalledWith(
        "Restoring background work…",
        expect.objectContaining({ id: "startup-background-operation" }),
      );
    });

    await user.click(openWorkspaceButton);

    expect(screen.getByRole("button", { name: "Workspace opened" })).toBeInTheDocument();
  });

  it("clears update context only after the current boot accepts the shell-paint milestone", async () => {
    markPostUpdatePreparing("0.12.3");
    currentStatus = startupStatus({
      appStateReady: true,
      runtimeReady: true,
      stage: "runtime_ready",
    });
    mockStartupStatus();
    render(<StartupRoot />);

    await waitFor(() =>
      expect(startupApi.reportFrontendMilestone).toHaveBeenCalledWith({
        bootId: "boot-1",
        attemptId: 1,
        milestone: "shell_painted",
      }),
    );
    await waitFor(() => expect(screen.queryByTestId("startup-screen")).not.toBeInTheDocument());
    expect(localStorage.getItem("ralphx:post-update-preparing")).toBeNull();
  });

  it("retains update context when the shell-paint milestone is rejected", async () => {
    markPostUpdatePreparing("0.12.3");
    vi.mocked(startupApi.reportFrontendMilestone).mockRejectedValue(
      new Error("stale startup attempt"),
    );
    currentStatus = startupStatus({
      appStateReady: true,
      runtimeReady: true,
      stage: "runtime_ready",
    });
    mockStartupStatus();
    render(<StartupRoot />);

    await waitFor(() => expect(startupApi.reportFrontendMilestone).toHaveBeenCalledTimes(1));

    expect(screen.getByTestId("startup-screen")).toBeInTheDocument();
    expect(localStorage.getItem("ralphx:post-update-preparing")).not.toBeNull();
  });

  it("lets the user retry a rejected shell handoff without exposing backend startup retry", async () => {
    const user = userEvent.setup();
    vi.mocked(startupApi.reportFrontendMilestone)
      .mockRejectedValueOnce(new Error("temporary milestone failure"))
      .mockResolvedValueOnce();
    currentStatus = startupStatus({
      appStateReady: true,
      runtimeReady: true,
      retryAllowed: false,
      stage: "runtime_ready",
    });
    mockStartupStatus();
    render(<StartupRoot />);

    const retryButton = await screen.findByRole("button", {
      name: "Try shell handoff again",
    });
    await user.click(retryButton);

    await waitFor(() =>
      expect(startupApi.reportFrontendMilestone).toHaveBeenCalledTimes(2),
    );
    await waitFor(() =>
      expect(screen.queryByTestId("startup-screen")).not.toBeInTheDocument(),
    );
  });

  it("retains update context when an earlier shell-paint report settles after a new attempt", async () => {
    markPostUpdatePreparing("0.12.3");
    let resolveFirstReport: (() => void) | undefined;
    let resolveSecondReport: (() => void) | undefined;
    vi.mocked(startupApi.reportFrontendMilestone)
      .mockImplementationOnce(
        () => new Promise<void>((resolve) => { resolveFirstReport = resolve; }),
      )
      .mockImplementationOnce(
        () => new Promise<void>((resolve) => { resolveSecondReport = resolve; }),
      );
    currentStatus = startupStatus({
      appStateReady: true,
      runtimeReady: true,
      stage: "runtime_ready",
    });
    mockStartupStatus();
    const { rerender } = render(<StartupRoot />);

    await waitFor(() => expect(startupApi.reportFrontendMilestone).toHaveBeenCalledTimes(1));
    currentStatus = startupStatus({
      bootId: "boot-2",
      attemptId: 2,
      appStateReady: true,
      runtimeReady: true,
      stage: "runtime_ready",
    });
    mockStartupStatus();
    rerender(<StartupRoot />);
    await waitFor(() => expect(startupApi.reportFrontendMilestone).toHaveBeenCalledTimes(2));

    await act(async () => {
      resolveFirstReport?.();
      await Promise.resolve();
    });

    expect(resolveSecondReport).toBeDefined();
    expect(screen.getByTestId("startup-screen")).toBeInTheDocument();
    expect(localStorage.getItem("ralphx:post-update-preparing")).not.toBeNull();
  });

  it("copies only validated startup diagnostics through the browser clipboard", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    currentStatus = startupStatus({
      stage: "failed",
      failureCode: "local_runtime_bind",
      diagnosticSummary: "RalphX could not start its local services.",
    });
    mockStartupStatus();
    render(<StartupRoot />);

    await user.click(screen.getByRole("button", { name: "Copy Diagnostics" }));

    expect(startupApi.getDiagnostics).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(JSON.stringify({
        attemptId: 1,
        stage: "failed",
        messageCode: "startup_failed",
        failureCode: "local_runtime_bind",
        canRetry: false,
      }, null, 2));
      expect(screen.getByText("Startup diagnostics copied.")).toBeInTheDocument();
    });
  });

  it("reports clipboard unavailability without claiming diagnostics were copied", async () => {
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: undefined,
    });
    currentStatus = startupStatus({ stage: "failed" });
    mockStartupStatus();
    render(<StartupRoot />);

    await user.click(screen.getByRole("button", { name: "Copy Diagnostics" }));

    await waitFor(() => expect(screen.getByText(
      "RalphX could not copy diagnostics. Quit and reopen RalphX to try again.",
    )).toBeInTheDocument());
  });

  it("reports clipboard write failures without a false success", async () => {
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    currentStatus = startupStatus({ stage: "failed" });
    mockStartupStatus();
    render(<StartupRoot />);

    await user.click(screen.getByRole("button", { name: "Copy Diagnostics" }));

    await waitFor(() => {
      expect(screen.queryByText("Startup diagnostics copied.")).not.toBeInTheDocument();
      expect(screen.getByText(
        "RalphX could not copy diagnostics. Quit and reopen RalphX to try again.",
      )).toBeInTheDocument();
    });
  });
});
