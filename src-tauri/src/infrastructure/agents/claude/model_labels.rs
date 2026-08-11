// Model ID → display label mapping for RalphX agents.
//
// Catalog construction (2026-04-06):
//   grep -r 'model' src-tauri/src/infrastructure/agents/claude/model_resolver.rs | grep -v '//' | head -40
//   grep -r 'model:' agents/*/claude/agent.yaml agents/*/agent.yaml | grep -v '#' | head -20
//
// Unique model strings found in config/ralphx.yaml and agent .md files:
//   sonnet, opus, haiku, fable  (short aliases used by shared runtime config)
//
// Full model IDs (claude-*) are included in the table as forward-mapping entries
// for when they appear in runtime --model output or are explicitly set.
//
// Frontend counterpart: frontend/src/lib/model-utils.ts
// When a new model is added to config/ralphx.yaml or model_resolver.rs, BOTH files must be updated.

/// Map a raw model ID string to a human-readable display label.
///
/// Fallback policy: if the ID is not in the table, the raw ID is returned as-is
/// (so the function never returns an empty string). The caller should provide
/// the raw ID in a tooltip for full-fidelity display.
// Phase 1 uses this when emitting agent:run_started events with model label.
#[allow(dead_code)]
pub(crate) fn model_id_to_label(id: &str) -> String {
    match id {
        // Short aliases used in config/ralphx.yaml and YAML agent configs
        "sonnet" => "Sonnet",
        "opus" => "Opus",
        "haiku" => "Haiku 4.5",
        "fable" => "Fable 5",
        "gpt-5.5" => "GPT-5.5",
        "gpt-5.4" => "GPT-5.4",
        "gpt-5.4-mini" => "GPT-5.4 Mini",
        "gpt-5.3-codex" => "GPT-5.3 Codex",
        "gpt-5.3-codex-spark" => "GPT-5.3 Codex Spark",
        "gpt-4.5" => "GPT-4.5",
        // Full model IDs (Claude API format)
        "claude-sonnet-5" => "Sonnet 5",
        "claude-sonnet-4-6" => "Sonnet 4.6",
        "claude-opus-4-6" => "Opus 4.6",
        "claude-opus-4-7" => "Opus 4.7",
        "claude-opus-4-8" => "Opus 4.8",
        "claude-opus-5" => "Opus 5",
        "claude-haiku-4-5-20251001" => "Haiku 4.5",
        "claude-fable-5" => "Fable 5",
        // Fallback: return raw ID so the chip is never blank
        other => return other.to_string(),
    }
    .to_string()
}

#[cfg(test)]
#[path = "model_labels_tests.rs"]
mod tests;
