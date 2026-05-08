import type {
  AgentEffort,
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import {
  AGENT_EFFORT_CATALOG,
  agentEffortOptionsForModel,
  agentModelOptionsForProvider,
  defaultEffortForModel,
  defaultModelForProvider,
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

export const AGENT_EFFORT_OPTIONS: AgentEffortOption[] = AGENT_EFFORT_CATALOG.map(
  ({ id, label, description }) => ({
    id,
    label,
    description,
  })
);

export const DEFAULT_AGENT_RUNTIME: AgentRuntimeSelection =
  normalizeAgentRuntimeSelection(null);

export { defaultEffortForModel, defaultModelForProvider };

export function normalizeRuntimeSelection(
  runtime: unknown,
  registry?: AgentModelRegistry
): AgentRuntimeSelection {
  return normalizeAgentRuntimeSelection(runtime, registry);
}

export function agentModelOptions(
  provider: AgentProvider,
  registry?: AgentModelRegistry
): AgentModelOption[] {
  return agentModelOptionsForProvider(provider, registry).map(
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
  registry?: AgentModelRegistry
): AgentEffortOption[] {
  return agentEffortOptionsForModel(provider, modelId, registry).map(
    ({ id, label, description }) => ({
      id,
      label,
      description,
    })
  );
}

export { agentEffortOptionsForModel };
