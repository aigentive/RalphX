use super::*;
use crate::domain::agents::AgentHarnessKind;
use crate::infrastructure::agents::claude::agent_names::AGENT_PLAN_VERIFIER;

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
fn test_resolve_agent_ideation_verification_returns_plan_verifier() {
    // Verification sessions route to ralphx-plan-verifier (prevents infinite cascade)
    let agent = resolve_agent(&ChatContextType::Ideation, Some("verification"));
    assert_eq!(agent, AGENT_PLAN_VERIFIER);
}

#[test]
fn test_resolve_agent_ideation_verification_overrides_team_mode() {
    // Verification purpose takes priority over team_mode
    let agent =
        resolve_agent_with_team_mode(&ChatContextType::Ideation, Some("verification"), true);
    assert_eq!(agent, AGENT_PLAN_VERIFIER);
}

#[test]
fn test_resolve_agent_team_mode_ideation_returns_team_lead() {
    let agent = resolve_agent_with_team_mode(&ChatContextType::Ideation, None, true);
    assert_eq!(agent, AGENT_IDEATION_TEAM_LEAD);
}

#[test]
fn test_resolve_agent_team_mode_execution_returns_worker_team() {
    let agent = resolve_agent_with_team_mode(&ChatContextType::TaskExecution, None, true);
    assert_eq!(agent, AGENT_WORKER_TEAM);
}

#[test]
fn test_resolve_agent_team_mode_project_falls_back_to_default() {
    // Contexts without team variants fall back to defaults
    let agent = resolve_agent_with_team_mode(&ChatContextType::Project, None, true);
    assert_eq!(agent, AGENT_CHAT_PROJECT);
}

#[test]
fn test_resolve_agent_team_mode_false_returns_default() {
    let agent = resolve_agent_with_team_mode(&ChatContextType::Ideation, None, false);
    assert_eq!(agent, AGENT_ORCHESTRATOR_IDEATION);
}

#[test]
fn test_resolve_agent_team_mode_status_overrides_team() {
    // Status-specific rules take priority over team_mode
    let agent = resolve_agent_with_team_mode(&ChatContextType::Ideation, Some("accepted"), true);
    assert_eq!(agent, AGENT_ORCHESTRATOR_IDEATION_READONLY);
}

#[test]
fn test_effective_team_mode_for_harness_keeps_claude_team_mode() {
    assert!(effective_team_mode_for_harness(
        true,
        AgentHarnessKind::Claude
    ));
}

#[test]
fn test_effective_team_mode_for_harness_disables_codex_team_mode() {
    assert!(!effective_team_mode_for_harness(
        true,
        AgentHarnessKind::Codex
    ));
}

#[test]
fn test_harness_supports_team_mode_only_for_claude() {
    assert!(harness_supports_team_mode(AgentHarnessKind::Claude));
    assert!(!harness_supports_team_mode(AgentHarnessKind::Codex));
}

#[test]
fn test_rx_native_team_is_supported_by_standard_harnesses() {
    assert!(harness_supports_rx_native_team(AgentHarnessKind::Claude));
    assert!(harness_supports_rx_native_team(AgentHarnessKind::Codex));
}

#[test]
fn test_effective_team_mode_for_harness_respects_requested_false() {
    assert!(!effective_team_mode_for_harness(
        false,
        AgentHarnessKind::Claude
    ));
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
