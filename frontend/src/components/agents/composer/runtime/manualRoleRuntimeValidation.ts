import type {
  ManualRoleCatalogEntry,
  ManualRoleRuntimeSelection,
} from "@/api/manual-role-defaults.types";
import type { AgentModelCatalogEntry } from "@/lib/agent-models";
import type { Persona } from "@/types/persona";

import type { AgentProviderAvailabilityOption } from "../../agentProviderAvailability";

export function getManualRoleRuntimeSelectionIssue({
  entry,
  value,
  providerOptions,
  modelsForProvider,
  personas,
}: {
  entry: ManualRoleCatalogEntry;
  value: ManualRoleRuntimeSelection;
  providerOptions: readonly AgentProviderAvailabilityOption[];
  modelsForProvider: (provider: string) => readonly AgentModelCatalogEntry[];
  personas: readonly Persona[];
}): string | null {
  const provider = providerOptions.find((option) => option.id === value.provider);
  if (!provider) return "The selected provider is no longer available.";
  if (provider.disabled) {
    return provider.disabledReason ?? "The selected provider is unavailable.";
  }
  const models = modelsForProvider(value.provider);
  const model = value.model
    ? models.find((candidate) => candidate.id === value.model)
    : null;
  if (value.model && !model) return "The selected model is no longer available.";
  if (
    value.effort &&
    (!model || !model.supportedEfforts.some((effort) => effort === value.effort))
  ) {
    return "The selected effort is not supported by this model.";
  }
  const speed = entry.controls.speeds.find(
    (option) => option.value === value.serviceTier,
  );
  if (!speed?.enabled) {
    return speed?.disabledReason ?? "The selected speed is unavailable.";
  }
  const coordinationMode = value.coordinationMode ?? "solo";
  const capability = entry.controls.capabilities.find(
    (option) => option.value === coordinationMode,
  );
  if (entry.controls.capabilities.length > 0 && !capability?.enabled) {
    return capability?.disabledReason ?? "The selected capability is unavailable.";
  }
  if (
    value.personaId &&
    (!entry.controls.persona.enabled ||
      !personas.some((persona) => persona.id === value.personaId))
  ) {
    return "The selected persona is no longer available.";
  }
  return null;
}
