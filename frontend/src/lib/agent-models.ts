export type AgentProvider = "claude" | "codex";
export type AgentEffort = "low" | "medium" | "high" | "xhigh" | "max" | "ultra";
export type AgentModelSource = "built_in" | "custom";

export interface AgentRuntimeSelection {
  provider: AgentProvider;
  modelId: string;
  effort: AgentEffort;
}

export interface AgentEffortCatalogEntry {
  id: AgentEffort;
  label: string;
  description?: string;
}

export interface AgentModelCatalogEntry {
  id: string;
  label: string;
  menuLabel: string;
  defaultEffort: AgentEffort;
  supportedEfforts: readonly AgentEffort[];
  supportsCodexUltra?: boolean;
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
  supportsCodexUltra?: boolean | undefined;
  source?: string | undefined;
  enabled?: boolean;
  description?: string | null | undefined;
}

export const AGENT_EFFORT_CATALOG = [
  {
    id: "low",
    label: "Low",
  },
  {
    id: "medium",
    label: "Medium",
  },
  {
    id: "high",
    label: "High",
  },
  {
    id: "xhigh",
    label: "Extra High",
  },
  {
    id: "max",
    label: "Max",
  },
  {
    id: "ultra",
    label: "Ultra",
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
    id: "claude-sonnet-4-6",
    label: "Claude Sonnet 4.6",
    menuLabel: "Claude Sonnet 4.6",
    defaultEffort: "high",
    supportedEfforts: ["low", "medium", "high", "max"],
    description: "Exact Claude Sonnet 4.6 model id.",
  },
  {
    id: "claude-sonnet-5",
    label: "Claude Sonnet 5",
    menuLabel: "Claude Sonnet 5",
    defaultEffort: "high",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    description: "Exact Claude Sonnet 5 model id; requires Claude Code 2.1.197 or newer.",
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
    id: "claude-opus-4-7",
    label: "Claude Opus 4.7",
    menuLabel: "Claude Opus 4.7",
    defaultEffort: "high",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    description: "Exact Claude Opus 4.7 model id; requires Claude Code 2.1.111 or newer.",
  },
  {
    id: "claude-opus-4-8",
    label: "Claude Opus 4.8",
    menuLabel: "Claude Opus 4.8",
    defaultEffort: "high",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    description: "Exact Claude Opus 4.8 model id; requires Claude Code 2.1.154 or newer.",
  },
  {
    id: "claude-opus-5",
    label: "Claude Opus 5",
    menuLabel: "Claude Opus 5",
    defaultEffort: "high",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    description: "Exact Claude Opus 5 model id; requires Claude Code 2.1.219 or newer.",
  },
  {
    id: "haiku",
    label: "haiku",
    menuLabel: "haiku",
    defaultEffort: "medium",
    supportedEfforts: ["low", "medium", "high"],
    description: "Claude Haiku model alias.",
  },
  {
    id: "fable",
    label: "fable",
    menuLabel: "fable",
    defaultEffort: "high",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    description: "Claude Fable 5 model alias.",
  },
] as const satisfies readonly AgentModelCatalogEntry[];

export const CODEX_MODEL_CATALOG = [
  {
    id: "gpt-5.6-sol",
    label:
      "gpt-5.6-sol - Flagship GPT-5.6 model for complex coding, research, and agentic work.",
    menuLabel: "gpt-5.6-sol",
    defaultEffort: "medium",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    supportsCodexUltra: true,
    description:
      "Flagship GPT-5.6 model for complex coding, research, and agentic work.",
  },
  {
    id: "gpt-5.6-terra",
    label:
      "gpt-5.6-terra - High-intelligence GPT-5.6 model for substantial coding and research tasks.",
    menuLabel: "gpt-5.6-terra",
    defaultEffort: "medium",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    supportsCodexUltra: true,
    description:
      "High-intelligence GPT-5.6 model for substantial coding and research tasks.",
  },
  {
    id: "gpt-5.6-luna",
    label:
      "gpt-5.6-luna - Efficient GPT-5.6 model for capable everyday coding work.",
    menuLabel: "gpt-5.6-luna",
    defaultEffort: "medium",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    description: "Efficient GPT-5.6 model for capable everyday coding work.",
  },
  {
    id: "gpt-5.5",
    label: "gpt-5.5 - Frontier model for complex coding, research, and real-world work.",
    menuLabel: "gpt-5.5",
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

const DEFAULT_MODEL_BY_PROVIDER = {
  claude: "sonnet",
  codex: "gpt-5.5",
} as const satisfies Record<AgentProvider, string>;

const SUPPORTED_DEFAULT_MODEL_BY_PROVIDER: Partial<Record<AgentProvider, string>> = {
  codex: "gpt-5.6-terra",
};

export const DEFAULT_CODEX_MODEL_ID = DEFAULT_MODEL_BY_PROVIDER.codex;

const CODEX_EFFORT_DESCRIPTIONS = {
  low: "Fast responses with lighter reasoning.",
  medium: "Balances speed and reasoning depth for everyday tasks.",
  high: "Greater reasoning depth for complex problems.",
  xhigh: "Extra high reasoning depth for complex problems.",
  max: "Maximum reasoning depth for the hardest problems.",
  ultra: "Maximum reasoning with automatic task delegation.",
} as const satisfies Record<AgentEffort, string>;

const CLAUDE_EFFORT_DESCRIPTIONS = {
  low: "For short, scoped tasks and latency-sensitive work that is not intelligence-sensitive.",
  medium: "For cost-sensitive work that reduces token usage while trading off intelligence.",
  high: "Balances token usage and intelligence; use at least high for intelligence-sensitive work.",
  xhigh: "Best setting for most coding and agentic use cases.",
  max: "For intelligence-demanding tasks that justify higher usage and possible overthinking.",
  ultra: "Not a distinct Claude effort level in the current Claude catalog.",
} as const satisfies Record<AgentEffort, string>;

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

function normalizeOrdinarySupportedEfforts(
  values: readonly unknown[]
): AgentEffort[] {
  return normalizeSupportedEfforts(values).filter((effort) => effort !== "ultra");
}

function intersectSupportedEfforts(
  modelEfforts: readonly AgentEffort[],
  providerEfforts?: readonly unknown[] | null
): AgentEffort[] {
  const providerSupportedEfforts =
    providerEfforts != null ? normalizeSupportedEfforts(providerEfforts) : [];
  if (providerSupportedEfforts.length === 0) {
    return [...modelEfforts];
  }
  return modelEfforts.filter((effort) => providerSupportedEfforts.includes(effort));
}

function fallbackEffortFromSupported(
  requestedEffort: unknown,
  defaultEffort: AgentEffort,
  supportedEfforts: readonly AgentEffort[]
): AgentEffort {
  if (isAgentEffort(requestedEffort) && supportedEfforts.includes(requestedEffort)) {
    return requestedEffort;
  }
  if (requestedEffort === "ultra" && supportedEfforts.includes("max")) {
    return "max";
  }
  if (supportedEfforts.includes(defaultEffort)) {
    return defaultEffort;
  }
  if (isAgentEffort(requestedEffort)) {
    const requestedRank = effortOrder(requestedEffort);
    const closestSupportedEffort = [...supportedEfforts]
      .reverse()
      .find((effort) => effortOrder(effort) <= requestedRank);
    if (closestSupportedEffort) {
      return closestSupportedEffort;
    }
  }
  return supportedEfforts[0] ?? defaultEffort;
}

function defaultModelEntryForProvider(
  provider: AgentProvider,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG,
  providerSupportedModelAliases?: readonly unknown[] | null
): AgentModelCatalogEntry {
  const models = registry[provider];
  const fallbackModels = models.length === 0 ? AGENT_MODEL_CATALOG[provider] : [];
  const supportedDefaultModelId = SUPPORTED_DEFAULT_MODEL_BY_PROVIDER[provider];
  const supportedDefault =
    supportedDefaultModelId != null
      ? models.find((model) => model.id === supportedDefaultModelId) ??
        fallbackModels.find((model) => model.id === supportedDefaultModelId)
      : undefined;
  if (
    supportedDefault &&
    isAgentModelSelectableForProvider(
      provider,
      supportedDefault.id,
      providerSupportedModelAliases
    )
  ) {
    return supportedDefault;
  }

  const providerDefaultModelId = DEFAULT_MODEL_BY_PROVIDER[provider];
  const explicitDefault =
    models.find((model) => model.id === providerDefaultModelId) ??
    fallbackModels.find((model) => model.id === providerDefaultModelId);
  if (
    explicitDefault &&
    isAgentModelSelectableForProvider(
      provider,
      explicitDefault.id,
      providerSupportedModelAliases
    )
  ) {
    return explicitDefault;
  }

  const firstSelectable =
    models.find((model) =>
      isAgentModelSelectableForProvider(
        provider,
        model.id,
        providerSupportedModelAliases
      )
    ) ??
    fallbackModels.find((model) =>
      isAgentModelSelectableForProvider(
        provider,
        model.id,
        providerSupportedModelAliases
      )
    );
  return firstSelectable ?? explicitDefault ?? fallbackModels[0] ?? CODEX_MODEL_CATALOG[0];
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
  const exact = registry[provider].find((model) => model.id === normalizedModelId);
  if (exact) {
    return exact;
  }
  const normalizedAlias = normalizeCodexGpt56ModelAlias(provider, normalizedModelId);
  if (normalizedAlias !== normalizedModelId) {
    return registry[provider].find((model) => model.id === normalizedAlias) ?? null;
  }
  return null;
}

function normalizeModelId(value: string): string {
  return value.trim().toLowerCase();
}

export function isClaudeFableModelId(modelId: string): boolean {
  const normalized = normalizeModelId(modelId);
  return normalized === "fable" || normalized === "claude-fable-5";
}

export function isClaudeSonnet5ModelId(modelId: string): boolean {
  return normalizeModelId(modelId) === "claude-sonnet-5";
}

export function isClaudeSonnet46ModelId(modelId: string): boolean {
  return normalizeModelId(modelId) === "claude-sonnet-4-6";
}

export function isClaudeOpus47ModelId(modelId: string): boolean {
  return normalizeModelId(modelId) === "claude-opus-4-7";
}

export function isClaudeOpus48ModelId(modelId: string): boolean {
  return normalizeModelId(modelId) === "claude-opus-4-8";
}

export function isClaudeOpus5ModelId(modelId: string): boolean {
  return normalizeModelId(modelId) === "claude-opus-5";
}

function isClaudeCapabilityGatedModelId(modelId: string): boolean {
  return (
    isClaudeFableModelId(modelId) ||
    isClaudeSonnet46ModelId(modelId) ||
    isClaudeSonnet5ModelId(modelId) ||
    isClaudeOpus47ModelId(modelId) ||
    isClaudeOpus48ModelId(modelId) ||
    isClaudeOpus5ModelId(modelId)
  );
}

function normalizeCodexGpt56ModelAlias(
  provider: AgentProvider,
  modelId: string
): string {
  if (provider !== "codex") {
    return modelId;
  }
  const normalized = normalizeModelId(modelId);
  return normalized === "gpt-5.6" ? "gpt-5.6-sol" : normalized;
}

function isCodexGpt56ModelId(modelId: string): boolean {
  const normalized = normalizeModelId(modelId);
  return (
    normalized === "gpt-5.6" ||
    normalized === "gpt-5.6-sol" ||
    normalized === "gpt-5.6-terra" ||
    normalized === "gpt-5.6-luna"
  );
}

function isClaudeGatedModelSupportedByAlias(modelId: string, alias: string): boolean {
  if (isClaudeFableModelId(modelId)) {
    return isClaudeFableModelId(alias);
  }
  if (isClaudeSonnet46ModelId(modelId)) {
    return isClaudeSonnet5ModelId(alias);
  }
  if (isClaudeSonnet5ModelId(modelId)) {
    return isClaudeSonnet5ModelId(alias);
  }
  if (isClaudeOpus47ModelId(modelId)) {
    return isClaudeOpus47ModelId(alias);
  }
  if (isClaudeOpus48ModelId(modelId)) {
    return isClaudeOpus48ModelId(alias);
  }
  if (isClaudeOpus5ModelId(modelId)) {
    return isClaudeOpus5ModelId(alias);
  }
  return false;
}

function isCodexGpt56ModelSupportedByAlias(modelId: string, alias: string): boolean {
  return (
    normalizeCodexGpt56ModelAlias("codex", modelId) ===
    normalizeCodexGpt56ModelAlias("codex", alias)
  );
}

function shouldValidateProviderModelAliases(
  provider: AgentProvider,
  modelId: string,
  providerSupportedModelAliases?: readonly unknown[] | null
): boolean {
  if (provider === "codex" && isCodexGpt56ModelId(modelId)) {
    return true;
  }
  return (
    provider === "claude" &&
    isClaudeCapabilityGatedModelId(modelId) &&
    providerSupportedModelAliases !== undefined
  );
}

export function isAgentModelSelectableForProvider(
  provider: AgentProvider,
  modelId: string,
  providerSupportedModelAliases?: readonly unknown[] | null
): boolean {
  if (provider === "codex" && isCodexGpt56ModelId(modelId)) {
    if (providerSupportedModelAliases == null) {
      return false;
    }
    return providerSupportedModelAliases.some(
      (alias) =>
        typeof alias === "string" &&
        isCodexGpt56ModelSupportedByAlias(modelId, alias)
    );
  }

  if (provider !== "claude" || !isClaudeCapabilityGatedModelId(modelId)) {
    return true;
  }

  if (providerSupportedModelAliases == null) {
    return false;
  }

  return providerSupportedModelAliases.some(
    (alias) =>
      typeof alias === "string" &&
      isClaudeGatedModelSupportedByAlias(modelId, alias)
  );
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

    const supportsCodexUltra =
      model.provider === "codex" &&
      (model.supportsCodexUltra === true || model.supportedEfforts.includes("ultra"));
    const supportedEfforts = normalizeOrdinarySupportedEfforts(model.supportedEfforts);
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
      ...(supportsCodexUltra ? { supportsCodexUltra: true } : {}),
      ...(model.description ? { description: model.description } : {}),
      ...(isAgentModelSource(model.source) ? { source: model.source } : {}),
      enabled: true,
    });
  }

  return {
    claude: registry.claude,
    codex: registry.codex,
  };
}

export function defaultModelForProvider(
  provider: AgentProvider,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG,
  providerSupportedModelAliases?: readonly unknown[] | null
): string {
  return defaultModelEntryForProvider(
    provider,
    registry,
    providerSupportedModelAliases
  ).id;
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
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG,
  providerSupportedModelAliases?: readonly unknown[] | null
): readonly AgentModelCatalogEntry[] {
  return registry[provider].filter((model) =>
    isAgentModelSelectableForProvider(
      provider,
      model.id,
      providerSupportedModelAliases
    )
  );
}

export function agentEffortOptionsForModel(
  provider: AgentProvider,
  modelId: string,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG,
  providerSupportedEfforts?: readonly unknown[] | null
): AgentEffortCatalogEntry[] {
  const providerEfforts =
    providerSupportedEfforts != null
      ? normalizeSupportedEfforts(providerSupportedEfforts)
      : [];
  const modelSupportedEfforts =
    findModelEntryForProvider(provider, modelId, registry)?.supportedEfforts ??
    (providerEfforts.length > 0 ? providerEfforts : defaultEffortsForProvider(provider));
  const supportedEfforts = intersectSupportedEfforts(
    modelSupportedEfforts,
    providerSupportedEfforts
  );
  return AGENT_EFFORT_CATALOG.filter((effort) =>
    supportedEfforts.includes(effort.id)
  ).map((effort) => ({
    ...effort,
    description: effortDescriptionForProvider(provider, effort.id),
  }));
}

function defaultEffortsForProvider(provider: AgentProvider): readonly AgentEffort[] {
  return provider === "codex"
    ? ["low", "medium", "high", "xhigh", "max"]
    : ["low", "medium", "high"];
}

export function agentModelSupportsCodexUltra(
  provider: AgentProvider,
  modelId: string,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG,
  providerUltraSupportedModels?: readonly unknown[] | null
): boolean {
  if (provider !== "codex") {
    return false;
  }
  const model = findModelEntryForProvider(provider, modelId, registry);
  return (
    model?.supportsCodexUltra === true &&
    providerUltraSupportedModels != null &&
    providerUltraSupportedModels.some(
      (supportedModel) =>
        typeof supportedModel === "string" &&
        isCodexGpt56ModelSupportedByAlias(model.id, supportedModel)
    )
  );
}

function effortDescriptionForProvider(
  provider: AgentProvider,
  effort: AgentEffort
): string {
  if (provider === "codex") {
    return CODEX_EFFORT_DESCRIPTIONS[effort];
  }
  return CLAUDE_EFFORT_DESCRIPTIONS[effort];
}

export function normalizeAgentRuntimeSelection(
  runtime: unknown,
  registry: AgentModelRegistry = AGENT_MODEL_CATALOG,
  providerSupportedEfforts?: readonly unknown[] | null,
  providerSupportedModelAliases?: readonly unknown[] | null
): AgentRuntimeSelection {
  const defaultEntry = defaultModelEntryForProvider(
    "codex",
    registry,
    providerSupportedModelAliases
  );
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
  if (
    requestedModelId &&
    shouldValidateProviderModelAliases(
      provider,
      requestedModelId,
      providerSupportedModelAliases
    ) &&
    !isAgentModelSelectableForProvider(
      provider,
      requestedModelId,
      providerSupportedModelAliases
    )
  ) {
    const model = defaultModelEntryForProvider(
      provider,
      registry,
      providerSupportedModelAliases
    );
    const supportedEfforts = intersectSupportedEfforts(
      model.supportedEfforts,
      providerSupportedEfforts
    );
    const effort = fallbackEffortFromSupported(
      candidate.effort,
      model.defaultEffort,
      supportedEfforts
    );
    return {
      provider,
      modelId: model.id,
      effort,
    };
  }

  if (!knownModel && requestedModelId) {
    const providerEfforts =
      providerSupportedEfforts != null
        ? normalizeSupportedEfforts(providerSupportedEfforts)
        : [];
    const supportedEfforts = intersectSupportedEfforts(
      providerEfforts.length > 0 ? providerEfforts : defaultEffortsForProvider(provider),
      providerSupportedEfforts
    );
    const effort = fallbackEffortFromSupported(
      candidate.effort,
      providerDefaultEffort(provider, registry),
      supportedEfforts
    );
    return {
      provider,
      modelId: requestedModelId,
      effort,
    };
  }

  const model =
    knownModel ??
    defaultModelEntryForProvider(provider, registry, providerSupportedModelAliases);
  const supportedEfforts = intersectSupportedEfforts(
    model.supportedEfforts,
    providerSupportedEfforts
  );
  const effort = fallbackEffortFromSupported(
    candidate.effort,
    model.defaultEffort,
    supportedEfforts
  );

  return {
    provider,
    modelId: model.id,
    effort,
  };
}

export function normalizeAgentRuntimeForPersistence(
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
    const effort = fallbackEffortFromSupported(
      candidate.effort,
      providerDefaultEffort(provider, registry),
      defaultEffortsForProvider(provider)
    );
    return {
      provider,
      modelId: requestedModelId,
      effort,
    };
  }

  const model = knownModel ?? defaultModelEntryForProvider(provider, registry);
  const effort = fallbackEffortFromSupported(
    candidate.effort,
    model.defaultEffort,
    model.supportedEfforts
  );

  return {
    provider,
    modelId: model.id,
    effort,
  };
}
