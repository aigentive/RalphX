use super::*;
use crate::agents::{AgentHarnessKind, LogicalEffort};
use crate::entities::ChatConversationId;

#[test]
fn test_agent_run_id_creation() {
    let id1 = AgentRunId::new();
    let id2 = AgentRunId::new();
    assert_ne!(id1, id2);
}

#[test]
fn test_agent_run_id_from_string() {
    let id = AgentRunId::new();
    let str_id = id.to_string();
    let parsed_id: AgentRunId = str_id.parse().unwrap();
    assert_eq!(id, parsed_id);
}

#[test]
fn test_status_serialization() {
    assert_eq!(AgentRunStatus::Running.to_string(), "running");
    assert_eq!(AgentRunStatus::Completed.to_string(), "completed");
    assert_eq!(AgentRunStatus::Failed.to_string(), "failed");
    assert_eq!(AgentRunStatus::Cancelled.to_string(), "cancelled");
}

#[test]
fn test_status_parsing() {
    assert_eq!(
        "running".parse::<AgentRunStatus>().unwrap(),
        AgentRunStatus::Running
    );
    assert_eq!(
        "completed".parse::<AgentRunStatus>().unwrap(),
        AgentRunStatus::Completed
    );
    assert_eq!(
        "failed".parse::<AgentRunStatus>().unwrap(),
        AgentRunStatus::Failed
    );
    assert_eq!(
        "cancelled".parse::<AgentRunStatus>().unwrap(),
        AgentRunStatus::Cancelled
    );
    assert!("invalid".parse::<AgentRunStatus>().is_err());
}

#[test]
fn test_status_is_terminal() {
    assert!(!AgentRunStatus::Running.is_terminal());
    assert!(AgentRunStatus::Completed.is_terminal());
    assert!(AgentRunStatus::Failed.is_terminal());
    assert!(AgentRunStatus::Cancelled.is_terminal());
}

#[test]
fn test_status_is_active() {
    assert!(AgentRunStatus::Running.is_active());
    assert!(!AgentRunStatus::Completed.is_active());
    assert!(!AgentRunStatus::Failed.is_active());
    assert!(!AgentRunStatus::Cancelled.is_active());
}

#[test]
fn test_new_agent_run() {
    let conversation_id = ChatConversationId::new();
    let run = AgentRun::new(conversation_id);

    assert_eq!(run.conversation_id, conversation_id);
    assert_eq!(run.status, AgentRunStatus::Running);
    assert!(run.is_active());
    assert!(!run.is_terminal());
    assert_eq!(run.completed_at, None);
    assert_eq!(run.error_message, None);
    assert_eq!(run.harness, None);
    assert_eq!(run.provider_session_id, None);
    assert_eq!(run.logical_model, None);
    assert_eq!(run.effective_model_id, None);
    assert_eq!(run.logical_effort, None);
    assert_eq!(run.effective_effort, None);
    assert_eq!(run.service_tier, None);
    assert_eq!(run.approval_policy, None);
    assert_eq!(run.sandbox_mode, None);
    assert!(run.run_chain_id.is_some());
    assert_eq!(run.parent_run_id, None);
    assert_eq!(run.action_kind, None);
    assert_eq!(run.action_context_id, None);
    assert_eq!(run.action_target_id, None);
}

#[test]
fn verify_plan_action_metadata_is_typed_and_roundtrips() {
    let parsed = AgentRunAction::from_metadata_json(Some(
        r#"{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"session-1","ralphx_action_target_id":"artifact-1"}"#,
    ))
    .expect("complete action tuple");
    assert_eq!(parsed.kind, AgentRunActionKind::VerifyPlan);
    assert_eq!(parsed.context_id, "session-1");
    assert_eq!(parsed.target_id, "artifact-1");

    let mut run = AgentRun::new(ChatConversationId::new());
    run.apply_action_metadata_json(Some(
        r#"{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"session-1","ralphx_action_target_id":"artifact-1"}"#,
    ));

    assert_eq!(run.action_kind, Some(AgentRunActionKind::VerifyPlan));
    assert_eq!(run.action_context_id.as_deref(), Some("session-1"));
    assert_eq!(run.action_target_id.as_deref(), Some("artifact-1"));
    assert_eq!(AgentRunActionKind::VerifyPlan.to_string(), "verify_plan");
    assert_eq!(
        "verify_plan".parse::<AgentRunActionKind>().unwrap(),
        AgentRunActionKind::VerifyPlan
    );
}

#[test]
fn workspace_review_fixer_action_metadata_is_typed_and_roundtrips() {
    let parsed = AgentRunAction::from_metadata_json(Some(
        r#"{"ralphx_action_kind":"workspace_review_fixer","ralphx_action_context_id":"conversation-1","ralphx_action_target_id":"attempt-1"}"#,
    ))
    .expect("complete action tuple");

    assert_eq!(parsed.kind, AgentRunActionKind::WorkspaceReviewFixer);
    assert_eq!(parsed.context_id, "conversation-1");
    assert_eq!(parsed.target_id, "attempt-1");
    assert_eq!(
        AgentRunActionKind::WorkspaceReviewFixer.to_string(),
        "workspace_review_fixer"
    );
    assert_eq!(
        "workspace_review_fixer"
            .parse::<AgentRunActionKind>()
            .unwrap(),
        AgentRunActionKind::WorkspaceReviewFixer
    );
}

#[test]
fn malformed_or_partial_action_metadata_is_not_authoritative() {
    for metadata in [
        None,
        Some("not-json"),
        Some(r#"{"ralphx_action_kind":"ordinary"}"#),
        Some(r#"{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"session-1"}"#),
        Some(
            r#"{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":" ","ralphx_action_target_id":"artifact-1"}"#,
        ),
        Some(
            r#"{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"session-1","ralphx_action_target_id":" "}"#,
        ),
    ] {
        let mut run = AgentRun::new(ChatConversationId::new());
        run.apply_action_metadata_json(metadata);
        assert_eq!(run.action_kind, None);
        assert_eq!(run.action_context_id, None);
        assert_eq!(run.action_target_id, None);
    }
}

#[test]
fn pr_autofix_action_metadata_roundtrips() {
    let parsed = AgentRunAction::from_metadata_json(Some(
        r#"{"ralphx_action_kind":"pr_autofix","ralphx_action_context_id":"42","ralphx_action_target_id":"github_pr_autofix:42:abc"}"#,
    ))
    .unwrap();

    assert_eq!(parsed.kind, AgentRunActionKind::PrAutofix);
    assert_eq!(parsed.context_id, "42");
    assert_eq!(parsed.target_id, "github_pr_autofix:42:abc");
    assert_eq!(AgentRunActionKind::PrAutofix.to_string(), "pr_autofix");
}

#[test]
fn test_new_continuation_run() {
    let conversation_id = ChatConversationId::new();
    let chain_id = "chain-123".to_string();
    let parent_id = "parent-456".to_string();
    let run = AgentRun::new_continuation(conversation_id, chain_id.clone(), parent_id.clone());

    assert_eq!(run.conversation_id, conversation_id);
    assert_eq!(run.status, AgentRunStatus::Running);
    assert_eq!(run.run_chain_id, Some(chain_id));
    assert_eq!(run.parent_run_id, Some(parent_id));
}

#[test]
fn test_agent_run_provider_metadata_serialization() {
    let conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(conversation_id);
    run.harness = Some(AgentHarnessKind::Codex);
    run.provider_session_id = Some("session-123".to_string());
    run.logical_model = Some("gpt-5.4".to_string());
    run.effective_model_id = Some("gpt-5.4".to_string());
    run.logical_effort = Some(LogicalEffort::XHigh);
    run.effective_effort = Some("high".to_string());
    run.service_tier = Some("fast".to_string());
    run.approval_policy = Some("on-request".to_string());
    run.sandbox_mode = Some("workspace-write".to_string());

    let serialized = serde_json::to_value(&run).expect("serialize agent run");
    assert_eq!(serialized["harness"], "codex");
    assert_eq!(serialized["provider_session_id"], "session-123");
    assert_eq!(serialized["logical_effort"], "xhigh");
    assert_eq!(serialized["service_tier"], "fast");
    assert_eq!(serialized["sandbox_mode"], "workspace-write");
}

#[test]
fn runtime_source_serializes_as_snake_case_and_ignores_unknown_values() {
    let mut run = AgentRun::new(ChatConversationId::new());
    run.runtime_source = Some(RuntimeSource::RoleDefault);

    let serialized = serde_json::to_value(&run).expect("serialize agent run");
    assert_eq!(serialized["runtime_source"], "role_default");

    let mut unknown = serialized;
    unknown["runtime_source"] = serde_json::Value::String("future_runtime_source".to_string());
    let hydrated: AgentRun =
        serde_json::from_value(unknown).expect("unknown source is legacy-safe");
    assert_eq!(hydrated.runtime_source, None);
}

#[test]
fn test_complete_agent_run() {
    let conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(conversation_id);

    run.complete();

    assert_eq!(run.status, AgentRunStatus::Completed);
    assert!(!run.is_active());
    assert!(run.is_terminal());
    assert!(run.completed_at.is_some());
    assert_eq!(run.error_message, None);
}

#[test]
fn test_fail_agent_run() {
    let conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(conversation_id);

    run.fail("Connection timeout");

    assert_eq!(run.status, AgentRunStatus::Failed);
    assert!(!run.is_active());
    assert!(run.is_terminal());
    assert!(run.completed_at.is_some());
    assert_eq!(run.error_message, Some("Connection timeout".to_string()));
}

#[test]
fn test_cancel_agent_run() {
    let conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(conversation_id);

    run.cancel();

    assert_eq!(run.status, AgentRunStatus::Cancelled);
    assert!(!run.is_active());
    assert!(run.is_terminal());
    assert!(run.completed_at.is_some());
    assert_eq!(run.error_message, None);
}

#[test]
fn test_duration() {
    let conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(conversation_id);

    // Duration is None when running
    assert_eq!(run.duration(), None);

    run.complete();

    // Duration is available after completion
    let duration = run.duration().expect("duration should be available");
    assert!(duration.num_milliseconds() >= 0);
}
