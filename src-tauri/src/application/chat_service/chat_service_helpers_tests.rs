use super::*;
use crate::domain::agents::AgentHarnessKind;

#[test]
fn test_resolve_agent_review_approved_returns_review_history() {
    let agent = resolve_agent(&ChatContextType::Review, Some("approved"));
    assert_eq!(agent, AGENT_REVIEW_HISTORY);
}

#[test]
fn test_resolve_agent_review_passed_returns_review_chat() {
    let agent = resolve_agent(&ChatContextType::Review, Some("review_passed"));
    assert_eq!(agent, AGENT_REVIEW_CHAT);
}

#[test]
fn test_resolve_agent_review_default_returns_reviewer() {
    let agent = resolve_agent(&ChatContextType::Review, None);
    assert_eq!(agent, AGENT_REVIEWER);
}

#[test]
fn test_resolve_agent_review_other_status_returns_reviewer() {
    let agent = resolve_agent(&ChatContextType::Review, Some("reviewing"));
    assert_eq!(agent, AGENT_REVIEWER);
}

#[test]
fn test_resolve_agent_ideation_accepted_returns_readonly() {
    let agent = resolve_agent(&ChatContextType::Ideation, Some("accepted"));
    assert_eq!(agent, AGENT_ORCHESTRATOR_IDEATION_READONLY);
}

#[test]
fn test_legacy_verification_purpose_uses_normal_ideation_agent() {
    let agent = resolve_agent(&ChatContextType::Ideation, Some("verification"));
    assert_eq!(agent, AGENT_ORCHESTRATOR_IDEATION);
}

#[test]
fn test_rx_native_team_is_supported_by_standard_harnesses() {
    assert!(harness_supports_rx_native_team(AgentHarnessKind::Claude));
    assert!(harness_supports_rx_native_team(AgentHarnessKind::Codex));
}

#[test]
fn test_effective_effort_for_claude_prefers_claude_effort() {
    assert_eq!(
        effective_effort_for_harness(
            AgentHarnessKind::Claude,
            Some("max"),
            Some(crate::domain::agents::LogicalEffort::High),
        ),
        "max"
    );
}

#[test]
fn test_effective_effort_for_codex_uses_logical_effort() {
    assert_eq!(
        effective_effort_for_harness(
            AgentHarnessKind::Codex,
            Some("max"),
            Some(crate::domain::agents::LogicalEffort::XHigh),
        ),
        "xhigh"
    );
}

#[test]
fn test_effective_effort_defaults_to_medium() {
    assert_eq!(
        effective_effort_for_harness(AgentHarnessKind::Claude, None, None),
        "medium"
    );
    assert_eq!(
        effective_effort_for_harness(AgentHarnessKind::Codex, None, None),
        "medium"
    );
}

#[test]
fn test_effective_model_label_for_codex_uses_raw_model_id() {
    assert_eq!(
        effective_model_label_for_harness(AgentHarnessKind::Codex, "gpt-4.5"),
        "gpt-4.5"
    );
}

#[test]
fn test_harness_supports_merge_completion_watcher_only_for_claude() {
    assert!(harness_supports_merge_completion_watcher(
        AgentHarnessKind::Claude
    ));
    assert!(!harness_supports_merge_completion_watcher(
        AgentHarnessKind::Codex
    ));
}

#[test]
fn fresh_provider_session_policy_rejects_implicit_workflow_continuation() {
    assert!(should_start_fresh_provider_session(
        false,
        false,
        Some("ralphx-pr-fixer")
    ));
    assert!(should_start_fresh_provider_session(false, true, None));
    assert!(should_start_fresh_provider_session(true, false, None));
    assert!(!should_start_fresh_provider_session(false, false, None));
}

#[test]
fn provider_session_model_compatibility_requires_an_exact_known_match() {
    assert!(provider_session_model_matches_requested(None, "gpt-5.5"));
    assert!(provider_session_model_matches_requested(
        Some("gpt-5.5"),
        "gpt-5.5"
    ));
    assert!(!provider_session_model_matches_requested(
        Some("gpt-5.6"),
        "gpt-5.5"
    ));
}

#[test]
fn test_context_type_to_process_mapping() {
    assert_eq!(
        context_type_to_process(&ChatContextType::Ideation),
        "ideation"
    );
    assert_eq!(context_type_to_process(&ChatContextType::Task), "task");
    assert_eq!(
        context_type_to_process(&ChatContextType::Project),
        "project"
    );
    assert_eq!(
        context_type_to_process(&ChatContextType::TaskExecution),
        "execution"
    );
    assert_eq!(context_type_to_process(&ChatContextType::Review), "review");
    assert_eq!(context_type_to_process(&ChatContextType::Merge), "merge");
}

#[test]
fn test_get_assistant_role_uses_orchestrator_for_ideation_chat() {
    assert_eq!(
        get_assistant_role(&ChatContextType::Ideation),
        MessageRole::Orchestrator
    );
    assert_eq!(
        get_assistant_role(&ChatContextType::Task),
        MessageRole::Orchestrator
    );
    assert_eq!(
        get_assistant_role(&ChatContextType::Project),
        MessageRole::Orchestrator
    );
}
