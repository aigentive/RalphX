use super::*;

#[test]
fn test_agent_harness_display() {
    assert_eq!(AgentHarnessKind::Claude.to_string(), "claude");
    assert_eq!(AgentHarnessKind::Codex.to_string(), "codex");
}

#[test]
fn test_agent_harness_parse() {
    assert_eq!(
        "claude".parse::<AgentHarnessKind>().unwrap(),
        AgentHarnessKind::Claude
    );
    assert_eq!(
        "codex".parse::<AgentHarnessKind>().unwrap(),
        AgentHarnessKind::Codex
    );
    assert!("gemini".parse::<AgentHarnessKind>().is_err());
}

#[test]
fn test_harness_to_client_type() {
    assert_eq!(
        ClientType::from(AgentHarnessKind::Claude),
        ClientType::ClaudeCode
    );
    assert_eq!(ClientType::from(AgentHarnessKind::Codex), ClientType::Codex);
}

#[test]
fn test_client_type_to_harness() {
    assert_eq!(
        AgentHarnessKind::try_from(ClientType::ClaudeCode).unwrap(),
        AgentHarnessKind::Claude
    );
    assert_eq!(
        AgentHarnessKind::try_from(ClientType::Codex).unwrap(),
        AgentHarnessKind::Codex
    );
    assert!(AgentHarnessKind::try_from(ClientType::Mock).is_err());
}

#[test]
fn test_logical_effort_parse_and_display() {
    assert_eq!(LogicalEffort::Low.to_string(), "low");
    assert_eq!(LogicalEffort::Medium.to_string(), "medium");
    assert_eq!(LogicalEffort::High.to_string(), "high");
    assert_eq!(LogicalEffort::XHigh.to_string(), "xhigh");
    assert_eq!(LogicalEffort::Max.to_string(), "max");

    assert_eq!("low".parse::<LogicalEffort>().unwrap(), LogicalEffort::Low);
    assert_eq!(
        "xhigh".parse::<LogicalEffort>().unwrap(),
        LogicalEffort::XHigh
    );
    assert_eq!("max".parse::<LogicalEffort>().unwrap(), LogicalEffort::Max);
}

#[test]
fn test_logical_effort_to_legacy_claude_effort() {
    assert_eq!(LogicalEffort::Low.to_legacy_claude_effort(), "low");
    assert_eq!(LogicalEffort::Medium.to_legacy_claude_effort(), "medium");
    assert_eq!(LogicalEffort::High.to_legacy_claude_effort(), "high");
    assert_eq!(LogicalEffort::XHigh.to_legacy_claude_effort(), "xhigh");
    assert_eq!(LogicalEffort::Max.to_legacy_claude_effort(), "max");
}

#[test]
fn test_agent_lane_display_and_parse() {
    assert_eq!(AgentLane::IdeationPrimary.to_string(), "ideation_primary");
    assert_eq!(AgentLane::ExecutionMerger.to_string(), "execution_merger");
    assert_eq!(
        "ideation_verifier_subagent".parse::<AgentLane>().unwrap(),
        AgentLane::IdeationVerifierSubagent
    );
    assert!("unknown_lane".parse::<AgentLane>().is_err());
}

#[test]
fn test_provider_session_ref_serialization() {
    let session = ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-session-123".to_string(),
    };

    let json = serde_json::to_value(&session).unwrap();
    assert_eq!(json["harness"], "codex");
    assert_eq!(json["providerSessionId"], "codex-session-123");
}

#[test]
fn test_agent_lane_settings_new() {
    let settings = AgentLaneSettings::new(AgentHarnessKind::Claude);
    assert_eq!(settings.harness, AgentHarnessKind::Claude);
    assert!(settings.model.is_none());
    assert!(settings.effort.is_none());
    assert!(settings.approval_policy.is_none());
    assert!(settings.sandbox_mode.is_none());
}

#[test]
fn test_agent_lane_settings_serialization_omits_empty_optionals() {
    let settings = AgentLaneSettings::new(AgentHarnessKind::Codex);
    let json = serde_json::to_value(&settings).unwrap();

    assert_eq!(json["harness"], "codex");
    assert!(json.get("model").is_none());
    assert!(json.get("effort").is_none());
    assert!(json.get("approvalPolicy").is_none());
    assert!(json.get("sandboxMode").is_none());
}

#[test]
fn test_generic_harness_lane_defaults_for_codex_primary() {
    let settings =
        generic_harness_lane_defaults(AgentHarnessKind::Codex, AgentLane::IdeationPrimary);

    assert_eq!(settings.harness, AgentHarnessKind::Codex);
    assert_eq!(settings.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(settings.effort, Some(LogicalEffort::XHigh));
    assert_eq!(settings.approval_policy.as_deref(), Some("never"));
    assert_eq!(settings.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[test]
fn test_standard_agent_lane_defaults_use_expected_harnesses() {
    let defaults = standard_agent_lane_defaults();

    assert_eq!(
        defaults.get(&AgentLane::IdeationPrimary).unwrap().harness,
        AgentHarnessKind::Codex
    );
    assert_eq!(
        defaults.get(&AgentLane::ExecutionWorker).unwrap().harness,
        DEFAULT_AGENT_HARNESS
    );
}

#[test]
fn test_standard_harness_map_keys_first_class_harnesses() {
    let map = standard_harness_map("claude", "codex");

    assert_eq!(map.get(&AgentHarnessKind::Claude), Some(&"claude"));
    assert_eq!(map.get(&AgentHarnessKind::Codex), Some(&"codex"));
}

#[test]
fn test_standard_harness_registry_builds_all_first_class_harnesses() {
    let map = standard_harness_registry(|harness| harness.to_string());

    assert_eq!(map.len(), STANDARD_AGENT_HARNESSES.len());
    assert_eq!(
        map.get(&AgentHarnessKind::Claude),
        Some(&"claude".to_string())
    );
    assert_eq!(
        map.get(&AgentHarnessKind::Codex),
        Some(&"codex".to_string())
    );
}

#[test]
fn test_standard_harness_behavior_for_claude() {
    let behavior = standard_harness_behavior(AgentHarnessKind::Claude);

    assert!(behavior.honors_team_mode);
    assert!(behavior.supports_merge_completion_watcher);
    assert_eq!(
        behavior.model_label_strategy,
        HarnessModelLabelStrategy::ClaudeMapped
    );
    assert_eq!(
        behavior.effort_strategy,
        HarnessEffortStrategy::ClaudeEffortFirst
    );
    assert_eq!(behavior.stream_mode, HarnessStreamMode::ClaudeEvents);
}

#[test]
fn test_standard_harness_behavior_for_codex() {
    let behavior = standard_harness_behavior(AgentHarnessKind::Codex);

    assert!(!behavior.honors_team_mode);
    assert!(!behavior.supports_merge_completion_watcher);
    assert_eq!(
        behavior.model_label_strategy,
        HarnessModelLabelStrategy::RawModelId
    );
    assert_eq!(behavior.effort_strategy, HarnessEffortStrategy::LogicalOnly);
    assert_eq!(behavior.stream_mode, HarnessStreamMode::CodexJsonl);
}
