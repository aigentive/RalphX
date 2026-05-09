import { useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  Cpu,
  ExternalLink,
  RefreshCw,
  RotateCcw,
  ShieldCheck,
} from "lucide-react";

import type { AgentProviderSettingsResponse } from "@/api/harness-providers";
import type { UpdateAgentProviderSettingsInput } from "@/api/harness-providers";
import { Button } from "@/components/ui/button";
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
import {
  AGENT_EFFORT_CATALOG,
  agentEffortOptionsForModel,
  defaultModelForProvider,
  type AgentProvider,
} from "@/lib/agent-models";

import { ErrorBanner, SectionCard } from "./SettingsView.shared";

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
  } = useHarnessProviders();
  const { models } = useAgentModels();
  const { confirm, confirmationDialogProps, ConfirmationDialog } =
    useConfirmation();
  const [expandedPermissions, setExpandedPermissions] = useState<
    Record<string, boolean>
  >({});

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
  ) => {
    const input: UpdateAgentProviderSettingsInput = {
      provider: provider.provider,
    };
    if (changes.enabled !== undefined) input.enabled = changes.enabled;
    if (changes.isDefault !== undefined) input.isDefault = changes.isDefault;
    if (changes.model !== undefined) input.model = changes.model;
    if (changes.effort !== undefined) input.effort = changes.effort;
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
    if (changes.resetToDefaults !== undefined) {
      input.resetToDefaults = changes.resetToDefaults;
    }
    if (changes.applyToAllLanes !== undefined) {
      input.applyToAllLanes = changes.applyToAllLanes;
    }
    await updateProviderAsync(input);
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

  return (
    <SectionCard
      icon={<ShieldCheck className="h-5 w-5" />}
      title="Providers"
      description="Validate CLI harnesses, enable agent providers, and choose the default used by new agent lanes."
    >
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
                (model) => model.provider === provider.provider && model.enabled,
              );
              const selectedModel =
                provider.model ?? PROVIDER_DEFAULT_SELECT_VALUE;
              const selectedModelEntry = providerModels.find(
                (model) => model.modelId === selectedModel,
              );
              const selectedModelId =
                provider.model ?? defaultModelForProvider(agentProvider);
              const effortOptions = agentEffortOptionsForModel(
                agentProvider,
                selectedModelId,
              );
              const selectedEffort =
                provider.effort ?? PROVIDER_DEFAULT_SELECT_VALUE;
              const hasCustomModel =
                provider.model != null &&
                provider.model.trim() !== "" &&
                selectedModelEntry == null;

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
    </SectionCard>
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
