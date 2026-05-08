import { Cpu, ExternalLink, RefreshCw, ShieldCheck } from "lucide-react";

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
import { AGENT_EFFORT_CATALOG, type AgentEffort } from "@/lib/agent-models";

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

function providerLabel(provider: string): string {
  return PROVIDER_LABELS[provider] ?? provider;
}

function effortLabel(effort: string): string {
  return AGENT_EFFORT_CATALOG.find((entry) => entry.id === effort)?.label ?? effort;
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

export function HarnessProvidersSection() {
  const {
    settings,
    providers,
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

  const displayedError =
    (isError && error instanceof Error ? error.message : null) ??
    (updateError instanceof Error ? updateError.message : null);

  const updateProvider = async (
    provider: AgentProviderSettingsResponse,
    changes: Partial<AgentProviderSettingsResponse> & {
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
    if (changes.applyToAllLanes !== undefined) {
      input.applyToAllLanes = changes.applyToAllLanes;
    }
    await updateProviderAsync(input);
  };

  const applyAsDefault = async (provider: AgentProviderSettingsResponse) => {
    const applyToAllLanes = await confirm({
      title: `Use ${providerLabel(provider.provider)} by default?`,
      description:
        "Apply this provider to all agent lanes now, or only make it the default for future resets and new lanes.",
      confirmText: "Apply to all lanes",
      cancelText: "Default only",
    });
    await updateProvider(provider, {
      isDefault: true,
      applyToAllLanes,
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

      <div className="flex items-center justify-between gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2">
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
          const providerModels = models.filter(
            (model) => model.provider === provider.provider && model.enabled,
          );
          const selectedModel =
            provider.model ?? providerModels[0]?.modelId ?? "";
          const selectedEffort =
            (provider.effort as AgentEffort | null | undefined) ??
            providerModels.find((model) => model.modelId === selectedModel)
              ?.defaultEffort ??
            "medium";

          return (
            <div
              key={provider.provider}
              className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]"
            >
              <div className="flex flex-wrap items-start justify-between gap-3 border-b border-[var(--border-subtle)] px-4 py-3">
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
                  <p className="text-xs text-[var(--text-muted)]">
                    {provider.status}
                  </p>
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

              <div className="grid gap-3 px-4 py-4 md:grid-cols-2">
                <div className="space-y-1">
                  <Label htmlFor={`provider-model-${provider.provider}`}>
                    Default Model
                  </Label>
                  <Select
                    value={selectedModel}
                    onValueChange={(model) =>
                      void updateProvider(provider, { model })
                    }
                    disabled={isUpdating || providerModels.length === 0}
                  >
                    <SelectTrigger id={`provider-model-${provider.provider}`}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {providerModels.map((model) => (
                        <SelectItem key={model.modelId} value={model.modelId}>
                          {model.menuLabel}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                <div className="space-y-1">
                  <Label htmlFor={`provider-effort-${provider.provider}`}>
                    Default Effort
                  </Label>
                  <Select
                    value={selectedEffort}
                    onValueChange={(effort) =>
                      void updateProvider(provider, { effort })
                    }
                    disabled={isUpdating}
                  >
                    <SelectTrigger id={`provider-effort-${provider.provider}`}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {AGENT_EFFORT_CATALOG.map((effort) => (
                        <SelectItem key={effort.id} value={effort.id}>
                          {effortLabel(effort.id)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                {provider.provider === "codex" && (
                  <>
                    <div className="space-y-1">
                      <Label htmlFor="codex-approval-policy">
                        Approval Policy
                      </Label>
                      <Select
                        value={provider.approvalPolicy ?? "never"}
                        onValueChange={(approvalPolicy) =>
                          void updateProvider(provider, { approvalPolicy })
                        }
                        disabled={isUpdating}
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
                      <Label htmlFor="codex-sandbox-mode">Sandbox Mode</Label>
                      <Select
                        value={provider.sandboxMode ?? "danger-full-access"}
                        onValueChange={(sandboxMode) =>
                          void updateProvider(provider, { sandboxMode })
                        }
                        disabled={isUpdating}
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
                  </>
                )}

                {provider.provider === "claude" && (
                  <>
                    <div className="space-y-1">
                      <Label htmlFor="claude-permission-mode">
                        Permission Mode
                      </Label>
                      <Select
                        value={
                          provider.claudePermissionMode ?? "bypassPermissions"
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

                    <div className="flex items-center justify-between rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2">
                      <Label
                        htmlFor="claude-dangerous-skip"
                        className="text-xs text-[var(--text-primary)]"
                      >
                        Skip Permissions
                      </Label>
                      <Switch
                        id="claude-dangerous-skip"
                        checked={provider.claudeDangerouslySkipPermissions}
                        disabled={isUpdating}
                        onCheckedChange={(checked) =>
                          void updateProvider(provider, {
                            claudeDangerouslySkipPermissions: checked,
                          })
                        }
                      />
                    </div>

                    <div className="flex items-center justify-between rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2">
                      <Label
                        htmlFor="claude-allow-dangerous-skip"
                        className="text-xs text-[var(--text-primary)]"
                      >
                        Allow Skip Option
                      </Label>
                      <Switch
                        id="claude-allow-dangerous-skip"
                        checked={provider.claudeAllowDangerouslySkipPermissions}
                        disabled={isUpdating}
                        onCheckedChange={(checked) =>
                          void updateProvider(provider, {
                            claudeAllowDangerouslySkipPermissions: checked,
                          })
                        }
                      />
                    </div>
                  </>
                )}
              </div>

              <div className="flex justify-end border-t border-[var(--border-subtle)] px-4 py-3">
                <Button
                  type="button"
                  variant={provider.isDefault ? "secondary" : "default"}
                  size="sm"
                  disabled={isUpdating || !provider.enabled || provider.isDefault}
                  onClick={() => void applyAsDefault(provider)}
                >
                  Apply as Default
                </Button>
              </div>
            </div>
          );
        })}
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
    </SectionCard>
  );
}
