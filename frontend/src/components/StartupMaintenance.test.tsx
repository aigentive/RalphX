import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  STARTUP_MAINTENANCE_IDLE_GRACE_MS,
  StartupMaintenance,
} from "./StartupMaintenance";

const {
  nativeEventHookMock,
  providerCliModuleLoadMock,
  updateModuleLoadMock,
} = vi.hoisted(() => ({
  nativeEventHookMock: vi.fn(),
  providerCliModuleLoadMock: vi.fn(),
  updateModuleLoadMock: vi.fn(),
}));

vi.mock("./UpdateChecker", () => {
  updateModuleLoadMock();
  return {
    UpdateChecker: ({
      automaticMaintenanceEnabled,
      checkForUpdatesRequest,
      listenForNativeActions,
      openReleaseNotesRequest,
    }: {
      automaticMaintenanceEnabled?: boolean;
      checkForUpdatesRequest?: number;
      listenForNativeActions?: boolean;
      openReleaseNotesRequest?: number;
    }) => (
      <div
        data-automatic-maintenance-enabled={String(automaticMaintenanceEnabled)}
        data-check-for-updates-request={String(checkForUpdatesRequest)}
        data-listen-for-native-actions={String(listenForNativeActions)}
        data-open-release-notes-request={String(openReleaseNotesRequest)}
        data-testid="update-checker"
      >
        Update checker
      </div>
    ),
  };
});

vi.mock("./UpdateChecker.events", () => ({
  useUpdateCheckerNativeEvents: (handlers: unknown) =>
    nativeEventHookMock(handlers),
}));

vi.mock("./ProviderCliUpdateChecker", () => {
  providerCliModuleLoadMock();
  return {
    ProviderCliUpdateChecker: () => (
      <div data-testid="provider-cli-update-checker">Provider CLI checker</div>
    ),
  };
});

describe("StartupMaintenance", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps native update actions mounted while deferring automatic maintenance until settlement", async () => {
    const { rerender } = render(<StartupMaintenance backgroundSettled={false} />);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(STARTUP_MAINTENANCE_IDLE_GRACE_MS * 2);
    });
    expect(updateModuleLoadMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("provider-cli-update-checker")).not.toBeInTheDocument();

    const nativeActions = nativeEventHookMock.mock.lastCall?.[0] as {
      openCurrentReleaseNotes: () => void;
    };
    await act(async () => {
      nativeActions.openCurrentReleaseNotes();
      await Promise.resolve();
    });

    expect(screen.getByTestId("update-checker")).toHaveAttribute(
      "data-automatic-maintenance-enabled",
      "false",
    );
    expect(screen.getByTestId("update-checker")).toHaveAttribute(
      "data-listen-for-native-actions",
      "false",
    );
    expect(screen.getByTestId("update-checker")).toHaveAttribute(
      "data-open-release-notes-request",
      "1",
    );
    expect(screen.queryByTestId("provider-cli-update-checker")).not.toBeInTheDocument();

    rerender(<StartupMaintenance backgroundSettled />);
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByTestId("update-checker")).toHaveAttribute(
      "data-automatic-maintenance-enabled",
      "false",
    );
    expect(providerCliModuleLoadMock).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(STARTUP_MAINTENANCE_IDLE_GRACE_MS);
      await Promise.resolve();
    });

    expect(providerCliModuleLoadMock).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("update-checker")).toHaveAttribute(
      "data-automatic-maintenance-enabled",
      "true",
    );
    expect(screen.getByTestId("provider-cli-update-checker")).toBeInTheDocument();
  });
});
