import React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { providerCliManagementApi } from "@/api/provider-cli-management";
import { useUiStore } from "@/stores/uiStore";

import {
  PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY,
  ProviderCliUpdateChecker,
} from "./ProviderCliUpdateChecker";

const mocks = vi.hoisted(() => ({
  toast: vi.fn(),
  toastDismiss: vi.fn(),
  toastError: vi.fn(),
  toastLoading: vi.fn(),
  toastSuccess: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: Object.assign(mocks.toast, {
    dismiss: mocks.toastDismiss,
    error: mocks.toastError,
    loading: mocks.toastLoading,
    success: mocks.toastSuccess,
  }),
}));

vi.mock("@/api/provider-cli-management", () => ({
  providerCliManagementApi: {
    status: vi.fn(),
    installOrUpdate: vi.fn(),
    autoUpdate: vi.fn(),
  },
}));

function renderChecker() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderCliUpdateChecker />
    </QueryClientProvider>,
  );
}

function renderToast() {
  const [body] = mocks.toast.mock.calls.at(-1)!;
  return render(body as React.ReactElement);
}

function mockUserManagedClaudeUpdate() {
  vi.mocked(providerCliManagementApi.status).mockResolvedValue({
    providers: [
      {
        provider: "claude",
        cliManagementMode: "user_managed",
        autoUpdateEnabled: false,
        supported: true,
        installed: true,
        binaryPath: "/Users/example/.local/bin/claude",
        currentVersion: "2.1.170",
        latestVersion: "2.1.175",
        updateAvailable: true,
        action: "none",
        status:
          "claude CLI 2.1.170 is user-managed; 2.1.175 is available. RX will not update it unless management is enabled.",
        error: null,
      },
    ],
  });
}

describe("ProviderCliUpdateChecker", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    localStorage.clear();
    useUiStore.setState({ activeModal: null, modalContext: undefined });
    vi.mocked(providerCliManagementApi.status).mockResolvedValue({
      providers: [
        {
          provider: "codex",
          cliManagementMode: "rx_managed",
          autoUpdateEnabled: false,
          supported: true,
          installed: true,
          binaryPath: "/mock/codex",
          currentVersion: "0.136.0",
          latestVersion: "0.137.0",
          updateAvailable: true,
          action: "update",
          status: "RX-managed codex 0.136.0 can update to 0.137.0.",
          error: null,
        },
      ],
    });
    vi.mocked(providerCliManagementApi.installOrUpdate).mockResolvedValue({
      provider: "codex",
      success: true,
      status: {
        provider: "codex",
        cliManagementMode: "rx_managed",
        autoUpdateEnabled: false,
        supported: true,
        installed: true,
        binaryPath: "/mock/codex",
        currentVersion: "0.137.0",
        latestVersion: "0.137.0",
        updateAvailable: false,
        action: "none",
        status: "ready",
        error: null,
      },
      stdout: null,
      stderr: null,
    });
    vi.mocked(providerCliManagementApi.autoUpdate).mockResolvedValue({
      updated: [],
      skipped: [],
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows a startup toast for manual RX-managed CLI updates", async () => {
    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    expect(providerCliManagementApi.status).toHaveBeenCalled();
    expect(mocks.toast).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ id: "provider-cli-update:codex" }),
    );

    renderToast();
    expect(screen.getByText("Codex CLI update available")).toBeInTheDocument();
    expect(
      screen.getByText("RX-managed codex 0.136.0 can update to 0.137.0."),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("provider-cli-update-now-button"));
    expect(providerCliManagementApi.installOrUpdate).toHaveBeenCalledWith({
      provider: "codex",
    });
  });

  it("runs auto-update when the provider is opted in", async () => {
    vi.mocked(providerCliManagementApi.status).mockResolvedValue({
      providers: [
        {
          provider: "codex",
          cliManagementMode: "rx_managed",
          autoUpdateEnabled: true,
          supported: true,
          installed: true,
          binaryPath: "/mock/codex",
          currentVersion: "0.136.0",
          latestVersion: "0.137.0",
          updateAvailable: true,
          action: "update",
          status: "RX-managed codex 0.136.0 can update to 0.137.0.",
          error: null,
        },
      ],
    });
    vi.mocked(providerCliManagementApi.autoUpdate).mockResolvedValue({
      updated: [
        {
          provider: "codex",
          success: true,
          status: {
            provider: "codex",
            cliManagementMode: "rx_managed",
            autoUpdateEnabled: true,
            supported: true,
            installed: true,
            binaryPath: "/mock/codex",
            currentVersion: "0.137.0",
            latestVersion: "0.137.0",
            updateAvailable: false,
            action: "none",
            status: "ready",
            error: null,
          },
          stdout: null,
          stderr: null,
        },
      ],
      skipped: [],
    });

    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    expect(providerCliManagementApi.autoUpdate).toHaveBeenCalled();
    expect(mocks.toastLoading).toHaveBeenCalledWith(
      "Updating managed CLI tools...",
      expect.objectContaining({ id: "provider-cli-auto-update" }),
    );
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      "Managed CLI tools are up to date.",
      expect.objectContaining({ id: "provider-cli-auto-update" }),
    );
  });

  it("dismisses the auto-update progress toast when no managed CLIs changed", async () => {
    vi.mocked(providerCliManagementApi.status).mockResolvedValue({
      providers: [
        {
          provider: "codex",
          cliManagementMode: "rx_managed",
          autoUpdateEnabled: true,
          supported: true,
          installed: true,
          binaryPath: "/mock/codex",
          currentVersion: "0.137.0",
          latestVersion: "0.137.0",
          updateAvailable: true,
          action: "update",
          status: "RX-managed codex 0.137.0 can update to 0.137.0.",
          error: null,
        },
      ],
    });
    vi.mocked(providerCliManagementApi.autoUpdate).mockResolvedValue({
      updated: [],
      skipped: [],
    });

    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    expect(mocks.toastDismiss).toHaveBeenCalledWith("provider-cli-auto-update");
  });

  it("shows an error toast when managed CLI auto-update fails", async () => {
    const error = new Error("installer failed");
    vi.mocked(providerCliManagementApi.status).mockResolvedValue({
      providers: [
        {
          provider: "codex",
          cliManagementMode: "rx_managed",
          autoUpdateEnabled: true,
          supported: true,
          installed: true,
          binaryPath: "/mock/codex",
          currentVersion: "0.136.0",
          latestVersion: "0.137.0",
          updateAvailable: true,
          action: "update",
          status: "RX-managed codex 0.136.0 can update to 0.137.0.",
          error: null,
        },
      ],
    });
    vi.mocked(providerCliManagementApi.autoUpdate).mockRejectedValue(error);

    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    expect(mocks.toastError).toHaveBeenCalledWith(
      "Failed to update managed CLI tools.",
      expect.objectContaining({
        id: "provider-cli-auto-update",
        description: error.message,
      }),
    );
  });

  it("shows a non-mutating startup toast for user-managed outdated CLIs", async () => {
    mockUserManagedClaudeUpdate();

    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    expect(mocks.toast).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ id: "provider-cli-update:claude" }),
    );

    renderToast();
    expect(screen.getByText("Claude CLI update available")).toBeInTheDocument();
    expect(
      screen.getByText(
        "claude CLI 2.1.170 is user-managed; 2.1.175 is available. RX will not update it unless management is enabled.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("provider-cli-open-settings-button"));

    expect(providerCliManagementApi.installOrUpdate).not.toHaveBeenCalled();
    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({ section: "providers" });
  });

  it("does not show a CLI update toast after don't-ask-again was remembered", async () => {
    mockUserManagedClaudeUpdate();
    localStorage.setItem(
      PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY,
      JSON.stringify(["claude:2.1.175"]),
    );

    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    expect(mocks.toast).not.toHaveBeenCalled();
  });

  it("deduplicates repeated provider update statuses during one startup check", async () => {
    const repeatedStatus = {
      provider: "codex" as const,
      cliManagementMode: "rx_managed" as const,
      autoUpdateEnabled: false,
      supported: true,
      installed: true,
      binaryPath: "/mock/codex",
      currentVersion: "0.136.0",
      latestVersion: "0.137.0",
      updateAvailable: true,
      action: "update" as const,
      status: "RX-managed codex 0.136.0 can update to 0.137.0.",
      error: null,
    };
    vi.mocked(providerCliManagementApi.status).mockResolvedValue({
      providers: [repeatedStatus, repeatedStatus],
    });

    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    expect(mocks.toast).toHaveBeenCalledTimes(1);
  });

  it("logs and suppresses toast noise when startup CLI status checks fail", async () => {
    const debugSpy = vi.spyOn(console, "debug").mockImplementation(() => undefined);
    const error = new Error("status unavailable");
    vi.mocked(providerCliManagementApi.status).mockRejectedValue(error);

    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    expect(mocks.toast).not.toHaveBeenCalled();
    expect(debugSpy).toHaveBeenCalledWith("Provider CLI update check failed:", error);

    debugSpy.mockRestore();
  });

  it("asks whether to remind again when dismissing a CLI update toast", async () => {
    mockUserManagedClaudeUpdate();
    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    renderToast();
    fireEvent.click(screen.getByTestId("provider-cli-update-dismiss-button"));

    expect(
      screen.getByRole("dialog", { name: "Dismiss Claude CLI update?" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Remind me again" }));

    expect(mocks.toastDismiss).toHaveBeenCalledWith("provider-cli-update:claude");
    expect(
      localStorage.getItem(PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY),
    ).toBeNull();
    expect(
      screen.queryByRole("dialog", { name: "Dismiss Claude CLI update?" }),
    ).not.toBeInTheDocument();
  });

  it("keeps the current toast when the dismiss preference dialog is closed", async () => {
    mockUserManagedClaudeUpdate();
    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    renderToast();
    fireEvent.click(screen.getByTestId("provider-cli-update-dismiss-button"));
    fireEvent.click(screen.getByTestId("dialog-close"));

    expect(mocks.toastDismiss).not.toHaveBeenCalled();
    expect(
      localStorage.getItem(PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY),
    ).toBeNull();
    expect(
      screen.queryByRole("dialog", { name: "Dismiss Claude CLI update?" }),
    ).not.toBeInTheDocument();
  });

  it("dismisses install toasts directly without the update reminder dialog", async () => {
    vi.mocked(providerCliManagementApi.status).mockResolvedValue({
      providers: [
        {
          provider: "codex",
          cliManagementMode: "rx_managed",
          autoUpdateEnabled: false,
          supported: true,
          installed: false,
          binaryPath: null,
          currentVersion: null,
          latestVersion: "0.137.0",
          updateAvailable: false,
          action: "install",
          status: "RX-managed codex is not installed.",
          error: null,
        },
      ],
    });

    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    renderToast();
    fireEvent.click(screen.getByTestId("provider-cli-update-dismiss-button"));

    expect(mocks.toastDismiss).toHaveBeenCalledWith("provider-cli-update:codex");
    expect(
      screen.queryByRole("dialog", { name: "Dismiss Codex CLI update?" }),
    ).not.toBeInTheDocument();
  });

  it("persists don't-ask-again for the current CLI update version", async () => {
    mockUserManagedClaudeUpdate();
    const firstRender = renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    const toastRender = renderToast();
    fireEvent.click(screen.getByTestId("provider-cli-update-dismiss-button"));
    fireEvent.click(screen.getByRole("button", { name: "Don't ask again" }));

    expect(mocks.toastDismiss).toHaveBeenCalledWith("provider-cli-update:claude");
    expect(
      JSON.parse(
        localStorage.getItem(PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY) ?? "[]",
      ),
    ).toEqual(["claude:2.1.175"]);

    firstRender.unmount();
    toastRender.unmount();
    mocks.toast.mockClear();

    renderChecker();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(7_000);
    });

    expect(mocks.toast).not.toHaveBeenCalled();
  });
});
