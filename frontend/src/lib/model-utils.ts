/**
 * Model ID → display label mapping for RalphX agents.
 *
 * Backend counterpart: src-tauri/src/infrastructure/agents/claude/model_labels.rs
 * When a new model is added to ralphx.yaml or model_resolver.rs,
 * BOTH files must be updated.
 */

const MODEL_LABEL_MAP: Record<string, string> = {
  // Short aliases used in ralphx.yaml and YAML agent configs
  sonnet: "Sonnet 4.6",
  opus: "Opus 4.6",
  haiku: "Haiku 4.5",
  "gpt-5.5": "GPT-5.5",
  "gpt-5.4": "GPT-5.4",
  "gpt-5.4-mini": "GPT-5.4 Mini",
  "gpt-5.3-codex": "GPT-5.3 Codex",
  "gpt-5.3-codex-spark": "GPT-5.3 Codex Spark",
  "gpt-4.5": "GPT-4.5",
  // Full model IDs (Claude API format)
  "claude-sonnet-4-6": "Sonnet 4.6",
  "claude-opus-4-6": "Opus 4.6",
  "claude-haiku-4-5-20251001": "Haiku 4.5",
};

/**
 * Map a raw model ID string to a human-readable display label.
 *
 * Fallback policy: if the ID is not in the table, the raw ID is returned as-is.
 * The function never returns an empty string for non-empty input.
 */
export function getModelLabel(id: string): string {
  return MODEL_LABEL_MAP[id] ?? id;
}
