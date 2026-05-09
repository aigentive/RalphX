import React from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UpdateChecker } from "./UpdateChecker";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  listen: vi.fn(),
  relaunch: vi.fn(),
  toast: vi.fn(),
  toastDismiss: vi.fn(),
  toastError: vi.fn(),
  toastLoading: vi.fn(),
  toastSuccess: vi.fn(),
  getCurrentReleaseNotes: vi.fn(),
  getLastSeenReleaseNotesVersion: vi.fn(),
  markReleaseNotesSeen: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: (...args: unknown[]) => mocks.check(...args),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: (...args: unknown[]) => mocks.relaunch(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => mocks.listen(...args),
}));

vi.mock("sonner", () => ({
  toast: Object.assign(mocks.toast, {
    dismiss: mocks.toastDismiss,
    error: mocks.toastError,
    loading: mocks.toastLoading,
    success: mocks.toastSuccess,
  }),
}));

vi.mock("@/api/release-notes", () => ({
  getCurrentReleaseNotes: (...args: unknown[]) => mocks.getCurrentReleaseNotes(...args),
  getLastSeenReleaseNotesVersion: (...args: unknown[]) =>
    mocks.getLastSeenReleaseNotesVersion(...args),
  markReleaseNotesSeen: (...args: unknown[]) => mocks.markReleaseNotesSeen(...args),
}));

const update = {
  version: "0.3.2",
  currentVersion: "0.3.1",
  body: "Daily release",
  downloadAndInstall: vi.fn(),
};

const eventListeners = new Map<string, (event: unknown) => unknown>();

function renderToastById(id: string) {
  const call = mocks.toast.mock.calls.find(
    ([, options]) => (options as { id?: string } | undefined)?.id === id,
  );
  expect(call).toBeTruthy();
  return render(call![0] as React.ReactElement);
}

async function flushAsyncWork() {
  await Promise.resolve();
  await Promise.resolve();
}

describe("UpdateChecker", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    eventListeners.clear();

    mocks.check.mockReset();
    mocks.listen.mockReset();
    mocks.relaunch.mockReset();
    mocks.toast.mockReset();
    mocks.toastDismiss.mockReset();
    mocks.toastError.mockReset();
    mocks.toastLoading.mockReset();
    mocks.toastSuccess.mockReset();
    mocks.getCurrentReleaseNotes.mockReset();
    mocks.getLastSeenReleaseNotesVersion.mockReset();
    mocks.markReleaseNotesSeen.mockReset();
    update.downloadAndInstall.mockReset();

    mocks.check.mockResolvedValue(update);
    mocks.listen.mockImplementation(async (event: string, handler: (event: unknown) => unknown) => {
      eventListeners.set(event, handler);
      return vi.fn();
    });
    mocks.getCurrentReleaseNotes.mockResolvedValue({
      version: "0.3.1",
      body: null,
      source: "missing",
    });
    mocks.getLastSeenReleaseNotesVersion.mockResolvedValue("0.3.1");
    mocks.markReleaseNotesSeen.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("checks after startup in React StrictMode", async () => {
    render(
      <React.StrictMode>
        <UpdateChecker />
      </React.StrictMode>,
    );

    expect(mocks.check).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(3_000);

    expect(mocks.check).toHaveBeenCalledTimes(1);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
  });

  it("polls for later releases without re-notifying the same version", async () => {
    render(<UpdateChecker />);

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.advanceTimersByTimeAsync(30 * 60 * 1_000);

    expect(mocks.check).toHaveBeenCalledTimes(2);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
  });

  it("swallows automatic check() errors silently", async () => {
    mocks.check.mockRejectedValue(new Error("network down"));
    render(<UpdateChecker />);

    await vi.advanceTimersByTimeAsync(3_000);

    expect(mocks.toast).not.toHaveBeenCalled();
    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it("Later button dismisses the update toast", async () => {
    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const toastUi = renderToastById("update-available");
    fireEvent.click(toastUi.getByTestId("update-later-button"));

    expect(mocks.toastDismiss).toHaveBeenCalledWith("update-available");
  });

  it("Update Now triggers downloadAndInstall, progress events, success toast and relaunch", async () => {
    const downloadAndInstall = vi.fn().mockImplementation(async (cb) => {
      cb({ event: "Started", data: { contentLength: 100 } });
      cb({ event: "Progress", data: { chunkLength: 50 } });
      cb({ event: "Finished" });
    });
    const versioned = { ...update, version: "0.7.1", downloadAndInstall };
    mocks.check.mockResolvedValue(versioned);

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const toastUi = renderToastById("update-available");
    await fireEvent.click(toastUi.getByTestId("update-install-button"));

    expect(downloadAndInstall).toHaveBeenCalled();
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      expect.stringContaining("Update installed"),
      expect.objectContaining({ id: "update-progress" }),
    );

    await vi.advanceTimersByTimeAsync(2_000);
    expect(mocks.relaunch).toHaveBeenCalled();
  });

  it("install failure shows an error toast and does not relaunch", async () => {
    const downloadAndInstall = vi.fn().mockRejectedValue(new Error("disk full"));
    const versioned = { ...update, version: "0.7.2", downloadAndInstall };
    mocks.check.mockResolvedValue(versioned);

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const toastUi = renderToastById("update-available");
    await fireEvent.click(toastUi.getByTestId("update-install-button"));

    expect(mocks.toastError).toHaveBeenCalledWith(
      expect.stringContaining("Failed to install update"),
      expect.objectContaining({ id: "update-progress" }),
    );
    await vi.advanceTimersByTimeAsync(2_000);
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it("native menu check runs immediately and reports current version when no update exists", async () => {
    mocks.check.mockResolvedValue(null);
    render(<UpdateChecker />);

    eventListeners.get("ralphx://check-for-updates")?.({ payload: undefined });
    await flushAsyncWork();

    expect(mocks.check).toHaveBeenCalledTimes(1);
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      "RalphX is up to date.",
      expect.objectContaining({ id: "update-check-result" }),
    );
  });

  it("checks on app focus after the lifecycle cooldown", async () => {
    mocks.check.mockResolvedValue(null);
    render(<UpdateChecker />);

    await vi.advanceTimersByTimeAsync(3_000);
    expect(mocks.check).toHaveBeenCalledTimes(1);

    window.dispatchEvent(new Event("focus"));
    await flushAsyncWork();
    expect(mocks.check).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(5 * 60 * 1_000);
    window.dispatchEvent(new Event("focus"));
    await flushAsyncWork();

    expect(mocks.check).toHaveBeenCalledTimes(2);
  });

  it("opens rendered update release notes from the update toast", async () => {
    mocks.check.mockResolvedValue({
      ...update,
      body: "## Daily Release\n\n- Better update prompts",
    });

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const toastUi = renderToastById("update-available");
    fireEvent.click(toastUi.getByTestId("update-release-notes-button"));
    await flushAsyncWork();

    expect(screen.getByTestId("release-notes-dialog-body")).toHaveTextContent(
      "Daily Release",
    );
    expect(screen.getByTestId("release-notes-dialog-body")).toHaveTextContent(
      "Better update prompts",
    );
  });

  it("shows manually dismissable What's new toast for unseen current release notes", async () => {
    mocks.check.mockResolvedValue(null);
    mocks.getCurrentReleaseNotes.mockResolvedValue({
      version: "0.9.0",
      body: "## Current Release\n\n- Rich markdown notes",
      source: "development_checkout",
    });
    mocks.getLastSeenReleaseNotesVersion.mockResolvedValue("0.8.0");

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(4_000);

    const toastUi = renderToastById("whats-new-0.9.0");
    fireEvent.click(toastUi.getByTestId("whats-new-open-button"));
    await flushAsyncWork();

    expect(screen.getByTestId("release-notes-dialog-body")).toHaveTextContent(
      "Current Release",
    );
    expect(screen.getByTestId("release-notes-dialog-body")).toHaveTextContent(
      "Rich markdown notes",
    );

    fireEvent.click(toastUi.getByTestId("whats-new-dismiss-button"));
    expect(mocks.markReleaseNotesSeen).toHaveBeenCalledWith("0.9.0");
    expect(mocks.toastDismiss).toHaveBeenCalledWith("whats-new-0.9.0");
  });

  it("native menu release notes opens current-version notes", async () => {
    mocks.getCurrentReleaseNotes.mockResolvedValue({
      version: "0.9.0",
      body: "## Current Version Notes\n\n- Manual menu access",
      source: "development_checkout",
    });

    render(<UpdateChecker />);

    await act(async () => {
      eventListeners.get("ralphx://show-release-notes")?.({ payload: undefined });
      await flushAsyncWork();
    });

    expect(screen.getByTestId("release-notes-dialog-body")).toHaveTextContent(
      "Current Version Notes",
    );
    expect(screen.getByTestId("release-notes-dialog-body")).toHaveTextContent(
      "Manual menu access",
    );
  });
});
