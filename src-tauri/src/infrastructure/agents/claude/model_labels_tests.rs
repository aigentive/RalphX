use super::*;
use crate::infrastructure::agents::harness_agent_catalog::{
    list_canonical_prompt_backed_agents, load_canonical_claude_metadata, AgentPromptHarness,
};

#[test]
fn test_short_alias_labels() {
    assert_eq!(model_id_to_label("sonnet"), "Sonnet");
    assert_eq!(model_id_to_label("opus"), "Opus");
    assert_eq!(model_id_to_label("haiku"), "Haiku 4.5");
    assert_eq!(model_id_to_label("fable"), "Fable 5");
    assert_eq!(model_id_to_label("gpt-5.5"), "GPT-5.5");
    assert_eq!(model_id_to_label("gpt-5.4"), "GPT-5.4");
    assert_eq!(model_id_to_label("gpt-5.4-mini"), "GPT-5.4 Mini");
    assert_eq!(model_id_to_label("gpt-5.3-codex"), "GPT-5.3 Codex");
    assert_eq!(
        model_id_to_label("gpt-5.3-codex-spark"),
        "GPT-5.3 Codex Spark"
    );
    assert_eq!(model_id_to_label("gpt-4.5"), "GPT-4.5");
}

#[test]
fn test_full_model_id_labels() {
    assert_eq!(model_id_to_label("claude-sonnet-5"), "Sonnet 5");
    assert_eq!(model_id_to_label("claude-sonnet-4-6"), "Sonnet 4.6");
    assert_eq!(model_id_to_label("claude-opus-4-6"), "Opus 4.6");
    assert_eq!(model_id_to_label("claude-opus-4-7"), "Opus 4.7");
    assert_eq!(model_id_to_label("claude-opus-4-8"), "Opus 4.8");
    assert_eq!(model_id_to_label("claude-opus-5"), "Opus 5");
    assert_eq!(model_id_to_label("claude-haiku-4-5-20251001"), "Haiku 4.5");
    assert_eq!(model_id_to_label("claude-fable-5"), "Fable 5");
}

#[test]
fn test_unknown_id_returns_raw() {
    assert_eq!(model_id_to_label("unknown-model"), "unknown-model");
    assert_eq!(model_id_to_label("z-ai/glm-4.7"), "z-ai/glm-4.7");
    assert_eq!(model_id_to_label(""), "");
}

/// Drift-prevention test: every live canonical Claude model value must have a distinct
/// display label (not equal to the raw ID).
///
/// Run: cargo nextest run --manifest-path src-tauri/Cargo.toml --lib -E 'test(test_all_yaml_models_have_labels)'
#[test]
fn test_all_yaml_models_have_labels() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = manifest_dir.join("..");
    let models: std::collections::HashSet<String> =
        list_canonical_prompt_backed_agents(&project_root, AgentPromptHarness::Claude)
            .into_iter()
            .filter_map(|agent_name| {
                load_canonical_claude_metadata(&project_root, &agent_name).model
            })
            .filter(|model| !model.is_empty() && !model.starts_with('<'))
            .collect();

    assert!(
        !models.is_empty(),
        "No model values found in canonical Claude agent metadata — check agents/*/agent.yaml"
    );

    for model_id in &models {
        let label = model_id_to_label(model_id.as_str());
        assert_ne!(
                label.as_str(), model_id.as_str(),
                "model_id_to_label({model_id:?}) returned the raw ID — add it to the mapping table in model_labels.rs and frontend/src/lib/model-utils.ts"
            );
        assert!(
            !label.is_empty(),
            "model_id_to_label({model_id:?}) returned an empty label"
        );
    }
}
