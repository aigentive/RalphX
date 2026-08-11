import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentProvidersSettingsResponse } from "@/api/harness-providers";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useProviderCliManagement } from "@/hooks/useProviderCliManagement";

import { providerCliUpdateToastId } from "@/lib/provider-cli-update-toast";
import { HarnessProvidersSection } from "./HarnessProvidersSection";

const toastMocks = vi.hoisted(() => ({
  dismiss: vi.fn(),
  error: vi.fn(),
  loading: vi.fn(),
  success: vi.fn(),
}));
const dialogMocks = vi.hoisted(() => ({
  open: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: toastMocks,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: dialogMocks.open,
}));

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: vi.fn(),
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: vi.fn(),
}));

vi.mock("@/hooks/useProviderCliManagement", () => ({
  useProviderCliManagement: vi.fn(),
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: vi.fn(),
}));

const providerUpdatedAt = new Date().toISOString();
const refetchProviders = vi.fn();
const refetchStatus = vi.fn();
const updateProviderAsync = vi.fn();
const installOrUpdateProviderAsync = vi.fn();
const confirm = vi.fn();

if (!HTMLElement.prototype.scrollIntoView) {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    value: vi.fn(),
    writable: true,
  });
}

const settings: AgentProvidersSettingsResponse = {
  defaultProvider: "codex",
  requiresOnboarding: false,
  providers: [
    {
      provider: "codex",
      enabled: true,
      isDefault: true,
      model: "gpt-5.5",
      effort: "xhigh",
      approvalPolicy: "never",
      sandboxMode: "danger-full-access",
      claudePermissionMode: null,
      claudeDangerouslySkipPermissions: false,
      claudeAllowDangerouslySkipPermissions: false,
      cliManagementMode: "rx_managed",
      autoUpdateEnabled: true,
      customBinaryEnabled: false,
      customBinaryPath: null,
      customEnvFileEnabled: false,
      customEnvFilePath: null,
      available: true,
      binaryFound: true,
      binaryPath: "/opt/homebrew/bin/codex",
      status: "Available codex detected at /opt/homebrew/bin/codex.",
      error: null,
      missingCoreExecFeatures: [],
      supportsFastMode: true,
      fastModeSupportedModels: ["gpt-5.5", "gpt-5.4"],
      updatedAt: providerUpdatedAt,
    },
    {
      provider: "claude",
      enabled: false,
      isDefault: false,
      model: "sonnet",
      effort: "medium",
      approvalPolicy: null,
      sandboxMode: null,
      claudePermissionMode: "bypassPermissions",
      claudeDangerouslySkipPermissions: true,
      claudeAllowDangerouslySkipPermissions: true,
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
      customBinaryPath: null,
      customEnvFileEnabled: false,
      customEnvFilePath: null,
      available: false,
      binaryFound: false,
      binaryPath: null,
      status: "Claude CLI not found",
      error: "Claude CLI not found",
      missingCoreExecFeatures: [],
      supportsFastMode: false,
      fastModeSupportedModels: [],
      updatedAt: providerUpdatedAt,
    },
  ],
};

const managedCliStatuses = {
  providers: [
    {
      provider: "codex" as const,
      cliManagementMode: "rx_managed" as const,
      autoUpdateEnabled: true,
      customBinaryEnabled: false,
      customBinaryPath: null,
      customEnvFileEnabled: false,
      customEnvFilePath: null,
      supported: true,
      installed: true,
      binaryPath: "/mock/ralphx/managed-cli/codex/bin/codex",
      currentVersion: "0.136.0",
      latestVersion: "0.137.0",
      updateAvailable: true,
      action: "update" as const,
      status: "RX-managed codex 0.136.0 can update to 0.137.0.",
      error: null,
    },
    {
      provider: "claude" as const,
      cliManagementMode: "user_managed" as const,
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
      customBinaryPath: null,
      customEnvFileEnabled: false,
      customEnvFilePath: null,
      supported: true,
      installed: true,
      binaryPath: "/Users/example/.local/bin/claude",
      currentVersion: "2.1.170",
      latestVersion: "2.1.175",
      updateAvailable: true,
      action: "none" as const,
      status:
        "claude CLI 2.1.170 is user-managed; 2.1.175 is available. RX will not update it unless management is enabled.",
      error: null,
    },
  ],
};

function mockProviders(
  nextSettings: AgentProvidersSettingsResponse = settings,
  overrides: Partial<ReturnType<typeof useHarnessProviders>> = {},
) {
  vi.mocked(useHarnessProviders).mockReturnValue({
    settings: nextSettings,
    providers: nextSettings.providers,
    isLoading: false,
    isPlaceholderData: false,
    isError: false,
    error: null,
    refetchProviders,
    updateProviderAsync,
    isUpdating: false,
    updateError: null,
    ...overrides,
  } as ReturnType<typeof useHarnessProviders>);
}

function mockProviderCliManagement(
  overrides: Partial<ReturnType<typeof useProviderCliManagement>> = {},
) {
  vi.mocked(useProviderCliManagement).mockReturnValue({
    statuses: managedCliStatuses,
    statusByProvider: new Map(
      managedCliStatuses.providers.map((provider) => [
        provider.provider,
        provider,
      ]),
    ),
    isLoadingStatus: false,
    isStatusPlaceholderData: false,
    isStatusError: false,
    statusError: null,
    refetchStatus,
    installOrUpdateProviderAsync,
    isInstallingProvider: false,
    installError: null,
    autoUpdateProvidersAsync: vi.fn(),
    isAutoUpdatingProviders: false,
    autoUpdateError: null,
    ...overrides,
  } as ReturnType<typeof useProviderCliManagement>);
}

function openSelectById(id: string) {
  const trigger = document.getElementById(id);
  expect(trigger).not.toBeNull();
  fireEvent.keyDown(trigger!, { key: "ArrowDown", code: "ArrowDown" });
}

describe("HarnessProvidersSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    confirm.mockResolvedValue(true);
    dialogMocks.open.mockResolvedValue(null);
    updateProviderAsync.mockResolvedValue(settings);
    refetchProviders.mockResolvedValue({ data: settings });
    refetchStatus.mockResolvedValue({ data: managedCliStatuses });
    installOrUpdateProviderAsync.mockResolvedValue({
      provider: "codex",
      success: true,
      status: managedCliStatuses.providers[0],
      stdout: null,
      stderr: null,
    });
    mockProviders();
    mockProviderCliManagement();
    vi.mocked(useAgentModels).mockReturnValue({
      models: [
        {
          provider: "codex",
          modelId: "gpt-5.5",
          menuLabel: "gpt-5.5",
          enabled: true,
          defaultEffort: "xhigh",
          description: "Frontier model for complex coding.",
        },
        {
          provider: "codex",
          modelId: "gpt-5.4",
          menuLabel: "gpt-5.4",
          enabled: true,
          defaultEffort: "high",
          description: "Strong model for everyday coding.",
        },
        {
          provider: "claude",
          modelId: "sonnet",
          menuLabel: "sonnet",
          enabled: true,
          defaultEffort: "medium",
          description: "Claude Sonnet model alias.",
        },
      ],
    } as ReturnType<typeof useAgentModels>);
    vi.mocked(useConfirmation).mockReturnValue({
      confirm,
      confirmationDialogProps: {
        isOpen: false,
        options: null,
        onConfirm: vi.fn(),
        onCancel: vi.fn(),
      },
      ConfirmationDialog: () => null,
    });
  });

  it("shows a loading state while provider settings are placeholder data", () => {
    mockProviders(
      { providers: [], defaultProvider: null, requiresOnboarding: true },
      { isLoading: true, isPlaceholderData: true },
    );

    render(<HarnessProvidersSection />);

    expect(screen.getByTestId("providers-loading-state")).toBeInTheDocument();
    expect(screen.getByText("Loading provider settings")).toBeInTheDocument();
    expect(
      screen.getByText("Checking configured providers and CLI availability."),
    ).toBeInTheDocument();
    expect(screen.queryByText("Default Provider")).not.toBeInTheDocument();
  });

  it("renders provider readiness, defaults, and repair guidance", async () => {
    const user = userEvent.setup();
    render(<HarnessProvidersSection />);

    expect(screen.getByText("Default Provider")).toBeInTheDocument();
    expect(screen.getAllByText("Codex").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Enabled").length).toBeGreaterThan(0);
    expect(screen.getByText("Default")).toBeInTheDocument();
    expect(screen.getByText("CLI Ready")).toBeInTheDocument();
    expect(screen.getByText("/opt/homebrew/bin/codex")).toBeInTheDocument();
    expect(screen.getByText("Available codex detected.")).toBeInTheDocument();
    expect(
      screen.queryByText("Available codex detected at /opt/homebrew/bin/codex."),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Claude CLI not found")).toBeInTheDocument();
    expect(screen.getByText("CLI Not Ready")).toBeInTheDocument();
    expect(
      screen.getByRole("link", { name: /Install instructions/ }),
    ).toHaveAttribute("href", "https://docs.anthropic.com/en/docs/claude-code/setup");
    expect(screen.queryByRole("button", { name: "Apply as Default" })).toBeNull();

    const codexCard = screen.getByTestId("provider-card-codex");
    expect(within(codexCard).getByText("RX-managed CLI")).toBeInTheDocument();
    expect(
      within(codexCard).getByText(
        "RX-managed codex 0.136.0 can update to 0.137.0.",
      ),
    ).toBeInTheDocument();
    expect(
      within(codexCard).getByRole("button", { name: "Update Codex" }),
    ).toBeEnabled();
    expect(
      within(codexCard).getByRole("button", { name: "Apply to all agents" }),
    ).toBeEnabled();

    const claudeCard = screen.getByTestId("provider-card-claude");
    expect(
      within(claudeCard).getByLabelText("Let RX manage this CLI"),
    ).toBeInTheDocument();
    expect(
      within(claudeCard).getByLabelText("Update automatically"),
    ).toBeDisabled();
    expect(
      within(claudeCard).getByText("CLI update available"),
    ).toBeInTheDocument();
    expect(
      within(claudeCard).getByText(
        "claude CLI 2.1.170 is user-managed; 2.1.175 is available. RX will not update it unless management is enabled.",
      ),
    ).toBeInTheDocument();
    expect(
      within(claudeCard).queryByRole("button", { name: "Update Claude" }),
    ).toBeNull();
    expect(within(claudeCard).queryByText("Default Model")).toBeNull();
    expect(within(claudeCard).queryByText("Default Effort")).toBeNull();
    expect(
      within(claudeCard).queryByRole("button", { name: "Show permissions" }),
    ).toBeNull();
    expect(
      within(claudeCard).queryByRole("button", { name: "Reset Claude" }),
    ).toBeNull();
    expect(
      within(claudeCard).queryByRole("button", {
        name: "Apply to all agents",
      }),
    ).toBeNull();

    await user.click(screen.getByRole("button", { name: /Re-check/ }));
    expect(refetchProviders).toHaveBeenCalledWith({ forceRuntime: true });

    await user.click(screen.getAllByRole("switch")[0]!);
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      enabled: false,
    });
  });

  it("disables Codex Fast mode when the provider does not advertise support", () => {
    mockProviders({
      ...settings,
      providers: settings.providers.map((provider) =>
        provider.provider === "codex"
          ? {
              ...provider,
              supportsFastMode: false,
              fastModeSupportedModels: [],
            }
          : provider,
      ),
    });

    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    expect(
      within(codexCard).getByText(
        "Fast mode is not available for this Codex CLI or model catalog.",
      ),
    ).toBeInTheDocument();
    expect(within(codexCard).getByLabelText("Fast mode")).toBeDisabled();
  });

  it("updates managed CLI and auto-update policy controls", async () => {
    const user = userEvent.setup();
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    expect(
      within(codexCard).getByLabelText("Let RX manage this CLI"),
    ).toBeChecked();
    expect(
      within(codexCard).getByLabelText("Update automatically"),
    ).toBeChecked();
    expect(
      within(codexCard).getByText(/RX-managed CLI handling is enabled/i),
    ).toBeInTheDocument();

    await user.click(within(codexCard).getByLabelText("Update automatically"));
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      autoUpdateEnabled: false,
    });

    await user.click(within(codexCard).getByLabelText("Let RX manage this CLI"));
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
    });

    const claudeCard = screen.getByTestId("provider-card-claude");
    expect(
      within(claudeCard).getByLabelText("Update automatically"),
    ).toBeDisabled();
    expect(
      within(claudeCard).getByText(/User-managed CLI installs are never modified/i),
    ).toBeInTheDocument();

    await user.click(
      within(claudeCard).getByLabelText("Let RX manage this CLI"),
    );
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "claude",
      cliManagementMode: "rx_managed",
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
    });
  });

  it("reveals and saves a manually entered custom binary path", async () => {
    const user = userEvent.setup();
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom binary"));

    const pathInput = within(codexCard).getByLabelText("Binary path");
    await user.type(pathInput, "/opt/custom/codex-wrapper");
    await user.click(within(codexCard).getByRole("button", { name: "Use path" }));

    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      customBinaryEnabled: true,
      customBinaryPath: "/opt/custom/codex-wrapper",
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
    });
  });

  it("auto-saves a custom binary path on blur and shows the expanded saved path", async () => {
    const user = userEvent.setup();
    updateProviderAsync.mockResolvedValueOnce({
      ...settings,
      providers: settings.providers.map((provider) =>
        provider.provider === "codex"
          ? {
              ...provider,
              cliManagementMode: "user_managed",
              autoUpdateEnabled: false,
              customBinaryEnabled: true,
              customBinaryPath: "/Users/example/bin/codex-wrapper",
              binaryPath: "/Users/example/bin/codex-wrapper",
            }
          : provider,
      ),
    });
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom binary"));

    const pathInput = within(codexCard).getByLabelText("Binary path");
    await user.type(pathInput, "~/bin/codex-wrapper");
    await user.tab();

    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      customBinaryEnabled: true,
      customBinaryPath: "~/bin/codex-wrapper",
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
    });
    await waitFor(() => {
      expect(pathInput).toHaveValue("/Users/example/bin/codex-wrapper");
    });
  });

  it("shows inline custom binary save errors and keeps the failed draft", async () => {
    const user = userEvent.setup();
    updateProviderAsync.mockRejectedValueOnce(
      new Error("Custom codex binary path must be absolute."),
    );
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom binary"));

    const pathInput = within(codexCard).getByLabelText("Binary path");
    await user.type(pathInput, "relative/codex");
    await user.keyboard("{Enter}");

    expect(
      await within(codexCard).findByText(
        "Custom codex binary path must be absolute.",
      ),
    ).toHaveAttribute("role", "alert");
    expect(pathInput).toHaveValue("relative/codex");
  });

  it("saves a browsed custom binary path", async () => {
    const user = userEvent.setup();
    dialogMocks.open.mockResolvedValueOnce("/opt/custom/codex-wrapper");
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom binary"));
    await user.click(within(codexCard).getByRole("button", { name: "Browse" }));

    expect(dialogMocks.open).toHaveBeenCalledWith({
      directory: false,
      multiple: false,
      title: "Select Codex binary",
    });
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      customBinaryEnabled: true,
      customBinaryPath: "/opt/custom/codex-wrapper",
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
    });
  });

  it("saves the first browsed custom binary path when the dialog returns an array", async () => {
    const user = userEvent.setup();
    dialogMocks.open.mockResolvedValueOnce([
      "/opt/custom/codex-wrapper",
      "/opt/custom/ignored",
    ]);
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom binary"));
    await user.click(within(codexCard).getByRole("button", { name: "Browse" }));

    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      customBinaryEnabled: true,
      customBinaryPath: "/opt/custom/codex-wrapper",
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
    });
  });

  it("does not save a custom binary path when browsing is cancelled", async () => {
    const user = userEvent.setup();
    dialogMocks.open.mockResolvedValueOnce(null);
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom binary"));
    await user.click(within(codexCard).getByRole("button", { name: "Browse" }));

    expect(dialogMocks.open).toHaveBeenCalledWith({
      directory: false,
      multiple: false,
      title: "Select Codex binary",
    });
    expect(updateProviderAsync).not.toHaveBeenCalled();
  });

  it("reveals and saves a manually entered custom env file path", async () => {
    const user = userEvent.setup();
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom env file"));

    const pathInput = within(codexCard).getByLabelText("Env file path");
    await user.type(pathInput, "/Users/example/.codex.env");
    await user.click(within(codexCard).getByRole("button", { name: "Use path" }));

    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      customEnvFileEnabled: true,
      customEnvFilePath: "/Users/example/.codex.env",
    });
    expect(refetchStatus).not.toHaveBeenCalled();
  });

  it("auto-saves a custom env file path on Enter and shows the expanded saved path", async () => {
    const user = userEvent.setup();
    updateProviderAsync.mockResolvedValueOnce({
      ...settings,
      providers: settings.providers.map((provider) =>
        provider.provider === "codex"
          ? {
              ...provider,
              customEnvFileEnabled: true,
              customEnvFilePath: "/Users/example/.codex.env",
            }
          : provider,
      ),
    });
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom env file"));

    const pathInput = within(codexCard).getByLabelText("Env file path");
    await user.type(pathInput, "~/.codex.env");
    await user.keyboard("{Enter}");

    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      customEnvFileEnabled: true,
      customEnvFilePath: "~/.codex.env",
    });
    await waitFor(() => {
      expect(pathInput).toHaveValue("/Users/example/.codex.env");
    });
  });

  it("shows inline custom env file save errors and keeps the failed draft", async () => {
    const user = userEvent.setup();
    updateProviderAsync.mockRejectedValueOnce(
      new Error("Custom codex env file path must be absolute."),
    );
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom env file"));

    const pathInput = within(codexCard).getByLabelText("Env file path");
    await user.type(pathInput, "relative.env");
    await user.keyboard("{Enter}");

    expect(
      await within(codexCard).findByText(
        "Custom codex env file path must be absolute.",
      ),
    ).toHaveAttribute("role", "alert");
    expect(pathInput).toHaveValue("relative.env");
  });

  it("saves a browsed custom env file path", async () => {
    const user = userEvent.setup();
    dialogMocks.open.mockResolvedValueOnce("/Users/example/.codex.env");
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom env file"));
    await user.click(within(codexCard).getByRole("button", { name: "Browse" }));

    expect(dialogMocks.open).toHaveBeenCalledWith({
      directory: false,
      multiple: false,
      title: "Select Codex env file",
    });
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      customEnvFileEnabled: true,
      customEnvFilePath: "/Users/example/.codex.env",
    });
  });

  it("does not save a custom env file path when browsing is cancelled", async () => {
    const user = userEvent.setup();
    dialogMocks.open.mockResolvedValueOnce(null);
    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    await user.click(within(codexCard).getByLabelText("Use custom env file"));
    await user.click(within(codexCard).getByRole("button", { name: "Browse" }));

    expect(dialogMocks.open).toHaveBeenCalledWith({
      directory: false,
      multiple: false,
      title: "Select Codex env file",
    });
    expect(updateProviderAsync).not.toHaveBeenCalled();
  });

  it("renders saved custom env file paths and can disable them", async () => {
    const user = userEvent.setup();
    const customSettings: AgentProvidersSettingsResponse = {
      ...settings,
      providers: settings.providers.map((provider) =>
        provider.provider === "codex"
          ? {
              ...provider,
              customEnvFileEnabled: true,
              customEnvFilePath: "/Users/example/.codex.env",
            }
          : provider,
      ),
    };
    mockProviders(customSettings);

    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    expect(within(codexCard).getByLabelText("Use custom env file")).toBeChecked();
    expect(within(codexCard).getByLabelText("Env file path")).toHaveValue(
      "/Users/example/.codex.env",
    );

    await user.click(within(codexCard).getByLabelText("Use custom env file"));
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      customEnvFileEnabled: false,
    });
  });

  it("disables managed controls and hides update actions for active custom binaries", () => {
    const customSettings: AgentProvidersSettingsResponse = {
      ...settings,
      providers: settings.providers.map((provider) =>
        provider.provider === "codex"
          ? {
              ...provider,
              cliManagementMode: "user_managed",
              autoUpdateEnabled: false,
              customBinaryEnabled: true,
              customBinaryPath: "/opt/custom/codex-wrapper",
              binaryPath: "/opt/custom/codex-wrapper",
              status: "Custom codex CLI 0.144.0 is configured.",
            }
          : provider,
      ),
    };
    const customStatuses = {
      providers: managedCliStatuses.providers.map((provider) =>
        provider.provider === "codex"
          ? {
              ...provider,
              cliManagementMode: "user_managed" as const,
              autoUpdateEnabled: false,
              customBinaryEnabled: true,
              customBinaryPath: "/opt/custom/codex-wrapper",
              binaryPath: "/opt/custom/codex-wrapper",
              updateAvailable: false,
              action: "none" as const,
              status:
                "Custom codex CLI 0.144.0 is configured. RX will not install or update it.",
            }
          : provider,
      ),
    };
    mockProviders(customSettings);
    mockProviderCliManagement({
      statuses: customStatuses,
      statusByProvider: new Map(
        customStatuses.providers.map((provider) => [
          provider.provider,
          provider,
        ]),
      ),
    });

    render(<HarnessProvidersSection />);

    const codexCard = screen.getByTestId("provider-card-codex");
    expect(within(codexCard).getByLabelText("Use custom binary")).toBeChecked();
    expect(
      within(codexCard).getByLabelText("Let RX manage this CLI"),
    ).toBeDisabled();
    expect(
      within(codexCard).getByLabelText("Update automatically"),
    ).toBeDisabled();
    expect(within(codexCard).getByLabelText("Binary path")).toHaveValue(
      "/opt/custom/codex-wrapper",
    );
    expect(
      within(codexCard).queryByRole("button", { name: "Update Codex" }),
    ).toBeNull();
    expect(within(codexCard).queryByText("RX-managed CLI")).toBeNull();
  });

  it("runs a managed CLI update from the provider card", async () => {
    const user = userEvent.setup();
    render(<HarnessProvidersSection />);

    await user.click(
      within(screen.getByTestId("provider-card-codex")).getByRole("button", {
        name: "Update Codex",
      }),
    );

    expect(installOrUpdateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
    });
    await waitFor(() => {
      expect(refetchStatus).toHaveBeenCalled();
      expect(refetchProviders).toHaveBeenCalled();
    });
  });

  it("dismisses the provider update toast when Settings installs the advertised version", async () => {
    const user = userEvent.setup();
    const advertisedStatus = managedCliStatuses.providers[0]!;
    installOrUpdateProviderAsync.mockResolvedValueOnce({
      provider: "codex",
      success: true,
      status: {
        ...advertisedStatus,
        currentVersion: advertisedStatus.latestVersion,
        updateAvailable: false,
        action: "none",
        status: "ready",
      },
      stdout: null,
      stderr: null,
    });

    render(<HarnessProvidersSection />);

    await user.click(
      within(screen.getByTestId("provider-card-codex")).getByRole("button", {
        name: "Update Codex",
      }),
    );

    await waitFor(() => {
      expect(toastMocks.dismiss).toHaveBeenCalledWith(
        providerCliUpdateToastId("codex"),
      );
    });
  });

  it("keeps the provider update toast when Settings installs a different version", async () => {
    const user = userEvent.setup();
    const advertisedStatus = managedCliStatuses.providers[0]!;
    installOrUpdateProviderAsync.mockResolvedValueOnce({
      provider: "codex",
      success: true,
      status: {
        ...advertisedStatus,
        currentVersion: "0.136.5",
        updateAvailable: true,
        action: "update",
        status: "still updating",
      },
      stdout: null,
      stderr: null,
    });

    render(<HarnessProvidersSection />);

    await user.click(
      within(screen.getByTestId("provider-card-codex")).getByRole("button", {
        name: "Update Codex",
      }),
    );

    await waitFor(() => {
      expect(installOrUpdateProviderAsync).toHaveBeenCalledWith({
        provider: "codex",
      });
    });
    expect(toastMocks.dismiss).not.toHaveBeenCalledWith(
      providerCliUpdateToastId("codex"),
    );
  });

  it("shows managed Claude install status with an action", () => {
    const nextSettings: AgentProvidersSettingsResponse = {
      ...settings,
      providers: [
        settings.providers[0]!,
        {
          ...settings.providers[1]!,
          cliManagementMode: "rx_managed",
        },
      ],
    };
    mockProviders(nextSettings);
    mockProviderCliManagement({
      statuses: {
        providers: [
          managedCliStatuses.providers[0]!,
          {
            ...managedCliStatuses.providers[1]!,
            cliManagementMode: "rx_managed",
            installed: false,
            currentVersion: null,
            latestVersion: "2.1.175",
            updateAvailable: false,
            action: "install",
            status: "RX-managed Claude is not installed.",
          },
        ],
      },
      statusByProvider: new Map([
        ["codex", managedCliStatuses.providers[0]!],
        [
          "claude",
          {
            ...managedCliStatuses.providers[1]!,
            cliManagementMode: "rx_managed",
            installed: false,
            currentVersion: null,
            latestVersion: "2.1.175",
            updateAvailable: false,
            action: "install",
            status: "RX-managed Claude is not installed.",
          },
        ],
      ]),
    } as Partial<ReturnType<typeof useProviderCliManagement>>);

    render(<HarnessProvidersSection />);

    const claudeCard = screen.getByTestId("provider-card-claude");
    expect(
      within(claudeCard).getByText("RX-managed Claude is not installed."),
    ).toBeInTheDocument();
    expect(
      within(claudeCard).getByRole("button", { name: "Install Claude" }),
    ).toBeEnabled();
  });

  it("updates provider model settings and applies an enabled provider to all agents", async () => {
    const user = userEvent.setup();
    const nextSettings: AgentProvidersSettingsResponse = {
      ...settings,
      providers: [
        { ...settings.providers[0]!, isDefault: false },
        {
          ...settings.providers[1]!,
          enabled: true,
          available: true,
          binaryFound: true,
          binaryPath: "/opt/homebrew/bin/claude",
          status: "Available claude detected at /opt/homebrew/bin/claude.",
        },
      ],
    };
    mockProviders(nextSettings);
    render(<HarnessProvidersSection />);

    openSelectById("provider-model-codex");
    expect(
      screen.getByRole("option", { name: /Harness default/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", {
        name: /gpt-5\.4.*Strong model for everyday coding\./,
      }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: /Harness default/ }));
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      model: "",
    });

    openSelectById("provider-model-codex");
    await user.click(screen.getByRole("option", { name: /gpt-5\.4/ }));
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      model: "gpt-5.4",
    });

    await user.click(
      within(screen.getByTestId("provider-card-claude")).getByRole("button", {
        name: "Apply to all agents",
      }),
    );

    expect(confirm).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "Apply Claude to all agents?",
        confirmText: "Apply to all agents",
        cancelText: "Cancel",
      }),
    );
    expect(updateProviderAsync).toHaveBeenLastCalledWith({
      provider: "claude",
      isDefault: true,
      applyToAllLanes: true,
    });
  });

  it("keeps a pinned model visible without saving when CLI capability support downgrades", () => {
    const downgradedSettings: AgentProvidersSettingsResponse = {
      ...settings,
      providers: settings.providers.map((provider) =>
        provider.provider === "claude"
          ? {
              ...provider,
              enabled: true,
              available: true,
              binaryFound: true,
              binaryPath: "/opt/homebrew/bin/claude",
              model: "claude-opus-5",
              supportedModelAliases: [
                "sonnet",
                "opus",
                "haiku",
                "claude-opus-4-7",
                "claude-opus-4-8",
              ],
            }
          : provider,
      ),
    };
    mockProviders(downgradedSettings);
    vi.mocked(useAgentModels).mockReturnValue({
      models: [
        {
          provider: "claude",
          modelId: "claude-opus-5",
          menuLabel: "Claude Opus 5",
          enabled: true,
          defaultEffort: "high",
          description: "Pinned Claude Opus 5.",
        },
      ],
    } as ReturnType<typeof useAgentModels>);

    render(<HarnessProvidersSection />);

    const claudeCard = screen.getByTestId("provider-card-claude");
    openSelectById("provider-model-claude");
    expect(within(claudeCard).getByText("claude-opus-5")).toBeInTheDocument();
    expect(updateProviderAsync).not.toHaveBeenCalled();
  });

  it("reapplies the current default provider to all agents", async () => {
    const user = userEvent.setup();
    render(<HarnessProvidersSection />);

    await user.click(
      within(screen.getByTestId("provider-card-codex")).getByRole("button", {
        name: "Apply to all agents",
      }),
    );

    expect(confirm).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "Apply Codex to all agents?",
        confirmText: "Apply to all agents",
      }),
    );
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      isDefault: true,
      applyToAllLanes: true,
    });
  });

  it("updates provider effort, policy, sandbox, and Claude permission defaults", async () => {
    const user = userEvent.setup();
    const nextSettings: AgentProvidersSettingsResponse = {
      ...settings,
      providers: [
        settings.providers[0]!,
        {
          ...settings.providers[1]!,
          enabled: true,
          available: true,
          binaryFound: true,
          binaryPath: "/opt/homebrew/bin/claude",
          status: "Available claude detected at /opt/homebrew/bin/claude.",
        },
      ],
    };
    mockProviders(nextSettings);
    render(<HarnessProvidersSection />);

    openSelectById("provider-effort-codex");
    expect(
      screen.getByRole("option", { name: /Harness default/ }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: /Harness default/ }));
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      effort: "",
    });

    openSelectById("provider-effort-codex");
    await user.click(screen.getByRole("option", { name: "High" }));
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      effort: "high",
    });

    const codexCard = screen.getByTestId("provider-card-codex");
    expect(within(codexCard).getByText("Approval Policy")).not.toBeVisible();
    expect(within(codexCard).getByText("Sandbox Mode")).not.toBeVisible();
    expect(
      within(codexCard).getByText(/RalphX MCP tools currently require Codex/i),
    ).not.toBeVisible();

    await user.click(
      within(codexCard).getByRole("button", { name: "Show permissions" }),
    );
    expect(document.getElementById("codex-approval-policy")).toBeDisabled();
    expect(document.getElementById("codex-sandbox-mode")).toBeDisabled();
    expect(
      within(codexCard).getByText(/RalphX MCP tools currently require Codex/i),
    ).toBeVisible();

    const claudeCard = screen.getByTestId("provider-card-claude");
    expect(within(claudeCard).getByText("Permission Mode")).not.toBeVisible();
    expect(within(claudeCard).getByText("Skip Permissions")).not.toBeVisible();
    await user.click(
      within(claudeCard).getByRole("button", { name: "Show permissions" }),
    );

    openSelectById("claude-permission-mode");
    await user.click(screen.getByRole("option", { name: "default" }));
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "claude",
      claudePermissionMode: "default",
    });

    await user.click(screen.getByLabelText("Skip Permissions"));
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "claude",
      claudeDangerouslySkipPermissions: false,
    });
    expect(
      screen.getByText(/Actually bypasses Claude permission prompts/i),
    ).toBeInTheDocument();

    expect(
      screen.queryByText("Allow Skip Option"),
    ).not.toBeInTheDocument();
  });

  it("resets provider defaults and applies them to lanes when the provider is default", async () => {
    const user = userEvent.setup();
    render(<HarnessProvidersSection />);

    await user.click(screen.getByRole("button", { name: "Reset Codex" }));

    expect(confirm).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "Reset Codex defaults?",
        confirmText: "Reset",
      }),
    );
    expect(updateProviderAsync).toHaveBeenCalledWith({
      provider: "codex",
      resetToDefaults: true,
      applyToAllLanes: true,
    });
  });

  it("shows load and save errors from provider settings", () => {
    mockProviders(settings, {
      isError: true,
      error: new Error("Provider settings failed"),
    });

    render(<HarnessProvidersSection />);

    expect(screen.getByText("Provider settings failed")).toBeInTheDocument();
  });

  it("shows provider update errors", () => {
    mockProviders(settings, {
      updateError: new Error("Provider update failed"),
    });

    render(<HarnessProvidersSection />);

    expect(screen.getByText("Provider update failed")).toBeInTheDocument();
  });
});
