import React from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useUiStore } from "@/stores/uiStore";
import { POST_UPDATE_PREPARING_STORAGE_KEY } from "@/lib/postUpdatePreparing";
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
  listReleaseNotesVersions: vi.fn(),
  getReleaseNotesForVersion: vi.fn(),
  fetchReleaseMetadata: vi.fn(),
  getVersion: vi.fn(),
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

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: (...args: unknown[]) => mocks.getVersion(...args),
}));

vi.mock("sonner", () => ({
  toast: Object.assign(mocks.toast, {
    dismiss: mocks.toastDismiss,
    error: mocks.toastError,
    loading: mocks.toastLoading,
    success: mocks.toastSuccess,
  }),
}));

vi.mock("react-markdown", () => ({
  default: ({ children }: { children: string }) => children,
}));

vi.mock("remark-gfm", () => ({
  default: () => {},
}));

vi.mock("@/components/Chat/MessageItem.markdown", () => ({
  markdownComponents: {},
}));

vi.mock("@/api/release-notes", async () => {
  const actual = await vi.importActual<Record<string, unknown>>("@/api/release-notes");
  return {
    ...actual,
    getCurrentReleaseNotes: (...args: unknown[]) => mocks.getCurrentReleaseNotes(...args),
    getLastSeenReleaseNotesVersion: (...args: unknown[]) =>
      mocks.getLastSeenReleaseNotesVersion(...args),
    markReleaseNotesSeen: (...args: unknown[]) => mocks.markReleaseNotesSeen(...args),
    listReleaseNotesVersions: (...args: unknown[]) => mocks.listReleaseNotesVersions(...args),
    getReleaseNotesForVersion: (...args: unknown[]) => mocks.getReleaseNotesForVersion(...args),
    fetchReleaseMetadata: (...args: unknown[]) => mocks.fetchReleaseMetadata(...args),
  };
});

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

function toastCallsById(id: string) {
  return mocks.toast.mock.calls.filter(
    ([, options]) => (options as { id?: string } | undefined)?.id === id,
  );
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
    mocks.listReleaseNotesVersions.mockReset();
    mocks.getReleaseNotesForVersion.mockReset();
    mocks.fetchReleaseMetadata.mockReset();
    mocks.getVersion.mockReset();
    update.downloadAndInstall.mockReset();
    localStorage.clear();
    useUiStore.setState({ activeModal: null, modalContext: undefined });

    mocks.check.mockResolvedValue(update);
    mocks.relaunch.mockResolvedValue(undefined);
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
    mocks.listReleaseNotesVersions.mockResolvedValue(["0.9.0", "0.8.0", "0.3.1"]);
    mocks.fetchReleaseMetadata.mockResolvedValue(new Map());
    mocks.getVersion.mockResolvedValue("0.3.1");
    mocks.getReleaseNotesForVersion.mockImplementation(async (version: string) => ({
      version,
      body: `Release notes for ${version}`,
      source: "development_checkout",
    }));
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
    expect(localStorage.getItem(POST_UPDATE_PREPARING_STORAGE_KEY)).toBeNull();

    await vi.advanceTimersByTimeAsync(2_000);
    const marker = JSON.parse(
      localStorage.getItem(POST_UPDATE_PREPARING_STORAGE_KEY) ?? "null",
    ) as { startedAt?: unknown; version?: unknown } | null;
    expect(marker).toMatchObject({ version: "0.7.1" });
    expect(typeof marker?.startedAt).toBe("number");
    expect(mocks.relaunch).toHaveBeenCalled();
  });

  it("clears the preparing marker and reports when relaunch fails", async () => {
    const restartError = new Error("restart denied");
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.relaunch.mockRejectedValue(restartError);
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    const versioned = { ...update, version: "0.7.3", downloadAndInstall };
    mocks.check.mockResolvedValue(versioned);

    try {
      render(<UpdateChecker />);
      await vi.advanceTimersByTimeAsync(3_000);

      const toastUi = renderToastById("update-available");
      await fireEvent.click(toastUi.getByTestId("update-install-button"));

      await vi.advanceTimersByTimeAsync(1_500);
      await flushAsyncWork();

      expect(localStorage.getItem(POST_UPDATE_PREPARING_STORAGE_KEY)).toBeNull();
      expect(mocks.toastError).toHaveBeenCalledWith(
        "Failed to restart RalphX. Please reopen the app manually.",
        expect.objectContaining({ id: "update-progress" }),
      );
      expect(consoleError).toHaveBeenCalledWith("Update relaunch failed:", restartError);
    } finally {
      consoleError.mockRestore();
    }
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

  it("native menu check reports manual update check failures", async () => {
    mocks.check.mockRejectedValue(new Error("offline"));
    render(<UpdateChecker />);

    eventListeners.get("ralphx://check-for-updates")?.({ payload: undefined });
    await flushAsyncWork();

    expect(mocks.toastError).toHaveBeenCalledWith(
      "Failed to check for updates. Please try again later.",
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

  it("opens the release notes dialog from the update toast", async () => {
    mocks.check.mockResolvedValue({
      ...update,
      body: "## Daily Release\n\n- Better update prompts",
    });

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const toastUi = renderToastById("update-available");
    fireEvent.click(toastUi.getByTestId("update-release-notes-button"));
    await flushAsyncWork();

    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();
  });

  it("opens update toast release notes for the update version when metadata is stale", async () => {
    mocks.check.mockResolvedValue({
      ...update,
      version: "0.31.1",
      currentVersion: "0.31.0",
      body: "## RalphX.app 0.31.1\n\nStabilizes active chat recovery",
    });
    mocks.listReleaseNotesVersions.mockResolvedValue(["0.31.0"]);
    mocks.getVersion.mockResolvedValue("0.31.0");

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const toastUi = renderToastById("update-available");
    fireEvent.click(toastUi.getByTestId("update-release-notes-button"));
    await act(async () => {
      for (let i = 0; i < 20; i++) await Promise.resolve();
    });

    expect(screen.getByRole("heading", { name: /v0\.31\.1/ })).toBeInTheDocument();
    expect(screen.getByTestId("release-notes-dialog-body")).toHaveTextContent(
      "Stabilizes active chat recovery",
    );
    expect(mocks.getReleaseNotesForVersion).not.toHaveBeenCalledWith("0.31.1");
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

    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();

    fireEvent.click(toastUi.getByTestId("whats-new-dismiss-button"));
    expect(mocks.markReleaseNotesSeen).toHaveBeenCalledWith("0.9.0");
    expect(mocks.toastDismiss).toHaveBeenCalledWith("whats-new-0.9.0");
  });

  it("strips generated GitHub metadata from What's new preview", async () => {
    mocks.check.mockResolvedValue(null);
    mocks.getCurrentReleaseNotes.mockResolvedValue({
      version: "0.9.0",
      body: [
        "## Current Release",
        "",
        "- Rich markdown notes",
        "",
        "<!-- github-release-metadata:start -->",
        "## What's Changed",
        "",
        "* Release metadata stays visible",
        "<!-- github-release-metadata:end -->",
      ].join("\n"),
      source: "development_checkout",
    });
    mocks.getLastSeenReleaseNotesVersion.mockResolvedValue("0.8.0");

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(4_000);

    const toastUi = renderToastById("whats-new-0.9.0");
    expect(toastUi.getByTestId("whats-new-toast")).toHaveTextContent(
      "Rich markdown notes",
    );
    expect(toastUi.getByTestId("whats-new-toast")).not.toHaveTextContent(
      "github-release-metadata",
    );
  });

  it("defers the What's new toast while the settings modal is open", async () => {
    mocks.check.mockResolvedValue(null);
    mocks.getCurrentReleaseNotes.mockResolvedValue({
      version: "0.9.0",
      body: "## Current Release\n\n- Rich markdown notes",
      source: "development_checkout",
    });
    mocks.getLastSeenReleaseNotesVersion.mockResolvedValue("0.8.0");
    useUiStore.setState({ activeModal: "settings", modalContext: undefined });

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(4_000);

    expect(toastCallsById("whats-new-0.9.0")).toHaveLength(0);

    act(() => {
      useUiStore.getState().closeModal();
    });
    await flushAsyncWork();

    expect(toastCallsById("whats-new-0.9.0")).toHaveLength(1);
    expect(mocks.markReleaseNotesSeen).not.toHaveBeenCalled();
  });

  it("dismisses and replays the What's new toast around settings without marking it seen", async () => {
    mocks.check.mockResolvedValue(null);
    mocks.getCurrentReleaseNotes.mockResolvedValue({
      version: "0.9.0",
      body: "## Current Release\n\n- Rich markdown notes",
      source: "development_checkout",
    });
    mocks.getLastSeenReleaseNotesVersion.mockResolvedValue("0.8.0");

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(4_000);
    expect(toastCallsById("whats-new-0.9.0")).toHaveLength(1);

    act(() => {
      useUiStore.getState().openModal("settings");
    });
    await flushAsyncWork();

    expect(mocks.toastDismiss).toHaveBeenCalledWith("whats-new-0.9.0");
    expect(mocks.markReleaseNotesSeen).not.toHaveBeenCalled();

    act(() => {
      useUiStore.getState().closeModal();
    });
    await flushAsyncWork();

    expect(toastCallsById("whats-new-0.9.0")).toHaveLength(2);
  });

  it("native menu release notes opens the dialog", async () => {
    render(<UpdateChecker />);

    await act(async () => {
      eventListeners.get("ralphx://show-release-notes")?.({ payload: undefined });
      await flushAsyncWork();
    });

    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();
  });

  it("does not show What's new when current release notes were already seen", async () => {
    mocks.check.mockResolvedValue(null);
    mocks.getCurrentReleaseNotes.mockResolvedValue({
      version: "0.9.0",
      body: "## Already Seen",
      source: "development_checkout",
    });
    mocks.getLastSeenReleaseNotesVersion.mockResolvedValue("0.9.0");

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(4_000);

    expect(mocks.toast).not.toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ id: "whats-new-0.9.0" }),
    );
    expect(mocks.markReleaseNotesSeen).not.toHaveBeenCalled();
  });

  it("handleUpdateFromDialog triggers update check and installs when available", async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    const versioned = { ...update, version: "0.5.0", downloadAndInstall };

    mocks.check.mockResolvedValueOnce({
      ...update,
      body: "## Release\n\n- Notes",
    });

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const toastUi = renderToastById("update-available");
    fireEvent.click(toastUi.getByTestId("update-release-notes-button"));

    await act(async () => {
      for (let i = 0; i < 20; i++) await Promise.resolve();
    });

    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();

    const updateButton = screen.getByTestId("release-notes-update-button");
    expect(updateButton).toBeInTheDocument();

    mocks.check.mockResolvedValueOnce(versioned);

    await act(async () => {
      fireEvent.click(updateButton);
      for (let i = 0; i < 10; i++) await Promise.resolve();
    });

    expect(mocks.check).toHaveBeenCalledTimes(2);
  });

  it("handleUpdateFromDialog shows up-to-date toast when no update available", async () => {
    mocks.check.mockResolvedValueOnce({
      ...update,
      body: "## Release\n\n- Notes",
    });

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const toastUi = renderToastById("update-available");
    fireEvent.click(toastUi.getByTestId("update-release-notes-button"));

    await act(async () => {
      for (let i = 0; i < 20; i++) await Promise.resolve();
    });

    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();

    mocks.check.mockResolvedValueOnce(null);

    const updateButton = screen.getByTestId("release-notes-update-button");
    await act(async () => {
      fireEvent.click(updateButton);
      for (let i = 0; i < 10; i++) await Promise.resolve();
    });

    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      "RalphX is up to date.",
      expect.objectContaining({ id: "update-check-result" }),
    );
  });

  it("handleUpdateFromDialog shows error toast when check fails", async () => {
    mocks.check.mockResolvedValueOnce({
      ...update,
      body: "## Release\n\n- Notes",
    });

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const toastUi = renderToastById("update-available");
    fireEvent.click(toastUi.getByTestId("update-release-notes-button"));

    await act(async () => {
      for (let i = 0; i < 20; i++) await Promise.resolve();
    });

    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();

    mocks.check.mockRejectedValueOnce(new Error("network error"));

    const updateButton = screen.getByTestId("release-notes-update-button");
    await act(async () => {
      fireEvent.click(updateButton);
      for (let i = 0; i < 10; i++) await Promise.resolve();
    });

    expect(mocks.toastError).toHaveBeenCalledWith(
      "Failed to check for updates.",
      expect.objectContaining({ id: "update-check-result" }),
    );
  });

  it("opens release notes dialog from native menu event", async () => {
    mocks.check.mockResolvedValue(null);
    render(<UpdateChecker />);

    await act(async () => {
      eventListeners.get("ralphx://show-release-notes")?.({ payload: undefined });
      await flushAsyncWork();
    });

    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();
  });
});
