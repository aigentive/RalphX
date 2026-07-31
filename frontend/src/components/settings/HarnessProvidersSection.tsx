import { useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Cpu,
  Download,
  ExternalLink,
  FolderOpen,
  RefreshCw,
  RotateCcw,
} from "lucide-react";
import { toast } from "sonner";

import type {
  AgentProvidersSettingsResponse,
  AgentProviderSettingsResponse,
  UpdateAgentProviderSettingsInput,
} from "@/api/harness-providers";
import type { ManagedProviderCliStatusResponse } from "@/api/provider-cli-management";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { useProviderCliManagement } from "@/hooks/useProviderCliManagement";
import {
  AGENT_EFFORT_CATALOG,
  agentEffortOptionsForModel,
  defaultModelForProvider,
  isAgentModelSelectableForProvider,
  type AgentProvider,
} from "@/lib/agent-models";
import {
  CODEX_FAST_MODE_DESCRIPTION,
  codexFastModeAvailabilityForProvider,
} from "@/lib/codex-fast-mode";
import {
  providerCliUpdateToastId,
  providerCliUpdateToastMatchesInstalledStatus,
} from "@/lib/provider-cli-update-toast";

import { ErrorBanner, SettingsSection } from "./SettingsView.shared";

const PROVIDER_LABELS: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
};

const PROVIDER_INSTALL_LINKS: Record<string, string> = {
  claude: "https://docs.anthropic.com/en/docs/claude-code/setup",
  codex: "https://help.openai.com/en/articles/11096431",
};

const CLAUDE_PERMISSION_MODES = [
  "bypassPermissions",
  "acceptEdits",
  "auto",
  "default",
  "dontAsk",
  "plan",
] as const;

const CODEX_APPROVAL_POLICIES = ["never", "on-request", "on-failure", "untrusted"] as const;
const CODEX_SANDBOX_MODES = ["danger-full-access", "workspace-write", "read-only"] as const;
const PROVIDER_DEFAULT_SELECT_VALUE = "__harness_default__";
const CODEX_MCP_LOCK_COPY =
  "RalphX MCP tools currently require Codex to run with Never approval and Danger Full Access.";
const RX_MANAGED_CLI_MODE = "rx_managed";
const USER_MANAGED_CLI_MODE = "user_managed";

function providerLabel(provider: string): string {
  return PROVIDER_LABELS[provider] ?? provider;
}

function effortLabel(effort: string): string {
  if (effort === PROVIDER_DEFAULT_SELECT_VALUE) return "Harness default";
  return AGENT_EFFORT_CATALOG.find((entry) => entry.id === effort)?.label ?? effort;
}

function isAgentProvider(value: string): value is AgentProvider {
  return value === "claude" || value === "codex";
}

function errorMessage(error: unknown): string {
  return error instanceof Error
    ? error.message
    : "Provider settings update failed.";
}

function ProviderBadge({ provider }: { provider: AgentProviderSettingsResponse }) {
  const tone = provider.enabled
    ? "border-[var(--status-success-border)] text-[var(--status-success)]"
    : provider.available
      ? "border-[var(--border-subtle)] text-[var(--text-muted)]"
      : "border-[var(--status-warning-border)] text-[var(--status-warning)]";

  return (
    <span className={`rounded-md border px-1.5 py-0.5 text-[10px] ${tone}`}>
      {provider.enabled ? "Enabled" : provider.available ? "Ready" : "Not ready"}
    </span>
  );
}

function providerStatusText(provider: AgentProviderSettingsResponse): string {
  const status = provider.status.trim();
  const binaryPath = provider.binaryPath?.trim();
  if (!binaryPath) return status;

  const removePathSuffix = (suffix: string) => {
    if (!status.endsWith(suffix)) return null;
    const trimmed = status.slice(0, status.length - suffix.length).trimEnd();
    return `${trimmed.replace(/\s+at$/i, "")}.`;
  };

  return removePathSuffix(`${binaryPath}.`) ?? removePathSuffix(binaryPath) ?? status;
}

function ProviderCliStatus({
  provider,
}: {
  provider: AgentProviderSettingsResponse;
}) {
  const statusTone = provider.available
    ? "bg-[var(--status-success)]"
    : "bg-[var(--status-warning)]";
  const statusLabel = provider.available ? "CLI Ready" : "CLI Not Ready";
  const statusText = providerStatusText(provider);

  return (
    <div className="space-y-1.5">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className={`h-2 w-2 rounded-full ${statusTone}`} aria-hidden="true" />
        <span className="font-medium text-[var(--text-secondary)]">{statusLabel}</span>
        {provider.binaryPath && (
          <code className="rounded border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-1.5 py-0.5 text-[0.6875rem] text-[var(--text-muted)]">
            {provider.binaryPath}
          </code>
        )}
      </div>
      <p className="text-xs text-[var(--text-muted)]">{statusText}</p>
      {!provider.available && (
        <a
          href={PROVIDER_INSTALL_LINKS[provider.provider]}
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-1 text-xs text-[var(--accent-primary)] hover:underline"
        >
          Install instructions
          <ExternalLink className="h-3 w-3" />
        </a>
      )}
    </div>
  );
}

function ProviderManagedCliStatus({
  provider,
  status,
  isLoading,
  isInstalling,
  onInstallOrUpdate,
}: {
  provider: AgentProviderSettingsResponse;
  status: ManagedProviderCliStatusResponse | undefined;
  isLoading: boolean;
  isInstalling: boolean;
  onInstallOrUpdate: () => void;
}) {
  const label = providerLabel(provider.provider);
  const isUserManagedNotice =
    status?.cliManagementMode === USER_MANAGED_CLI_MODE;
  const canRunAction =
    !isUserManagedNotice &&
    status?.supported &&
    (status.action === "install" || status.action === "update");
  const actionLabel =
    status?.action === "install"
      ? `Install ${label}`
      : status?.action === "update"
        ? `Update ${label}`
        : null;

  return (
    <div className="border-t border-[var(--border-subtle)] px-4 py-3">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2">
        <div className="min-w-0 space-y-1">
          <div className="text-xs font-medium text-[var(--text-primary)]">
            {isUserManagedNotice ? "CLI update available" : "RX-managed CLI"}
          </div>
          <p className="text-[0.6875rem] leading-relaxed text-[var(--text-muted)]">
            {isLoading && !status ? "Checking managed CLI status." : status?.status}
          </p>
          {status?.binaryPath && (
            <code className="block max-w-full truncate rounded border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-1.5 py-0.5 text-[0.6875rem] text-[var(--text-muted)]">
              {status.binaryPath}
            </code>
          )}
          {status?.error && !status.supported && (
            <p className="text-[0.6875rem] leading-relaxed text-[var(--status-warning)]">
              {status.error}
            </p>
          )}
        </div>
        {canRunAction && actionLabel && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={isInstalling}
            onClick={onInstallOrUpdate}
          >
            {status.action === "install" ? (
              <Download className="mr-2 h-4 w-4" />
            ) : (
              <RefreshCw className="mr-2 h-4 w-4" />
            )}
            {actionLabel}
          </Button>
        )}
      </div>
    </div>
  );
}

function ProvidersLoadingState() {
  return (
    <div className="space-y-4" data-testid="providers-loading-state">
      <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2">
        <div className="text-sm font-medium text-[var(--text-primary)]">
          Loading provider settings
        </div>
        <div className="text-xs text-[var(--text-muted)]">
          Checking configured providers and CLI availability.
        </div>
      </div>
      {[1, 2].map((index) => (
        <div
          key={index}
          className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-4 py-4"
        >
          <div className="mb-4 flex items-start justify-between gap-3">
            <div className="space-y-2">
              <div className="h-4 w-28 animate-pulse rounded bg-[var(--overlay-moderate)]" />
              <div className="h-3 w-56 animate-pulse rounded bg-[var(--overlay-moderate)]" />
            </div>
            <div className="h-6 w-11 animate-pulse rounded-full bg-[var(--overlay-moderate)]" />
          </div>
          <div className="grid gap-3 md:grid-cols-2">
            <div className="h-9 animate-pulse rounded-md bg-[var(--overlay-moderate)]" />
            <div className="h-9 animate-pulse rounded-md bg-[var(--overlay-moderate)]" />
          </div>
        </div>
      ))}
    </div>
  );
}

export function HarnessProvidersSection() {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const {
    settings,
    providers,
    isLoading,
    isPlaceholderData,
    isError,
    error,
    updateError,
    updateProviderAsync,
    isUpdating,
    refetchProviders,
  } = useHarnessProviders({ refreshRuntime: true });
  const { models } = useAgentModels();
  const { confirm, confirmationDialogProps, ConfirmationDialog } =
    useConfirmation();
  const {
    statusByProvider,
    isLoadingStatus,
    installOrUpdateProviderAsync,
    isInstallingProvider,
    refetchStatus,
  } = useProviderCliManagement();
  const [expandedPermissions, setExpandedPermissions] = useState<
    Record<string, boolean>
  >({});
  const [customBinaryDrafts, setCustomBinaryDrafts] = useState<
    Record<string, string>
  >({});
  const [customBinaryEditors, setCustomBinaryEditors] = useState<
    Record<string, boolean>
  >({});
  const [customBinaryPathErrors, setCustomBinaryPathErrors] = useState<
    Record<string, string>
  >({});
  const [customEnvFileDrafts, setCustomEnvFileDrafts] = useState<
    Record<string, string>
  >({});
  const [customEnvFileEditors, setCustomEnvFileEditors] = useState<
    Record<string, boolean>
  >({});
  const [customEnvFilePathErrors, setCustomEnvFilePathErrors] = useState<
    Record<string, string>
  >({});
  const customBinarySaveInFlight = useRef<Record<string, string>>({});
  const customEnvFileSaveInFlight = useRef<Record<string, string>>({});

  const displayedError =
    (isError && error instanceof Error ? error.message : null) ??
    (updateError instanceof Error ? updateError.message : null);
  const showLoading = isLoading || isPlaceholderData;

  const updateProvider = async (
    provider: AgentProviderSettingsResponse,
    changes: Partial<AgentProviderSettingsResponse> & {
      resetToDefaults?: boolean;
      applyToAllLanes?: boolean;
    },
  ): Promise<AgentProvidersSettingsResponse> => {
    const input: UpdateAgentProviderSettingsInput = {
      provider: provider.provider,
    };
    if (changes.enabled !== undefined) input.enabled = changes.enabled;
    if (changes.isDefault !== undefined) input.isDefault = changes.isDefault;
    if (changes.model !== undefined) input.model = changes.model;
    if (changes.effort !== undefined) input.effort = changes.effort;
    if (changes.serviceTier !== undefined) {
      input.serviceTier = changes.serviceTier;
    }
    if (changes.approvalPolicy !== undefined) {
      input.approvalPolicy = changes.approvalPolicy;
    }
    if (changes.sandboxMode !== undefined) input.sandboxMode = changes.sandboxMode;
    if (changes.claudePermissionMode !== undefined) {
      input.claudePermissionMode = changes.claudePermissionMode;
    }
    if (changes.claudeDangerouslySkipPermissions !== undefined) {
      input.claudeDangerouslySkipPermissions =
        changes.claudeDangerouslySkipPermissions;
    }
    if (changes.claudeAllowDangerouslySkipPermissions !== undefined) {
      input.claudeAllowDangerouslySkipPermissions =
        changes.claudeAllowDangerouslySkipPermissions;
    }
    if (changes.cliManagementMode !== undefined) {
      input.cliManagementMode = changes.cliManagementMode;
    }
    if (changes.autoUpdateEnabled !== undefined) {
      input.autoUpdateEnabled = changes.autoUpdateEnabled;
    }
    if (changes.customBinaryEnabled !== undefined) {
      input.customBinaryEnabled = changes.customBinaryEnabled;
    }
    if (changes.customBinaryPath !== undefined) {
      input.customBinaryPath = changes.customBinaryPath;
    }
    if (changes.customEnvFileEnabled !== undefined) {
      input.customEnvFileEnabled = changes.customEnvFileEnabled;
    }
    if (changes.customEnvFilePath !== undefined) {
      input.customEnvFilePath = changes.customEnvFilePath;
    }
    if (changes.resetToDefaults !== undefined) {
      input.resetToDefaults = changes.resetToDefaults;
    }
    if (changes.applyToAllLanes !== undefined) {
      input.applyToAllLanes = changes.applyToAllLanes;
    }
    const response = await updateProviderAsync(input);
    if (
      changes.cliManagementMode !== undefined ||
      changes.autoUpdateEnabled !== undefined ||
      changes.customBinaryEnabled !== undefined ||
      changes.customBinaryPath !== undefined
    ) {
      await refetchStatus();
    }
    return response;
  };

  const applyProviderToAgents = async (
    provider: AgentProviderSettingsResponse,
  ) => {
    const label = providerLabel(provider.provider);
    const confirmed = await confirm({
      title: `Apply ${label} to all agents?`,
      description: provider.isDefault
        ? `Update every agent lane to use ${label} with this provider's current defaults.`
        : `Make ${label} the default provider and update every agent lane to use this provider's current defaults.`,
      confirmText: "Apply to all agents",
      cancelText: "Cancel",
    });
    if (!confirmed) return;
    await updateProvider(provider, {
      isDefault: true,
      applyToAllLanes: true,
    });
  };

  const resetProviderDefaults = async (
    provider: AgentProviderSettingsResponse,
  ) => {
    const confirmed = await confirm({
      title: `Reset ${providerLabel(provider.provider)} defaults?`,
      description: provider.isDefault
        ? "Restore this provider's built-in defaults and apply them to all agent lanes."
        : "Restore this provider's built-in defaults without changing enabled/default status.",
      confirmText: "Reset",
      cancelText: "Cancel",
    });
    if (!confirmed) return;
    await updateProvider(provider, {
      resetToDefaults: true,
      applyToAllLanes: provider.isDefault,
    });
  };

  const installOrUpdateManagedCli = async (
    provider: AgentProviderSettingsResponse,
  ) => {
    const status = statusByProvider.get(provider.provider);
    const actionVerb = status?.action === "install" ? "Installing" : "Updating";
    const label = providerLabel(provider.provider);
    const toastId = `provider-cli-management:${provider.provider}`;
    toast.loading(`${actionVerb} ${label} CLI...`, { id: toastId });
    try {
      const result = await installOrUpdateProviderAsync({
        provider: provider.provider,
      });
      if (providerCliUpdateToastMatchesInstalledStatus(status, result.status)) {
        toast.dismiss(providerCliUpdateToastId(provider.provider));
      }
      toast.success(`${label} CLI is ready.`, { id: toastId });
      await Promise.all([refetchStatus(), refetchProviders()]);
    } catch (error) {
      toast.error(`Failed to update ${label} CLI.`, {
        id: toastId,
        description: error instanceof Error ? error.message : undefined,
      });
    }
  };

  const customBinaryDraft = (provider: AgentProviderSettingsResponse) =>
    customBinaryDrafts[provider.provider] ?? provider.customBinaryPath ?? "";

  const showCustomBinaryEditor = (provider: AgentProviderSettingsResponse) =>
    Boolean(
      provider.customBinaryEnabled ||
        provider.customBinaryPath ||
        customBinaryEditors[provider.provider],
    );

  const setCustomBinaryDraft = (provider: string, value: string) => {
    setCustomBinaryDrafts((current) => ({ ...current, [provider]: value }));
  };

  const setCustomBinaryPathError = (provider: string, error: string | null) => {
    setCustomBinaryPathErrors((current) => {
      const next = { ...current };
      if (error) {
        next[provider] = error;
      } else {
        delete next[provider];
      }
      return next;
    });
  };

  const setCustomBinaryEditor = (provider: string, visible: boolean) => {
    setCustomBinaryEditors((current) => ({ ...current, [provider]: visible }));
  };

  const saveCustomBinaryPath = async (
    provider: AgentProviderSettingsResponse,
    path: string,
  ) => {
    const trimmedPath = path.trim();
    if (!trimmedPath) {
      setCustomBinaryEditor(provider.provider, true);
      setCustomBinaryPathError(provider.provider, "Binary path is required.");
      return;
    }
    if (
      provider.customBinaryEnabled &&
      provider.customBinaryPath?.trim() === trimmedPath
    ) {
      setCustomBinaryDraft(provider.provider, trimmedPath);
      setCustomBinaryPathError(provider.provider, null);
      return;
    }
    if (customBinarySaveInFlight.current[provider.provider] === trimmedPath) {
      return;
    }
    customBinarySaveInFlight.current[provider.provider] = trimmedPath;
    setCustomBinaryDraft(provider.provider, trimmedPath);
    setCustomBinaryPathError(provider.provider, null);
    try {
      const response = await updateProvider(provider, {
        customBinaryEnabled: true,
        customBinaryPath: trimmedPath,
        cliManagementMode: USER_MANAGED_CLI_MODE,
        autoUpdateEnabled: false,
      });
      const savedProvider = response.providers.find(
        (item) => item.provider === provider.provider,
      );
      setCustomBinaryDraft(
        provider.provider,
        savedProvider?.customBinaryPath ?? trimmedPath,
      );
    } catch (error) {
      setCustomBinaryEditor(provider.provider, true);
      setCustomBinaryDraft(provider.provider, trimmedPath);
      setCustomBinaryPathError(provider.provider, errorMessage(error));
    } finally {
      if (customBinarySaveInFlight.current[provider.provider] === trimmedPath) {
        delete customBinarySaveInFlight.current[provider.provider];
      }
    }
  };

  const toggleCustomBinary = async (
    provider: AgentProviderSettingsResponse,
    checked: boolean,
  ) => {
    if (!checked) {
      await updateProvider(provider, { customBinaryEnabled: false });
      return;
    }
    setCustomBinaryEditor(provider.provider, true);
    const path = customBinaryDraft(provider);
    if (path.trim()) {
      await saveCustomBinaryPath(provider, path);
    }
  };

  const browseCustomBinary = async (provider: AgentProviderSettingsResponse) => {
    const selected = await openDialog({
      directory: false,
      multiple: false,
      title: `Select ${providerLabel(provider.provider)} binary`,
    });
    const selectedPath = Array.isArray(selected) ? selected[0] : selected;
    if (typeof selectedPath !== "string" || selectedPath.trim() === "") return;
    await saveCustomBinaryPath(provider, selectedPath);
  };

  const customEnvFileDraft = (provider: AgentProviderSettingsResponse) =>
    customEnvFileDrafts[provider.provider] ?? provider.customEnvFilePath ?? "";

  const showCustomEnvFileEditor = (provider: AgentProviderSettingsResponse) =>
    Boolean(
      provider.customEnvFileEnabled ||
        provider.customEnvFilePath ||
        customEnvFileEditors[provider.provider],
    );

  const setCustomEnvFileDraft = (provider: string, value: string) => {
    setCustomEnvFileDrafts((current) => ({ ...current, [provider]: value }));
  };

  const setCustomEnvFilePathError = (provider: string, error: string | null) => {
    setCustomEnvFilePathErrors((current) => {
      const next = { ...current };
      if (error) {
        next[provider] = error;
      } else {
        delete next[provider];
      }
      return next;
    });
  };

  const setCustomEnvFileEditor = (provider: string, visible: boolean) => {
    setCustomEnvFileEditors((current) => ({ ...current, [provider]: visible }));
  };

  const saveCustomEnvFilePath = async (
    provider: AgentProviderSettingsResponse,
    path: string,
  ) => {
    const trimmedPath = path.trim();
    if (!trimmedPath) {
      setCustomEnvFileEditor(provider.provider, true);
      setCustomEnvFilePathError(provider.provider, "Env file path is required.");
      return;
    }
    if (
      provider.customEnvFileEnabled &&
      provider.customEnvFilePath?.trim() === trimmedPath
    ) {
      setCustomEnvFileDraft(provider.provider, trimmedPath);
      setCustomEnvFilePathError(provider.provider, null);
      return;
    }
    if (customEnvFileSaveInFlight.current[provider.provider] === trimmedPath) {
      return;
    }
    customEnvFileSaveInFlight.current[provider.provider] = trimmedPath;
    setCustomEnvFileDraft(provider.provider, trimmedPath);
    setCustomEnvFilePathError(provider.provider, null);
    try {
      const response = await updateProvider(provider, {
        customEnvFileEnabled: true,
        customEnvFilePath: trimmedPath,
      });
      const savedProvider = response.providers.find(
        (item) => item.provider === provider.provider,
      );
      setCustomEnvFileDraft(
        provider.provider,
        savedProvider?.customEnvFilePath ?? trimmedPath,
      );
    } catch (error) {
      setCustomEnvFileEditor(provider.provider, true);
      setCustomEnvFileDraft(provider.provider, trimmedPath);
      setCustomEnvFilePathError(provider.provider, errorMessage(error));
    } finally {
      if (customEnvFileSaveInFlight.current[provider.provider] === trimmedPath) {
        delete customEnvFileSaveInFlight.current[provider.provider];
      }
    }
  };

  const toggleCustomEnvFile = async (
    provider: AgentProviderSettingsResponse,
    checked: boolean,
  ) => {
    if (!checked) {
      await updateProvider(provider, { customEnvFileEnabled: false });
      return;
    }
    setCustomEnvFileEditor(provider.provider, true);
    const path = customEnvFileDraft(provider);
    if (path.trim()) {
      await saveCustomEnvFilePath(provider, path);
    }
  };

  const browseCustomEnvFile = async (
    provider: AgentProviderSettingsResponse,
  ) => {
    const selected = await openDialog({
      directory: false,
      multiple: false,
      title: `Select ${providerLabel(provider.provider)} env file`,
    });
    const selectedPath = Array.isArray(selected) ? selected[0] : selected;
    if (typeof selectedPath !== "string" || selectedPath.trim() === "") return;
    await saveCustomEnvFilePath(provider, selectedPath);
  };

  // Provider settings are `Denied` on the remote facade by design — the command probes
  // provider CLIs and carries provider identities, models, CLI paths, and the credential
  // surface. So this is not a transient unavailability to spin on: under a remote environment
  // the read never answers, and the pane used to sit on its skeleton forever. Say whose
  // machine owns the setting instead, and render nothing that implies it is loading.
  if (isRemoteEnvironment) {
    // The dialog-level strip already says whose settings these are, so this pane only needs
    // to name what specifically is unavailable — not repeat the banner.
    return (
      <SettingsSection>
        <p
          className="text-sm text-[var(--text-muted)]"
          data-testid="providers-host-only-note"
        >
          Harness providers, models, and CLI availability are configured on the host and
          cannot be read from here.
        </p>
      </SettingsSection>
    );
  }

  return (
    <SettingsSection>
      {displayedError && (
        <ErrorBanner error={displayedError} onDismiss={() => undefined} />
      )}

      {showLoading ? (
        <ProvidersLoadingState />
      ) : (
        <>
          <div className="mb-4 flex items-center justify-between gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2">
            <div className="min-w-0">
              <div className="text-sm font-medium text-[var(--text-primary)]">
                Default Provider
              </div>
              <div className="truncate text-xs text-[var(--text-muted)]">
                {settings.defaultProvider
                  ? providerLabel(settings.defaultProvider)
                  : "No enabled default provider"}
              </div>
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void refetchProviders()}
            >
              <RefreshCw className="mr-2 h-4 w-4" />
              Re-check
            </Button>
          </div>

          <div className="space-y-4">
            {providers.map((provider) => {
              const agentProvider = isAgentProvider(provider.provider)
                ? provider.provider
                : "claude";
              const providerModels = models.filter(
                (model) =>
                  model.provider === provider.provider &&
                  model.enabled &&
                  isAgentModelSelectableForProvider(
                    agentProvider,
                    model.modelId,
                    provider.supportedModelAliases,
                  ),
              );
              const selectedModel =
                provider.model ?? PROVIDER_DEFAULT_SELECT_VALUE;
              const selectedModelEntry = providerModels.find(
                (model) => model.modelId === selectedModel,
              );
              const selectedModelId =
                provider.model ??
                defaultModelForProvider(
                  agentProvider,
                  undefined,
                  provider.supportedModelAliases,
                );
              const effortOptions = agentEffortOptionsForModel(
                agentProvider,
                selectedModelId,
                undefined,
                provider.supportedEfforts,
              );
              const codexFastModeAvailability =
                provider.provider === "codex"
                  ? codexFastModeAvailabilityForProvider({
                      provider,
                      modelId: selectedModelId,
                      isReady: !showLoading,
                    })
                  : null;
              const codexFastModeChecked = provider.serviceTier === "fast";
              const codexFastModeDisabled =
                isUpdating ||
                (!codexFastModeChecked &&
                  codexFastModeAvailability?.supported === false);
              const codexFastModeDescription =
                codexFastModeAvailability?.reason ??
                CODEX_FAST_MODE_DESCRIPTION;
              const selectedEffort =
                provider.effort ?? PROVIDER_DEFAULT_SELECT_VALUE;
              const hasCustomModel =
                provider.model != null &&
                provider.model.trim() !== "" &&
                selectedModelEntry == null;
              const isRxManagedCli =
                provider.cliManagementMode === RX_MANAGED_CLI_MODE &&
                !provider.customBinaryEnabled;
              const managedCliStatus = statusByProvider.get(provider.provider);
              const showCustomEditor = showCustomBinaryEditor(provider);
              const customPath = customBinaryDraft(provider);
              const customPathError =
                customBinaryPathErrors[provider.provider];
              const showEnvFileEditor = showCustomEnvFileEditor(provider);
              const customEnvFilePath = customEnvFileDraft(provider);
              const customEnvFilePathError =
                customEnvFilePathErrors[provider.provider];

              return (
                <div
                  key={provider.provider}
                  data-testid={`provider-card-${provider.provider}`}
                  className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]"
                >
                  <div
                    className={`flex flex-wrap items-start justify-between gap-3 px-4 py-3 ${
                      provider.enabled
                        ? "border-b border-[var(--border-subtle)]"
                        : ""
                    }`}
                  >
                    <div className="min-w-0 space-y-1">
                      <div className="flex items-center gap-2">
                        <Cpu className="h-4 w-4 text-[var(--text-muted)]" />
                        <h4 className="text-sm font-semibold text-[var(--text-primary)]">
                          {providerLabel(provider.provider)}
                        </h4>
                        <ProviderBadge provider={provider} />
                        {provider.isDefault && (
                          <span className="rounded-md border border-[var(--accent-primary)] px-1.5 py-0.5 text-[10px] text-[var(--accent-primary)]">
                            Default
                          </span>
                        )}
                      </div>
                      <ProviderCliStatus provider={provider} />
                    </div>
                    <div className="flex items-center gap-3">
                      <Label
                        htmlFor={`provider-enabled-${provider.provider}`}
                        className="text-xs text-[var(--text-muted)]"
                      >
                        Enabled
                      </Label>
                      <Switch
                        id={`provider-enabled-${provider.provider}`}
                        checked={provider.enabled}
                        disabled={isUpdating || !provider.available}
                        onCheckedChange={(checked) =>
                          void updateProvider(provider, { enabled: checked })
                        }
                      />
                    </div>
                  </div>

                  <div
                    className={`grid gap-3 border-t border-[var(--border-subtle)] px-4 py-3 md:grid-cols-2 ${
                      provider.enabled
                        ? "border-b border-[var(--border-subtle)]"
                        : ""
                    }`}
                  >
                    <div className="flex items-start justify-between gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2">
                      <div className="space-y-1">
                        <Label
                          htmlFor={`provider-managed-cli-${provider.provider}`}
                          className="text-xs text-[var(--text-primary)]"
                        >
                          Let RX manage this CLI
                        </Label>
                        <p className="text-[0.6875rem] leading-relaxed text-[var(--text-muted)]">
                          Allows RX-managed installs and updates without
                          changing user-managed PATH installs.
                        </p>
                      </div>
                      <Switch
                        id={`provider-managed-cli-${provider.provider}`}
                        checked={isRxManagedCli}
                        disabled={isUpdating || Boolean(provider.customBinaryEnabled)}
                        onCheckedChange={(checked) =>
                          void updateProvider(provider, {
                            customBinaryEnabled: false,
                            cliManagementMode: checked
                              ? RX_MANAGED_CLI_MODE
                              : USER_MANAGED_CLI_MODE,
                            autoUpdateEnabled: checked
                              ? Boolean(provider.autoUpdateEnabled)
                              : false,
                          })
                        }
                      />
                    </div>

                    <div className="flex items-start justify-between gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2">
                      <div className="space-y-1">
                        <Label
                          htmlFor={`provider-custom-binary-${provider.provider}`}
                          className="text-xs text-[var(--text-primary)]"
                        >
                          Use custom binary
                        </Label>
                        <p className="text-[0.6875rem] leading-relaxed text-[var(--text-muted)]">
                          Launch this provider from a user-owned executable or
                          wrapper.
                        </p>
                      </div>
                      <Switch
                        id={`provider-custom-binary-${provider.provider}`}
                        checked={Boolean(provider.customBinaryEnabled)}
                        disabled={isUpdating}
                        onCheckedChange={(checked) =>
                          void toggleCustomBinary(provider, checked)
                        }
                      />
                    </div>

                    <div className="flex items-start justify-between gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2">
                      <div className="space-y-1">
                        <Label
                          htmlFor={`provider-custom-env-file-${provider.provider}`}
                          className="text-xs text-[var(--text-primary)]"
                        >
                          Use custom env file
                        </Label>
                        <p className="text-[0.6875rem] leading-relaxed text-[var(--text-muted)]">
                          Read provider credential and transport variables at
                          launch. RX still controls model selection.
                        </p>
                      </div>
                      <Switch
                        id={`provider-custom-env-file-${provider.provider}`}
                        checked={Boolean(provider.customEnvFileEnabled)}
                        disabled={isUpdating}
                        onCheckedChange={(checked) =>
                          void toggleCustomEnvFile(provider, checked)
                        }
                      />
                    </div>

                    <div
                      className={`flex items-start justify-between gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2 ${
                        isRxManagedCli ? "" : "opacity-75"
                      }`}
                    >
                      <div className="space-y-1">
                        <Label
                          htmlFor={`provider-auto-update-${provider.provider}`}
                          className="text-xs text-[var(--text-primary)]"
                        >
                          Update automatically
                        </Label>
                        <p className="text-[0.6875rem] leading-relaxed text-[var(--text-muted)]">
                          Runs only when RX-managed CLI handling is enabled.
                          User-managed CLI installs are never modified.
                        </p>
                      </div>
                      <Switch
                        id={`provider-auto-update-${provider.provider}`}
                        checked={
                          isRxManagedCli && Boolean(provider.autoUpdateEnabled)
                        }
                        disabled={isUpdating || !isRxManagedCli}
                        onCheckedChange={(checked) =>
                          void updateProvider(provider, {
                            autoUpdateEnabled: checked,
                          })
                        }
                      />
                    </div>

                    {showCustomEditor && (
                      <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2 md:col-span-2">
                        <div className="flex flex-col gap-2 md:flex-row md:items-end">
                          <div className="min-w-0 flex-1 space-y-1">
                            <Label
                              htmlFor={`provider-custom-binary-path-${provider.provider}`}
                              className="text-xs text-[var(--text-primary)]"
                            >
                              Binary path
                            </Label>
                            <Input
                              id={`provider-custom-binary-path-${provider.provider}`}
                              value={customPath}
                              disabled={isUpdating}
                              placeholder={`/path/to/${provider.provider}-wrapper`}
                              onChange={(event) => {
                                setCustomBinaryDraft(
                                  provider.provider,
                                  event.target.value,
                                );
                                setCustomBinaryPathError(
                                  provider.provider,
                                  null,
                                );
                              }}
                              onBlur={(event) =>
                                void saveCustomBinaryPath(
                                  provider,
                                  event.currentTarget.value,
                                )
                              }
                              onKeyDown={(event) => {
                                if (event.key !== "Enter") return;
                                event.preventDefault();
                                void saveCustomBinaryPath(
                                  provider,
                                  event.currentTarget.value,
                                );
                              }}
                            />
                            {customPathError && (
                              <p
                                role="alert"
                                className="text-[0.6875rem] leading-relaxed text-[var(--status-error)]"
                              >
                                {customPathError}
                              </p>
                            )}
                          </div>
                          <div className="flex flex-wrap gap-2">
                            <Button
                              type="button"
                              variant="outline"
                              size="sm"
                              disabled={isUpdating}
                              onClick={() => void browseCustomBinary(provider)}
                            >
                              <FolderOpen className="mr-2 h-4 w-4" />
                              Browse
                            </Button>
                            <Button
                              type="button"
                              variant="outline"
                              size="sm"
                              disabled={isUpdating || customPath.trim() === ""}
                              onClick={() =>
                                void saveCustomBinaryPath(provider, customPath)
                              }
                            >
                              <Check className="mr-2 h-4 w-4" />
                              Use path
                            </Button>
                          </div>
                        </div>
                      </div>
                    )}

                    {showEnvFileEditor && (
                      <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2 md:col-span-2">
                        <div className="flex flex-col gap-2 md:flex-row md:items-end">
                          <div className="min-w-0 flex-1 space-y-1">
                            <Label
                              htmlFor={`provider-custom-env-file-path-${provider.provider}`}
                              className="text-xs text-[var(--text-primary)]"
                            >
                              Env file path
                            </Label>
                            <Input
                              id={`provider-custom-env-file-path-${provider.provider}`}
                              value={customEnvFilePath}
                              disabled={isUpdating}
                              placeholder="/path/to/.env"
                              onChange={(event) => {
                                setCustomEnvFileDraft(
                                  provider.provider,
                                  event.target.value,
                                );
                                setCustomEnvFilePathError(
                                  provider.provider,
                                  null,
                                );
                              }}
                              onBlur={(event) =>
                                void saveCustomEnvFilePath(
                                  provider,
                                  event.currentTarget.value,
                                )
                              }
                              onKeyDown={(event) => {
                                if (event.key !== "Enter") return;
                                event.preventDefault();
                                void saveCustomEnvFilePath(
                                  provider,
                                  event.currentTarget.value,
                                );
                              }}
                            />
                            {customEnvFilePathError && (
                              <p
                                role="alert"
                                className="text-[0.6875rem] leading-relaxed text-[var(--status-error)]"
                              >
                                {customEnvFilePathError}
                              </p>
                            )}
                          </div>
                          <div className="flex flex-wrap gap-2">
                            <Button
                              type="button"
                              variant="outline"
                              size="sm"
                              disabled={isUpdating}
                              onClick={() => void browseCustomEnvFile(provider)}
                            >
                              <FolderOpen className="mr-2 h-4 w-4" />
                              Browse
                            </Button>
                            <Button
                              type="button"
                              variant="outline"
                              size="sm"
                              disabled={
                                isUpdating || customEnvFilePath.trim() === ""
                              }
                              onClick={() =>
                                void saveCustomEnvFilePath(
                                  provider,
                                  customEnvFilePath,
                                )
                              }
                            >
                              <Check className="mr-2 h-4 w-4" />
                              Use path
                            </Button>
                          </div>
                        </div>
                      </div>
                    )}
                  </div>

                  {(isRxManagedCli ||
                    (!provider.customBinaryEnabled &&
                      managedCliStatus?.updateAvailable)) && (
                    <ProviderManagedCliStatus
                      provider={provider}
                      status={managedCliStatus}
                      isLoading={isLoadingStatus}
                      isInstalling={isInstallingProvider}
                      onInstallOrUpdate={() =>
                        void installOrUpdateManagedCli(provider)
                      }
                    />
                  )}

                  {provider.enabled && (
                    <>
                      <div className="grid gap-3 px-4 py-4 md:grid-cols-2">
                        <div className="space-y-1">
                          <Label htmlFor={`provider-model-${provider.provider}`}>
                            Default Model
                          </Label>
                          <Select
                            value={selectedModel}
                            onValueChange={(model) =>
                              void updateProvider(provider, {
                                model:
                                  model === PROVIDER_DEFAULT_SELECT_VALUE
                                    ? ""
                                    : model,
                              })
                            }
                            disabled={isUpdating || providerModels.length === 0}
                          >
                            <SelectTrigger
                              id={`provider-model-${provider.provider}`}
                            >
                              <SelectValue>
                                <span className="truncate">
                                  {provider.model == null
                                    ? "Harness default"
                                    : selectedModelEntry?.menuLabel ??
                                      provider.model}
                                </span>
                              </SelectValue>
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem
                                value={PROVIDER_DEFAULT_SELECT_VALUE}
                                textValue="Harness default"
                              >
                                <div className="flex flex-col">
                                  <span className="text-[var(--text-primary)]">
                                    Harness default
                                  </span>
                                  <span className="text-xs text-[var(--text-muted)]">
                                    Use the provider's built-in default model.
                                  </span>
                                </div>
                              </SelectItem>
                              {providerModels.map((model) => (
                                <SelectItem
                                  key={model.modelId}
                                  value={model.modelId}
                                  textValue={model.menuLabel}
                                >
                                  <div className="flex flex-col">
                                    <span className="text-[var(--text-primary)]">
                                      {model.menuLabel}
                                    </span>
                                    {model.description && (
                                      <span className="text-xs text-[var(--text-muted)]">
                                        {model.description}
                                      </span>
                                    )}
                                  </div>
                                </SelectItem>
                              ))}
                              {hasCustomModel && provider.model && (
                                <SelectItem
                                  value={provider.model}
                                  textValue={provider.model}
                                >
                                  <div className="flex flex-col">
                                    <span className="text-[var(--text-primary)]">
                                      Custom model
                                    </span>
                                    <span className="text-xs text-[var(--text-muted)]">
                                      {provider.model}
                                    </span>
                                  </div>
                                </SelectItem>
                              )}
                            </SelectContent>
                          </Select>
                        </div>

                        <div className="space-y-1">
                          <Label
                            htmlFor={`provider-effort-${provider.provider}`}
                          >
                            Default Effort
                          </Label>
                          <Select
                            value={selectedEffort}
                            onValueChange={(effort) =>
                              void updateProvider(provider, {
                                effort:
                                  effort === PROVIDER_DEFAULT_SELECT_VALUE
                                    ? ""
                                    : effort,
                              })
                            }
                            disabled={isUpdating}
                          >
                            <SelectTrigger
                              id={`provider-effort-${provider.provider}`}
                            >
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem
                                value={PROVIDER_DEFAULT_SELECT_VALUE}
                                textValue="Harness default"
                              >
                                <div className="flex flex-col">
                                  <span className="text-[var(--text-primary)]">
                                    Harness default
                                  </span>
                                  <span className="text-xs text-[var(--text-muted)]">
                                    Use the provider's built-in default effort.
                                  </span>
                                </div>
                              </SelectItem>
                              {effortOptions.map((effort) => (
                                <SelectItem key={effort.id} value={effort.id}>
                                  {effortLabel(effort.id)}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        </div>

                        {provider.provider === "codex" && (
                          <>
                            <div className="flex items-start justify-between gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2 md:col-span-2">
                              <div className="space-y-1">
                                <Label
                                  htmlFor="codex-fast-mode"
                                  className="text-xs text-[var(--text-primary)]"
                                >
                                  Fast mode
                                </Label>
                                <p className="text-[0.6875rem] leading-relaxed text-[var(--text-muted)]">
                                  {codexFastModeDescription}
                                </p>
                              </div>
                              <Switch
                                id="codex-fast-mode"
                                data-testid="codex-provider-fast-mode"
                                checked={codexFastModeChecked}
                                disabled={codexFastModeDisabled}
                                onCheckedChange={(checked) =>
                                  void updateProvider(provider, {
                                    serviceTier: checked ? "fast" : null,
                                  })
                                }
                              />
                            </div>
                            <ProviderPermissionDisclosure
                              provider={provider.provider}
                              expanded={
                                !!expandedPermissions[provider.provider]
                              }
                              onToggle={() =>
                                setExpandedPermissions((current) => ({
                                  ...current,
                                  [provider.provider]:
                                    !current[provider.provider],
                                }))
                              }
                            />
                            <div
                              id={`provider-permissions-${provider.provider}`}
                              hidden={!expandedPermissions[provider.provider]}
                              className="grid gap-3 md:col-span-2 md:grid-cols-2"
                            >
                              <div className="space-y-1">
                                <Label htmlFor="codex-approval-policy">
                                  Approval Policy
                                </Label>
                                <Select
                                  value={provider.approvalPolicy ?? "never"}
                                  onValueChange={() => undefined}
                                  disabled
                                >
                                  <SelectTrigger id="codex-approval-policy">
                                    <SelectValue />
                                  </SelectTrigger>
                                  <SelectContent>
                                    {CODEX_APPROVAL_POLICIES.map((policy) => (
                                      <SelectItem key={policy} value={policy}>
                                        {policy}
                                      </SelectItem>
                                    ))}
                                  </SelectContent>
                                </Select>
                              </div>

                              <div className="space-y-1">
                                <Label htmlFor="codex-sandbox-mode">
                                  Sandbox Mode
                                </Label>
                                <Select
                                  value={
                                    provider.sandboxMode ??
                                    "danger-full-access"
                                  }
                                  onValueChange={() => undefined}
                                  disabled
                                >
                                  <SelectTrigger id="codex-sandbox-mode">
                                    <SelectValue />
                                  </SelectTrigger>
                                  <SelectContent>
                                    {CODEX_SANDBOX_MODES.map((mode) => (
                                      <SelectItem key={mode} value={mode}>
                                        {mode}
                                      </SelectItem>
                                    ))}
                                  </SelectContent>
                                </Select>
                              </div>
                              <p className="text-xs text-[var(--text-muted)] md:col-span-2">
                                {CODEX_MCP_LOCK_COPY}
                              </p>
                            </div>
                          </>
                        )}

                        {provider.provider === "claude" && (
                          <>
                            <ProviderPermissionDisclosure
                              provider={provider.provider}
                              expanded={
                                !!expandedPermissions[provider.provider]
                              }
                              onToggle={() =>
                                setExpandedPermissions((current) => ({
                                  ...current,
                                  [provider.provider]:
                                    !current[provider.provider],
                                }))
                              }
                            />
                            <div
                              id={`provider-permissions-${provider.provider}`}
                              hidden={!expandedPermissions[provider.provider]}
                              className="grid gap-3 md:col-span-2 md:grid-cols-2"
                            >
                              <div className="space-y-1">
                                <Label htmlFor="claude-permission-mode">
                                  Permission Mode
                                </Label>
                                <Select
                                  value={
                                    provider.claudePermissionMode ??
                                    "bypassPermissions"
                                  }
                                  onValueChange={(claudePermissionMode) =>
                                    void updateProvider(provider, {
                                      claudePermissionMode,
                                    })
                                  }
                                  disabled={isUpdating}
                                >
                                  <SelectTrigger id="claude-permission-mode">
                                    <SelectValue />
                                  </SelectTrigger>
                                  <SelectContent>
                                    {CLAUDE_PERMISSION_MODES.map((mode) => (
                                      <SelectItem key={mode} value={mode}>
                                        {mode}
                                      </SelectItem>
                                    ))}
                                  </SelectContent>
                                </Select>
                              </div>

                              <div className="flex items-start justify-between gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2">
                                <div className="space-y-1">
                                  <Label
                                    htmlFor="claude-dangerous-skip"
                                    className="text-xs text-[var(--text-primary)]"
                                  >
                                    Skip Permissions
                                  </Label>
                                  <p className="text-[0.6875rem] leading-relaxed text-[var(--text-muted)]">
                                    Actually bypasses Claude permission prompts
                                    for RalphX-launched runs.
                                  </p>
                                </div>
                                <Switch
                                  id="claude-dangerous-skip"
                                  checked={
                                    provider.claudeDangerouslySkipPermissions
                                  }
                                  disabled={isUpdating}
                                  onCheckedChange={(checked) =>
                                    void updateProvider(provider, {
                                      claudeDangerouslySkipPermissions: checked,
                                    })
                                  }
                                />
                              </div>
                            </div>
                          </>
                        )}
                      </div>

                      <div className="flex justify-end gap-2 border-t border-[var(--border-subtle)] px-4 py-3">
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          disabled={isUpdating}
                          onClick={() => void resetProviderDefaults(provider)}
                        >
                          <RotateCcw className="mr-2 h-4 w-4" />
                          Reset {providerLabel(provider.provider)}
                        </Button>
                        <Button
                          type="button"
                          variant="default"
                          size="sm"
                          disabled={isUpdating}
                          onClick={() => void applyProviderToAgents(provider)}
                        >
                          Apply to all agents
                        </Button>
                      </div>
                    </>
                  )}
                </div>
              );
            })}
          </div>
        </>
      )}
      <ConfirmationDialog {...confirmationDialogProps} />
    </SettingsSection>
  );
}

function ProviderPermissionDisclosure({
  provider,
  expanded,
  onToggle,
}: {
  provider: string;
  expanded: boolean;
  onToggle: () => void;
}) {
  return (
    <div className="md:col-span-2">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={expanded}
        aria-controls={`provider-permissions-${provider}`}
        className="inline-flex items-center gap-1 text-xs font-medium text-[var(--text-secondary)] transition-colors hover:text-[var(--accent-primary)]"
      >
        {expanded ? (
          <ChevronDown className="h-3.5 w-3.5" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5" />
        )}
        {expanded ? "Hide permissions" : "Show permissions"}
      </button>
    </div>
  );
}
