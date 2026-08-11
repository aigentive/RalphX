use super::scripted_agent_workflow::*;
use super::{ChatConversationId, ProjectId};

fn meta() -> AgentWorkflowMeta {
    AgentWorkflowMeta {
        name: "Review workflow".to_string(),
        description: "Review in parallel".to_string(),
        phases: vec!["research".to_string(), "synthesize".to_string()],
        max_concurrency: 4,
        max_invocations: 20,
    }
}

#[test]
fn approval_is_bound_to_both_script_and_permission_hashes() {
    let mut script = AgentWorkflowScript::new(
        ChatConversationId::from_string("conversation-1"),
        ProjectId::from_string("project-1".to_string()),
        "return await agent('review');".to_string(),
        meta(),
        r#"{"filesystem":"read-only"}"#.to_string(),
        2,
    )
    .expect("valid script");

    script.approved_script_hash = Some(script.script_hash.clone());
    script.approved_permission_hash = Some(script.permission_hash.clone());
    script.approved_at = Some(chrono::Utc::now());
    assert!(script.is_approved_for_current_content());

    script.permission_hash = sha256_hex(b"changed");
    assert!(!script.is_approved_for_current_content());
}

#[test]
fn workflow_limits_fail_closed_above_hard_ceiling() {
    let mut invalid = meta();
    invalid.max_concurrency = 17;
    assert!(invalid.validate().unwrap_err().contains("between 1 and 16"));

    invalid = meta();
    invalid.max_invocations = 1_001;
    assert!(invalid
        .validate()
        .unwrap_err()
        .contains("between 1 and 1000"));
}

#[test]
fn workflow_statuses_round_trip_and_terminal_classification_is_explicit() {
    for status in [
        AgentWorkflowRunStatus::AwaitingApproval,
        AgentWorkflowRunStatus::Running,
        AgentWorkflowRunStatus::Paused,
        AgentWorkflowRunStatus::Completed,
        AgentWorkflowRunStatus::Failed,
        AgentWorkflowRunStatus::Cancelled,
    ] {
        let encoded = status.to_string();
        assert_eq!(encoded.parse::<AgentWorkflowRunStatus>().unwrap(), status);
    }
    assert!(AgentWorkflowRunStatus::Completed.is_terminal());
    assert!(!AgentWorkflowRunStatus::Disabled.is_terminal());
}

#[test]
fn workflow_defaults_and_invalid_values_are_explicit() {
    let generated_id = AgentWorkflowRunId::default();
    assert!(!generated_id.as_str().is_empty());
    assert_eq!(generated_id.to_string(), generated_id.as_str());

    let parse_error = "unknown"
        .parse::<AgentWorkflowRunStatus>()
        .expect_err("unknown status must fail");
    assert_eq!(parse_error, "Invalid AgentWorkflowRunStatus: unknown");

    let decoded: AgentWorkflowMeta =
        serde_json::from_str(r#"{"name":"Defaults"}"#).expect("deserialize default limits");
    assert_eq!(decoded.max_concurrency, 4);
    assert_eq!(decoded.max_invocations, 64);

    let mut missing_name = meta();
    missing_name.name = "  ".to_string();
    assert_eq!(
        missing_name.validate(),
        Err("Workflow name is required".to_string())
    );

    let empty_source = AgentWorkflowScript::new(
        ChatConversationId::from_string("conversation-1"),
        ProjectId::from_string("project-1".to_string()),
        "  ".to_string(),
        meta(),
        "{}".to_string(),
        0,
    );
    assert_eq!(
        empty_source.expect_err("empty source must fail"),
        "Workflow script source is required"
    );
}
