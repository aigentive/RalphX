import { useState } from "react";

import type { AgentProviderSettingsResponse } from "@/api/harness-providers";
import type { KnownHarness } from "@/api/ideation-harness";
import {
  workspaceReviewUtilityDefaultsForProvider,
  type WorkspaceReviewRuntimeSettingsResponse,
} from "@/api/workspace-review-settings";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useWorkspaceReviewRuntimeSettings } from "@/hooks/useWorkspaceReviewSettings";
import {
  agentEffortOptionsForModel,
  agentModelOptionsForProvider,
  type AgentModelRegistry,
  type AgentProvider,
} from "@/lib/agent-models";
import { ErrorBanner } from "../SettingsView.shared";
import { isKnownHarness } from "./workspaceReviewHarness";

const SELECT_TRIGGER_CLASS = "h-9 items-center";
const MODEL_DEFAULT_VALUE = "__default__";
const MODEL_CUSTOM_VALUE_PREFIX = "__custom__:";

const PROVIDER_LABELS: Record<KnownHarness, string> = {
  claude: "Claude",
  codex: "Codex",
};

interface ModelPreset {
  value: string;
  display: string;
  description?: string;
}

function isAgentProvider(value: string): value is AgentProvider {
  return value === "claude" || value === "codex";
}

function providerLabel(provider: KnownHarness): string {
  return PROVIDER_LABELS[provider] ?? provider;
}

function getModelPresets(
  provider: KnownHarness,
  modelRegistry: AgentModelRegistry,
  providerSupportedModelAliases?: readonly string[] | null,
): readonly ModelPreset[] {
  if (!isAgentProvider(provider)) {
    return [];
  }
  return agentModelOptionsForProvider(
    provider,
    modelRegistry,
    providerSupportedModelAliases,
  ).map(({ id, menuLabel, description }) => ({
    value: id,
    display: menuLabel,
    ...(description ? { description } : {}),
  }));
}

function modelSelectValue(
  value: string | null,
  presets: readonly ModelPreset[],
): string {
  if (!value) {
    return MODEL_DEFAULT_VALUE;
  }
  return presets.some((preset) => preset.value === value)
    ? value
    : `${MODEL_CUSTOM_VALUE_PREFIX}${value}`;
}

function effortLabel(value: string | null | undefined): string {
  if (!value || value === "inherit") return "Default";
  switch (value) {
    case "low":
      return "Low";
    case "medium":
      return "Medium";
    case "high":
      return "High";
    case "xhigh":
      return "Extra High";
    case "max":
      return "Max";
    default:
      return value;
  }
}

function selectValue(value: string | null | undefined): string {
  return value ?? "inherit";
}

function fromSelectValue(value: string): string | null {
  return value === "inherit" ? null : value;
}

function rowForProvider(
  rows: readonly WorkspaceReviewRuntimeSettingsResponse[],
  provider: KnownHarness,
): WorkspaceReviewRuntimeSettingsResponse | null {
  return rows.find((row) => row.provider === provider) ?? null;
}

function effectiveRuntime(
  provider: KnownHarness,
  row: WorkspaceReviewRuntimeSettingsResponse | null,
  globalRow: WorkspaceReviewRuntimeSettingsResponse | null,
) {
  const utility = workspaceReviewUtilityDefaultsForProvider(provider);
  return {
    model: row?.model ?? globalRow?.model ?? utility.model,
    effort: row?.effort ?? globalRow?.effort ?? utility.effort,
  };
}

function ModelSelect({
  provider,
  value,
  disabled,
  isGlobal,
  modelRegistry,
  providerSupportedModelAliases,
  onChange,
}: {
  provider: KnownHarness;
  value: string | null;
  disabled: boolean;
  isGlobal: boolean;
  modelRegistry: AgentModelRegistry;
  providerSupportedModelAliases?: readonly string[] | null;
  onChange: (value: string | null) => void;
}) {
  const presets = getModelPresets(
    provider,
    modelRegistry,
    providerSupportedModelAliases,
  );
  const currentValue = modelSelectValue(value, presets);
  const defaultLabel = isGlobal ? "Utility default" : "Use global default";
  const utility = workspaceReviewUtilityDefaultsForProvider(provider);
  const triggerLabel =
    value == null
      ? defaultLabel
      : presets.find((preset) => preset.value === value)?.display ?? value;
  const hasCustomValue =
    value != null && !presets.some((preset) => preset.value === value);

  return (
    <Select
      value={currentValue}
      onValueChange={(nextValue) => {
        const resolvedValue =
          nextValue === MODEL_DEFAULT_VALUE
            ? null
            : nextValue.startsWith(MODEL_CUSTOM_VALUE_PREFIX)
              ? nextValue.slice(MODEL_CUSTOM_VALUE_PREFIX.length)
              : nextValue;
        if (resolvedValue !== value) {
          onChange(resolvedValue);
        }
      }}
      disabled={disabled}
    >
      <SelectTrigger
        data-testid={`workspace-review-model-${provider}`}
        aria-label={`${providerLabel(provider)} Workspace Review model`}
        className={`${SELECT_TRIGGER_CLASS} bg-[var(--bg-surface)] border-[var(--border-default)]`}
      >
        <SelectValue placeholder="Select model">
          <span className="truncate">{triggerLabel}</span>
        </SelectValue>
      </SelectTrigger>
      <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
        <SelectItem value={MODEL_DEFAULT_VALUE} textValue={defaultLabel}>
          <div className="flex flex-col">
            <span className="text-[var(--text-primary)]">{defaultLabel}</span>
            <span className="text-xs text-[var(--text-muted)]">
              {isGlobal
                ? `Uses ${utility.model}.`
                : "Inherits the global default for this provider."}
            </span>
          </div>
        </SelectItem>
        {presets.map((preset) => (
          <SelectItem
            key={preset.value}
            value={preset.value}
            textValue={preset.display}
          >
            <div className="flex flex-col">
              <span className="text-[var(--text-primary)]">{preset.display}</span>
              {preset.description && (
                <span className="text-xs text-[var(--text-muted)]">
                  {preset.description}
                </span>
              )}
            </div>
          </SelectItem>
        ))}
        {hasCustomValue && value && (
          <SelectItem
            value={`${MODEL_CUSTOM_VALUE_PREFIX}${value}`}
            textValue={value}
          >
            <div className="flex flex-col">
              <span className="text-[var(--text-primary)]">Custom model</span>
              <span className="text-xs text-[var(--text-muted)]">{value}</span>
            </div>
          </SelectItem>
        )}
      </SelectContent>
    </Select>
  );
}

function WorkspaceReviewProviderRow({
  provider,
  row,
  globalRow,
  disabled,
  isGlobal,
  modelRegistry,
  onChange,
}: {
  provider: AgentProviderSettingsResponse;
  row: WorkspaceReviewRuntimeSettingsResponse | null;
  globalRow: WorkspaceReviewRuntimeSettingsResponse | null;
  disabled: boolean;
  isGlobal: boolean;
  modelRegistry: AgentModelRegistry;
  onChange: (patch: { model?: string | null; effort?: string | null }) => void;
}) {
  if (!isKnownHarness(provider.provider)) {
    return null;
  }
  const providerId = provider.provider;
  const effective = effectiveRuntime(providerId, row, isGlobal ? null : globalRow);
  const modelForEffort = row?.model ?? effective.model;
  const effortOptions = [
    {
      value: "inherit",
      label: "Default",
      description: isGlobal
        ? `Uses ${workspaceReviewUtilityDefaultsForProvider(providerId).effort}.`
        : "Inherits the global default for this provider.",
    },
    ...agentEffortOptionsForModel(
      providerId,
      modelForEffort,
      modelRegistry,
      provider.supportedEfforts ?? null,
    ).map(({ id, label, description }) => ({
      value: id,
      label,
      description,
    })),
  ];

  return (
    <div className="py-5">
      <div className="flex flex-col gap-1 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h3 className="text-[0.9375rem] font-semibold text-[var(--text-primary)]">
            {providerLabel(providerId)}
          </h3>
          <p className="text-xs text-[var(--text-muted)]">
            Effective: {effective.model} · {effortLabel(effective.effort)}
          </p>
        </div>
      </div>
      <div className="mt-3 grid gap-3 md:grid-cols-2">
        <div className="space-y-1">
          <p className="text-xs font-medium text-[var(--text-secondary)]">
            Model
          </p>
          <ModelSelect
            provider={providerId}
            value={row?.model ?? null}
            disabled={disabled}
            isGlobal={isGlobal}
            modelRegistry={modelRegistry}
            providerSupportedModelAliases={provider.supportedModelAliases ?? null}
            onChange={(model) => onChange({ model, effort: null })}
          />
        </div>
        <div className="space-y-1">
          <p className="text-xs font-medium text-[var(--text-secondary)]">
            Effort
          </p>
          <Select
            value={selectValue(row?.effort)}
            onValueChange={(value) => onChange({ effort: fromSelectValue(value) })}
            disabled={disabled}
          >
            <SelectTrigger
              data-testid={`workspace-review-effort-${providerId}`}
              aria-label={`${providerLabel(providerId)} Workspace Review effort`}
              className={`${SELECT_TRIGGER_CLASS} bg-[var(--bg-surface)] border-[var(--border-default)]`}
            >
              <SelectValue placeholder="Select effort">
                <span className="truncate">{effortLabel(row?.effort)}</span>
              </SelectValue>
            </SelectTrigger>
            <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
              {effortOptions.map((option) => (
                <SelectItem
                  key={option.value}
                  value={option.value}
                  textValue={option.label}
                >
                  <div className="flex flex-col">
                    <span className="text-[var(--text-primary)]">
                      {option.label}
                    </span>
                    <span className="text-xs text-[var(--text-muted)]">
                      {option.description}
                    </span>
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
    </div>
  );
}

export function WorkspaceReviewScopeRows({
  projectId,
  projectName,
  isGlobal,
  providers,
  modelRegistry,
  globalRows,
}: {
  projectId: string | null;
  projectName: string | null;
  isGlobal: boolean;
  providers: readonly AgentProviderSettingsResponse[];
  modelRegistry: AgentModelRegistry;
  globalRows: readonly WorkspaceReviewRuntimeSettingsResponse[];
}) {
  const [showError, setShowError] = useState(false);
  const { rows, isPlaceholderData, updateSettings, saveError } =
    useWorkspaceReviewRuntimeSettings(projectId);
  const disabled = (!isGlobal && projectId === null) || isPlaceholderData;

  const handleChange = (
    provider: KnownHarness,
    patch: { model?: string | null; effort?: string | null },
  ) => {
    if (disabled) {
      return;
    }
    const row = rowForProvider(rows, provider);
    setShowError(false);
    updateSettings(
      {
        provider,
        model: "model" in patch ? (patch.model ?? null) : (row?.model ?? null),
        effort: "effort" in patch ? (patch.effort ?? null) : (row?.effort ?? null),
      },
      { onError: () => setShowError(true) },
    );
  };

  return (
    <div>
      <p className="mb-3 text-xs text-[var(--text-muted)]">
        {isGlobal
          ? "Legacy fallback used only while Settings → Agents → Feedback Loops → Reviewer follows provider defaults."
          : projectId !== null
            ? `Legacy fallback overrides for ${projectName ?? "the active project"}. Leave blank to inherit the global fallback.`
            : "Select a project to override its legacy Workspace Review fallback."}
      </p>
      {showError && saveError && (
        <ErrorBanner
          error={saveError.message ?? "Failed to save Workspace Review settings"}
          onDismiss={() => setShowError(false)}
        />
      )}
      <div className={disabled ? "opacity-50 pointer-events-none" : undefined}>
        <div className="divide-y divide-[var(--border-default)]">
          {providers.map((provider) => {
            if (!isKnownHarness(provider.provider)) {
              return null;
            }
            const providerId = provider.provider;
            return (
              <WorkspaceReviewProviderRow
                key={providerId}
                provider={provider}
                row={rowForProvider(rows, providerId)}
                globalRow={rowForProvider(globalRows, providerId)}
                disabled={disabled}
                isGlobal={isGlobal}
                modelRegistry={modelRegistry}
                onChange={(patch) => handleChange(providerId, patch)}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}
