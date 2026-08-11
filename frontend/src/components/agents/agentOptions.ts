import type {
  AgentEffort,
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import {
  agentEffortOptionsForModel,
  agentModelOptionsForProvider,
  agentModelSupportsCodexUltra,
  defaultEffortForModel,
  defaultModelForProvider,
  normalizeAgentRuntimeForPersistence,
  normalizeAgentRuntimeSelection,
  type AgentModelRegistry,
} from "@/lib/agent-models";

export interface AgentModelOption {
  id: string;
  label: string;
  description?: string;
}

export interface AgentEffortOption {
  id: AgentEffort;
  label: string;
  description?: string;
}

export const AGENT_PROVIDER_OPTIONS: Array<{ id: AgentProvider; label: string }> = [
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
];

export const DEFAULT_AGENT_RUNTIME: AgentRuntimeSelection =
  normalizeAgentRuntimeSelection(null);

export {
  agentModelSupportsCodexUltra,
  defaultEffortForModel,
  defaultModelForProvider,
};

export function normalizeRuntimeSelection(
  runtime: unknown,
  registry?: AgentModelRegistry,
  providerSupportedEfforts?: readonly unknown[] | null,
  providerSupportedModelAliases?: readonly unknown[] | null
): AgentRuntimeSelection {
  return normalizeAgentRuntimeSelection(
    runtime,
    registry,
    providerSupportedEfforts,
    providerSupportedModelAliases
  );
}

export function normalizeRuntimeForPersistence(
  runtime: unknown,
  registry?: AgentModelRegistry
): AgentRuntimeSelection {
  return normalizeAgentRuntimeForPersistence(runtime, registry);
}

export function agentModelOptions(
  provider: AgentProvider,
  registry?: AgentModelRegistry,
  providerSupportedModelAliases?: readonly unknown[] | null
): AgentModelOption[] {
  return agentModelOptionsForProvider(
    provider,
    registry,
    providerSupportedModelAliases
  ).map(
    ({ id, menuLabel, description }) => ({
      id,
      label: menuLabel,
      ...(description ? { description } : {}),
    })
  );
}

export function agentEffortOptions(
  provider: AgentProvider,
  modelId: string,
  registry?: AgentModelRegistry,
  providerSupportedEfforts?: readonly unknown[] | null
): AgentEffortOption[] {
  return agentEffortOptionsForModel(
    provider,
    modelId,
    registry,
    providerSupportedEfforts
  ).map(({ id, label, description }) => ({
    id,
    label,
    ...(description ? { description } : {}),
  }));
}

export { agentEffortOptionsForModel };
