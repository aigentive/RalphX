import { useMemo, useRef } from "react";

import type {
  ManualRoleCatalogEntry,
  ManualRoleRuntimeSelection,
  ManualServiceTier,
} from "@/api/manual-role-defaults.types";
import type { CoordinationMode } from "@/types/chat-conversation";
import type { AgentModelCatalogEntry } from "@/lib/agent-models";
import type { Persona } from "@/types/persona";
import type { AgentProvider } from "@/stores/agentSessionStore";

import { ComposerRuntimeSelector } from "./ComposerRuntimeSelector";

const DEFAULT_VALUE = "__provider_default__";
const NO_PERSONA = "__no_persona__";

function providerLabel(provider: string) {
  if (provider === "codex") return "Codex";
  if (provider === "claude") return "Claude";
  return provider;
}

function capabilityLabel(value: string) {
  const labels: Record<string, string> = {
    solo: "Defaults",
    rx_native_team: "Team",
    rx_native_workflow: "Workflow",
    codex_native_ultra: "Codex Ultra",
  };
  return labels[value] ?? value;
}

export function ManualRoleRuntimeSelector({
  entry,
  value,
  providers,
  modelsForProvider,
  personas,
  disabled = false,
  runtimeDefault,
  onChange,
  onManagePersonas,
}: {
  entry: ManualRoleCatalogEntry;
  value: ManualRoleRuntimeSelection;
  providers: readonly string[];
  modelsForProvider: (provider: string) => readonly AgentModelCatalogEntry[];
  personas: readonly Persona[];
  disabled?: boolean;
  runtimeDefault?: {
    source?: string | null;
    isResetting?: boolean;
    disabled?: boolean;
    onReset: () => Promise<unknown> | void;
  };
  onChange: (value: ManualRoleRuntimeSelection) => void;
  onManagePersonas?: () => void;
}) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const models = modelsForProvider(value.provider);
  const selectedModel = models.find((model) => model.id === value.model);
  const providerOptions = useMemo(
    () =>
      (providers.includes(value.provider)
        ? providers
        : [value.provider, ...providers]
      ).map((provider) => ({
        id: provider as AgentProvider,
        label: providerLabel(provider),
      })),
    [providers, value.provider],
  );
  const modelOptions = [
    { id: DEFAULT_VALUE, label: "Provider default" },
    ...models.map((model) => ({ id: model.id, label: model.menuLabel })),
  ];
  if (value.model && !selectedModel) {
    modelOptions.push({ id: value.model, label: value.model });
  }
  const efforts = selectedModel?.supportedEfforts ?? [];
  const effortOptions = [
    { id: DEFAULT_VALUE, label: "Provider default" },
    ...efforts.map((effort) => ({ id: effort, label: effort })),
  ];
  if (value.effort && !efforts.some((effort) => effort === value.effort)) {
    effortOptions.push({ id: value.effort, label: value.effort });
  }
  const capabilityOptions = entry.controls.capabilities.map((option) => ({
    id: option.value,
    label: capabilityLabel(option.value),
    disabled: !option.enabled,
    ...(option.disabledReason ? { disabledReason: option.disabledReason } : {}),
  }));
  const speedOptions = entry.controls.speeds.map((option) => ({
    id: option.value,
    label:
      option.value === "provider_default"
        ? "Provider default"
        : option.value === "fast"
          ? "Fast"
          : "Standard",
    disabled: !option.enabled,
    ...(option.disabledReason ? { disabledReason: option.disabledReason } : {}),
  }));

  const commit = (patch: Partial<ManualRoleRuntimeSelection>) =>
    onChange({ ...value, ...patch });

  return (
    <div ref={surfaceRef} className="min-w-0">
      <ComposerRuntimeSelector
        surfaceRef={surfaceRef}
        provider={{
          value: value.provider as AgentProvider,
          options: providerOptions,
          disabled,
          onValueChange: (provider) => {
            const firstModel = modelsForProvider(provider)[0];
            commit({
              provider,
              model: firstModel?.id ?? null,
              effort: firstModel?.defaultEffort ?? null,
            });
          },
        }}
        model={{
          value: value.model ?? DEFAULT_VALUE,
          options: modelOptions,
          disabled,
          onValueChange: (model) => {
            const nextModel = model === DEFAULT_VALUE ? null : model;
            const selected = models.find((candidate) => candidate.id === nextModel);
            commit({ model: nextModel, effort: selected?.defaultEffort ?? null });
          },
        }}
        effort={{
          value: value.effort ?? DEFAULT_VALUE,
          options: effortOptions,
          disabled,
          onValueChange: (effort) =>
            commit({ effort: effort === DEFAULT_VALUE ? null : effort }),
        }}
        {...(capabilityOptions.length > 0
          ? {
              capability: {
                value: (value.coordinationMode ?? "solo") as CoordinationMode,
                options: capabilityOptions,
                disabled,
                onValueChange: (coordinationMode) => commit({ coordinationMode }),
              },
            }
          : {})}
        {...(entry.controls.persona.enabled
          ? {
              persona: {
                value: value.personaId ?? NO_PERSONA,
                options: [
                  { id: NO_PERSONA, label: "No role persona" },
                  ...personas.map((persona) => ({ id: persona.id, label: persona.name })),
                ],
                disabled,
                onValueChange: (personaId) =>
                  commit({ personaId: personaId === NO_PERSONA ? null : personaId }),
                ...(onManagePersonas
                  ? {
                      footerAction: (
                        <button
                          type="button"
                          className="px-2 py-1 text-xs text-[var(--accent-primary)]"
                          onClick={onManagePersonas}
                        >
                          Manage personas
                        </button>
                      ),
                    }
                  : {}),
              },
            }
          : {})}
        speed={{
          value: value.serviceTier,
          options: speedOptions,
          disabled,
          onValueChange: (serviceTier) =>
            commit({ serviceTier: serviceTier as ManualServiceTier }),
        }}
        {...(runtimeDefault ? { runtimeDefault } : {})}
      />
    </div>
  );
}
