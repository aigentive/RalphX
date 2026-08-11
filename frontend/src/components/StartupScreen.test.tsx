import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { StartupStatus } from "@/api/startup";
import { StartupScreen } from "./StartupScreen";

function startupStatus(overrides: Partial<StartupStatus> = {}): StartupStatus {
  return {
    bootId: "boot-1",
    attemptId: 1,
    stage: "migrating",
    startedAt: new Date(Date.now() - 5_000).toISOString(),
    stageStartedAt: new Date(Date.now() - 2_000).toISOString(),
    completedAt: null,
    appStateReady: false,
    runtimeReady: false,
    backgroundComplete: false,
    retryAllowed: false,
    progress: { completedUnits: 2, totalUnits: 4 },
    messageCode: "migrating_workspace_data",
    failureCode: null,
    diagnosticSummary: null,
    ...overrides,
  };
}

describe("StartupScreen", () => {
  it("presents typed progress through an accessible live status region", () => {
    render(<StartupScreen status={startupStatus()} updateVersion="0.12.3" />);

    expect(screen.getByRole("status")).toHaveTextContent(
      "Finishing the RalphX update",
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Upgrading workspace data",
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "2");
    expect(screen.getByText("2 of 4 complete")).toBeInTheDocument();
  });

  it("keeps terminal failure in the startup surface and offers retry", async () => {
    const onRetry = vi.fn();
    const user = userEvent.setup();
    render(
      <StartupScreen
        status={startupStatus({
          stage: "failed",
          retryAllowed: true,
          failureCode: "database_open_failed",
          diagnosticSummary: "RalphX could not prepare local workspace data.",
        })}
        onRetry={onRetry}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent(
      "RalphX could not finish starting",
    );
    expect(
      screen.getByText("RalphX could not prepare local workspace data."),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Retry startup" }));

    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("keeps post-registration failures recoverable without exposing a doomed retry", async () => {
    const onRetry = vi.fn();
    const onOpenLogs = vi.fn().mockResolvedValue(undefined);
    const onCopyDiagnostics = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(
      <StartupScreen
        status={startupStatus({
          stage: "failed",
          appStateReady: true,
          failureCode: "local_runtime_bind",
          diagnosticSummary: "RalphX could not start its local services.",
          retryAllowed: false,
        })}
        onCopyDiagnostics={onCopyDiagnostics}
        onOpenLogs={onOpenLogs}
        onRetry={onRetry}
      />,
    );

    expect(screen.queryByRole("button", { name: "Retry startup" })).not.toBeInTheDocument();
    expect(screen.getByText("Quit RalphX completely, then reopen it to start a fresh session.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Open Logs" }));
    await user.click(screen.getByRole("button", { name: "Copy Diagnostics" }));

    expect(onRetry).not.toHaveBeenCalled();
    expect(onOpenLogs).toHaveBeenCalledTimes(1);
    expect(onCopyDiagnostics).toHaveBeenCalledTimes(1);
  });
});
