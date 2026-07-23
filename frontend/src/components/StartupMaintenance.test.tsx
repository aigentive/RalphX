import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  STARTUP_MAINTENANCE_IDLE_GRACE_MS,
  StartupMaintenance,
} from "./StartupMaintenance";

const { providerCliModuleLoadMock, updateModuleLoadMock } = vi.hoisted(() => ({
  providerCliModuleLoadMock: vi.fn(),
  updateModuleLoadMock: vi.fn(),
}));

vi.mock("./UpdateChecker", () => {
  updateModuleLoadMock();
  return { UpdateChecker: () => <div data-testid="update-checker">Update checker</div> };
});

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

  it("does not load app, release-note, or provider CLI maintenance before settlement and idle grace", async () => {
    const { rerender } = render(<StartupMaintenance backgroundSettled={false} />);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(STARTUP_MAINTENANCE_IDLE_GRACE_MS * 2);
    });
    expect(updateModuleLoadMock).not.toHaveBeenCalled();
    expect(providerCliModuleLoadMock).not.toHaveBeenCalled();

    rerender(<StartupMaintenance backgroundSettled />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(STARTUP_MAINTENANCE_IDLE_GRACE_MS - 1);
    });
    expect(updateModuleLoadMock).not.toHaveBeenCalled();
    expect(providerCliModuleLoadMock).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
      await Promise.resolve();
    });

    expect(updateModuleLoadMock).toHaveBeenCalledTimes(1);
    expect(providerCliModuleLoadMock).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("update-checker")).toBeInTheDocument();
    expect(screen.getByTestId("provider-cli-update-checker")).toBeInTheDocument();
  });
});
