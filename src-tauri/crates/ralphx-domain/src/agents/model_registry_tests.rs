use super::*;

#[test]
fn built_in_registry_tracks_provider_model_effort_compatibility() {
    let snapshot = AgentModelRegistrySnapshot::merged(Vec::new());

    let opus = snapshot
        .find_enabled(AgentHarnessKind::Claude, "opus")
        .expect("opus should be registered");
    assert_eq!(
        opus.supported_efforts,
        vec![
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::XHigh,
            LogicalEffort::Max,
        ]
    );
    assert_eq!(opus.default_effort, LogicalEffort::XHigh);

    let codex_mini = snapshot
        .find_enabled(AgentHarnessKind::Codex, "gpt-5.4-mini")
        .expect("gpt-5.4-mini should be registered");
    assert_eq!(
        codex_mini.supported_efforts,
        vec![
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High
        ]
    );
    assert_eq!(codex_mini.default_effort, LogicalEffort::Medium);
}

#[test]
fn custom_models_override_built_ins() {
    let snapshot = AgentModelRegistrySnapshot::merged(vec![AgentModelDefinition::custom(
        AgentHarnessKind::Codex,
        "gpt-5.5",
        "Internal GPT-5.5",
        "Internal GPT-5.5",
        None,
        vec![LogicalEffort::Low, LogicalEffort::Medium],
        LogicalEffort::Medium,
        true,
    )]);

    let model = snapshot
        .find_enabled(AgentHarnessKind::Codex, "gpt-5.5")
        .expect("custom override should remain enabled");
    assert_eq!(model.source, AgentModelSource::Custom);
    assert_eq!(model.menu_label, "Internal GPT-5.5");
    assert_eq!(
        model.supported_efforts,
        vec![LogicalEffort::Low, LogicalEffort::Medium]
    );
}

#[test]
fn built_in_registry_exposes_expected_defaults_for_each_provider() {
    let models = built_in_agent_models();

    assert_eq!(models.len(), 8);
    assert_eq!(default_model_for_provider(AgentHarnessKind::Claude), "sonnet");
    assert_eq!(default_model_for_provider(AgentHarnessKind::Codex), "gpt-5.5");
    assert_eq!(lightweight_model_for_provider(AgentHarnessKind::Claude), "haiku");
    assert_eq!(
        lightweight_model_for_provider(AgentHarnessKind::Codex),
        "gpt-5.4-mini"
    );
    assert_eq!(
        default_effort_for_provider(AgentHarnessKind::Claude),
        LogicalEffort::Medium
    );
    assert_eq!(
        default_effort_for_provider(AgentHarnessKind::Codex),
        LogicalEffort::XHigh
    );
    assert_eq!(
        default_efforts_for_provider(AgentHarnessKind::Claude),
        &[
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High
        ]
    );
    assert_eq!(
        default_efforts_for_provider(AgentHarnessKind::Codex),
        &[
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::XHigh,
        ]
    );

    let snapshot = AgentModelRegistrySnapshot::merged(Vec::new());
    let claude_models: Vec<_> = snapshot
        .enabled_for_provider(AgentHarnessKind::Claude)
        .map(|model| model.model_id.as_str())
        .collect();
    let codex_models: Vec<_> = snapshot
        .enabled_for_provider(AgentHarnessKind::Codex)
        .map(|model| model.model_id.as_str())
        .collect();

    assert_eq!(claude_models, vec!["sonnet", "opus", "haiku"]);
    assert_eq!(
        codex_models,
        vec![
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.4-mini",
            "gpt-5.3-codex",
            "gpt-5.3-codex-spark",
        ]
    );
    assert_eq!(
        snapshot
            .default_for_provider(AgentHarnessKind::Codex)
            .map(|model| model.model_id.as_str()),
        Some("gpt-5.5")
    );
}

#[test]
fn custom_model_normalization_trims_sorts_and_falls_back_safely() {
    let normalized = AgentModelDefinition::custom(
        AgentHarnessKind::Claude,
        "  claude-opus-5  ",
        "  ",
        "  ",
        Some("  \n ".to_string()),
        vec![
            LogicalEffort::High,
            LogicalEffort::Low,
            LogicalEffort::High,
        ],
        LogicalEffort::Max,
        true,
    )
    .normalized();

    assert_eq!(normalized.model_id, "claude-opus-5");
    assert_eq!(normalized.label, "claude-opus-5");
    assert_eq!(normalized.menu_label, "claude-opus-5");
    assert_eq!(normalized.description, None);
    assert_eq!(
        normalized.supported_efforts,
        vec![LogicalEffort::Low, LogicalEffort::High]
    );
    assert_eq!(normalized.default_effort, LogicalEffort::Low);

    let provider_defaults = AgentModelDefinition::custom(
        AgentHarnessKind::Codex,
        "gpt-5.6",
        "GPT-5.6",
        "",
        Some(" next model ".to_string()),
        Vec::new(),
        LogicalEffort::Max,
        false,
    )
    .normalized();

    assert_eq!(provider_defaults.menu_label, "GPT-5.6");
    assert_eq!(
        provider_defaults.description.as_deref(),
        Some("next model")
    );
    assert_eq!(
        provider_defaults.supported_efforts,
        default_efforts_for_provider(AgentHarnessKind::Codex)
    );
    assert_eq!(provider_defaults.default_effort, LogicalEffort::XHigh);
    assert!(!provider_defaults.enabled);
}

#[test]
fn disabled_custom_override_is_not_selectable_as_default() {
    let snapshot = AgentModelRegistrySnapshot::merged(vec![AgentModelDefinition::custom(
        AgentHarnessKind::Codex,
        "gpt-5.5",
        "Disabled GPT-5.5",
        "Disabled GPT-5.5",
        None,
        vec![LogicalEffort::Low],
        LogicalEffort::Low,
        false,
    )]);

    assert!(snapshot
        .find_enabled(AgentHarnessKind::Codex, "gpt-5.5")
        .is_none());
    assert_eq!(
        snapshot
            .default_for_provider(AgentHarnessKind::Codex)
            .map(|model| model.model_id.as_str()),
        Some("gpt-5.4")
    );
}
