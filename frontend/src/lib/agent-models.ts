export type AgentProvider = "claude" | "codex";
export type AgentEffort = "low" | "medium" | "high" | "xhigh" | "max";
export type AgentModelSource = "built_in" | "custom";

export interface AgentRuntimeSelection {
  provider: AgentProvider;
  modelId: string;
  effort: AgentEffort;
}

export interface AgentEffortCatalogEntry {
  id: AgentEffort;
  label: string;
  description: string;
}

export interface AgentModelCatalogEntry {
  id: string;
  label: string;
  menuLabel: string;
  defaultEffort: AgentEffort;
  supportedEfforts: readonly AgentEffort[];
  description?: string;
  source?: AgentModelSource;
  enabled?: boolean;
}

export type AgentModelRegistry = Record<
  AgentProvider,
  readonly AgentModelCatalogEntry[]
>;

export interface AgentModelRegistryModel {
  provider: string;
  modelId: string;
  label: string;
  menuLabel: string;
  defaultEffort: string;
  supportedEfforts: readonly string[];
  source?: string | undefined;
  enabled?: boolean;
  description?: string | null | undefined;
}

export const AGENT_EFFORT_CATALOG = [
  {
    id: "low",
    label: "Low",
    description: "Fastest responses with lighter reasoning.",
  },
  {
    id: "medium",
    label: "Medium",
    description: "Balanced reasoning depth for everyday tasks.",
  },
  {
    id: "high",
    label: "High",
    description: "Greater reasoning depth for complex work.",
  },
  {
    id: "xhigh",
    label: "Extra High",
    description: "Extended reasoning depth for long-horizon work.",
  },
  {
    id: "max",
    label: "Max",
    description: "Deepest reasoning with the highest token spend.",
  },
] as const satisfies readonly AgentEffortCatalogEntry[];

const CLAUDE_MODEL_CATALOG = [
  {
    id: "sonnet",
    label: "sonnet",
    menuLabel: "sonnet",
    defaultEffort: "medium",
    supportedEfforts: ["low", "medium", "high", "max"],
    description: "Claude Sonnet model alias.",
  },
  {
    id: "opus",
    label: "opus",
    menuLabel: "opus",
    defaultEffort: "xhigh",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    description: "Claude Opus model alias.",
  },
  {
    id: "haiku",
    label: "haiku",
    menuLabel: "haiku",
    defaultEffort: "medium",
    supportedEfforts: ["low", "medium", "high"],
    description: "Claude Haiku model alias.",
  },
] as const satisfies readonly AgentModelCatalogEntry[];

export const CODEX_MODEL_CATALOG = [
  {
    id: "gpt-5.5",
    label: "gpt-5.5 - Frontier model for complex coding, research, and real-world work.",
    menuLabel: "gpt-5.5 (Current)",
    defaultEffort: "xhigh",
    supportedEfforts: ["low", "medium", "high", "xhigh"],
    description: "Frontier model for complex coding, research, and real-world work.",
  },
  {
    id: "gpt-5.4",
    label: "gpt-5.4 - Strong model for everyday coding.",
    menuLabel: "gpt-5.4",
    defaultEffort: "xhigh",
    supportedEfforts: ["low", "medium", "high", "xhigh"],
    description: "Strong model for everyday coding.",
  },
  {
    id: "gpt-5.4-mini",
    label: "gpt-5.4-mini - Small, fast, and cost-efficient model for simpler coding tasks.",
    menuLabel: "gpt-5.4-mini",
    defaultEffort: "medium",
    supportedEfforts: ["low", "medium", "high"],
    description: "Small, fast, and cost-efficient model for simpler coding tasks.",
  },
  {
    id: "gpt-5.3-codex",
    label: "gpt-5.3-codex - Coding-optimized model.",
    menuLabel: "gpt-5.3-codex",
    defaultEffort: "high",
    supportedEfforts: ["low", "medium", "high"],
    description: "Coding-optimized model.",
  },
  {
    id: "gpt-5.3-codex-spark",
    label: "gpt-5.3-codex-spark - Ultra-fast coding model.",
    menuLabel: "gpt-5.3-codex-spark",
    defaultEffort: "medium",
    supportedEfforts: ["low", "medium"],
    description: "Ultra-fast coding model.",
  },
] as const satisfies readonly AgentModelCatalogEntry[];

export const AGENT_MODEL_CATALOG: AgentModelRegistry = {
  claude: CLAUDE_MODEL_CATALOG,
  codex: CODEX_MODEL_CATALOG,
};

export const DEFAULT_CODEX_MODEL_ID = CODEX_MODEL_CATALOG[0].id;

function isAgentProvider(value: unknown): value is AgentProvider {
  return value === "claude" || value === "codex";
}

function isAgentEffort(value: unknown): value is AgentEffort {
  return AGENT_EFFORT_CATALOG.some((effort) => effort.id === value);
}

function isAgentModelSource(value: unknown): value is AgentModelSource {
  return value === "built_in" || value === "custom";
}

function effortOrder(effort: AgentEffort): number {
  return AGENT_EFFORT_CATALOG.findIndex((entry) => entry.id === effort);
}

function normalizeSupportedEfforts(values: readonly unknown[]): AgentEffort[] {
  const efforts = values.filter(isAgentEffort);
  return [...new Set(efforts)].sort((a, b) => effortOrder(a) - effortOrder(b));
}

function defaultModelEntryForProvider(
  provider: AgentProvider,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG
): AgentModelCatalogEntry {
  return registry[provider][0] ?? AGENT_MODEL_CATALOG[provider][0] ?? CODEX_MODEL_CATALOG[0];
}

function findModelEntryForProvider(
  provider: AgentProvider,
  modelId: unknown,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG
): AgentModelCatalogEntry | null {
  if (typeof modelId !== "string") {
    return null;
  }
  const normalizedModelId = modelId.trim();
  if (!normalizedModelId) {
    return null;
  }
  return registry[provider].find((model) => model.id === normalizedModelId) ?? null;
}

function providerDefaultEffort(
  provider: AgentProvider,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG
): AgentEffort {
  return defaultModelEntryForProvider(provider, registry).defaultEffort;
}

export function buildAgentModelRegistry(
  models: readonly AgentModelRegistryModel[]
): AgentModelRegistry {
  const registry: Record<AgentProvider, AgentModelCatalogEntry[]> = {
    claude: [],
    codex: [],
  };

  for (const model of models) {
    if (!isAgentProvider(model.provider) || model.enabled === false) {
      continue;
    }
    const modelId = model.modelId.trim();
    if (!modelId) {
      continue;
    }

    const supportedEfforts = normalizeSupportedEfforts(model.supportedEfforts);
    const fallbackEffort = supportedEfforts[0];
    if (!fallbackEffort) {
      continue;
    }
    const defaultEffort = isAgentEffort(model.defaultEffort)
      ? model.defaultEffort
      : fallbackEffort;

    registry[model.provider].push({
      id: modelId,
      label: model.label.trim() || modelId,
      menuLabel: model.menuLabel.trim() || model.label.trim() || modelId,
      defaultEffort: supportedEfforts.includes(defaultEffort)
        ? defaultEffort
        : fallbackEffort,
      supportedEfforts,
      ...(model.description ? { description: model.description } : {}),
      ...(isAgentModelSource(model.source) ? { source: model.source } : {}),
      enabled: true,
    });
  }

  return {
    claude: registry.claude.length > 0 ? registry.claude : AGENT_MODEL_CATALOG.claude,
    codex: registry.codex.length > 0 ? registry.codex : AGENT_MODEL_CATALOG.codex,
  };
}

export function defaultModelForProvider(
  provider: AgentProvider,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG
): string {
  return defaultModelEntryForProvider(provider, registry).id;
}

export function defaultEffortForModel(
  provider: AgentProvider,
  modelId: string,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG
): AgentEffort {
  return (
    findModelEntryForProvider(provider, modelId, registry)?.defaultEffort ??
    providerDefaultEffort(provider, registry)
  );
}

export function agentModelOptionsForProvider(
  provider: AgentProvider,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG
): readonly AgentModelCatalogEntry[] {
  return registry[provider];
}

export function agentEffortOptionsForModel(
  provider: AgentProvider,
  modelId: string,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG
): AgentEffortCatalogEntry[] {
  const supportedEfforts =
    findModelEntryForProvider(provider, modelId, registry)?.supportedEfforts ??
    defaultEffortsForProvider(provider);
  return AGENT_EFFORT_CATALOG.filter((effort) =>
    supportedEfforts.includes(effort.id)
  );
}

function defaultEffortsForProvider(provider: AgentProvider): readonly AgentEffort[] {
  return provider === "codex"
    ? ["low", "medium", "high", "xhigh"]
    : ["low", "medium", "high"];
}

export function normalizeAgentRuntimeSelection(
  runtime: unknown,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG
): AgentRuntimeSelection {
  const defaultEntry = defaultModelEntryForProvider("codex", registry);
  const defaultRuntime: AgentRuntimeSelection = {
    provider: "codex",
    modelId: defaultEntry.id,
    effort: defaultEntry.defaultEffort,
  };

  if (!runtime || typeof runtime !== "object") {
    return defaultRuntime;
  }

  const candidate = runtime as Partial<Record<keyof AgentRuntimeSelection, unknown>>;
  if (!isAgentProvider(candidate.provider)) {
    return defaultRuntime;
  }

  const provider = candidate.provider;
  const requestedModelId =
    typeof candidate.modelId === "string" ? candidate.modelId.trim() : "";
  const knownModel = findModelEntryForProvider(provider, requestedModelId, registry);
  if (!knownModel && requestedModelId) {
    const effort = isAgentEffort(candidate.effort)
      ? candidate.effort
      : providerDefaultEffort(provider, registry);
    return {
      provider,
      modelId: requestedModelId,
      effort,
    };
  }

  const model = knownModel ?? defaultModelEntryForProvider(provider, registry);
  const effort =
    isAgentEffort(candidate.effort) &&
    model.supportedEfforts.includes(candidate.effort)
      ? candidate.effort
      : model.defaultEffort;

  return {
    provider,
    modelId: model.id,
    effort,
  };
}
