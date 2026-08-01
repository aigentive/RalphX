use chrono::Duration;

use crate::domain::entities::{AgentRunId, ChatConversationId, DelegationWakePolicy};
use crate::http_server::delegation::DelegationService;
use crate::infrastructure::agents::claude::delegation_config;

use super::delegation_park_test_support::{harness, park};
use super::ArmParkRequest;

#[tokio::test]
async fn arm_rejects_a_job_owned_by_a_different_parent() {
    let harness = harness();
    let service = harness.service();
    let delegation = DelegationService::new();
    let expected_conversation = ChatConversationId::new();
    let expected_run = AgentRunId::new();
    delegation
        .register_running(
            "job".to_string(),
            "project".to_string(),
            "x".to_string(),
            None,
            None,
            Some(ChatConversationId::new().as_str()),
            Some(AgentRunId::new().as_str()),
            None,
            "session".to_string(),
            None,
            Some(AgentRunId::new().as_str()),
            "delegate".to_string(),
            None,
            "codex",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

    let error = service
        .arm(
            ArmParkRequest {
                parent_conversation_id: expected_conversation,
                parent_agent_run_id: expected_run,
                job_ids: vec!["job".to_string()],
                wake_policy: DelegationWakePolicy::AllSettled,
                wake_on_failure: false,
                max_wait_secs: None,
            },
            &delegation,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::error::AppError::Validation(_)));
}

#[tokio::test]
async fn arm_rejects_a_job_without_a_durable_run() {
    let harness = harness();
    let service = harness.service();
    let delegation = DelegationService::new();
    let conversation = ChatConversationId::new();
    let run = AgentRunId::new();
    delegation
        .register_running(
            "job".to_string(),
            "project".to_string(),
            "x".to_string(),
            None,
            None,
            Some(conversation.as_str()),
            Some(run.as_str()),
            None,
            "session".to_string(),
            None,
            None,
            "delegate".to_string(),
            None,
            "codex",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

    let error = service
        .arm(
            ArmParkRequest {
                parent_conversation_id: conversation,
                parent_agent_run_id: run,
                job_ids: vec!["job".to_string()],
                wake_policy: DelegationWakePolicy::AllSettled,
                wake_on_failure: false,
                max_wait_secs: None,
            },
            &delegation,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::error::AppError::Validation(_)));
}

#[tokio::test]
async fn arm_clamps_wait_and_supersedes_an_armed_park() {
    let harness = harness();
    let service = harness.service();
    let delegation = DelegationService::new();
    let conversation = ChatConversationId::new();
    let parent = AgentRunId::new();
    let delegated = AgentRunId::new();
    delegation
        .register_running(
            "job".to_string(),
            "project".to_string(),
            "x".to_string(),
            None,
            None,
            Some(conversation.as_str()),
            Some(parent.as_str()),
            None,
            "session".to_string(),
            None,
            Some(delegated.as_str()),
            "delegate".to_string(),
            None,
            "codex",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
    harness
        .parks
        .insert(park(conversation.clone(), parent, delegated))
        .await;

    let armed = service
        .arm(
            ArmParkRequest {
                parent_conversation_id: conversation,
                parent_agent_run_id: parent,
                job_ids: vec!["job".to_string()],
                wake_policy: DelegationWakePolicy::AllSettled,
                wake_on_failure: false,
                max_wait_secs: Some(u64::MAX),
            },
            &delegation,
        )
        .await
        .unwrap();
    assert_eq!(
        armed.deadline_at - armed.created_at,
        Duration::seconds(delegation_config().park_max_secs as i64)
    );
    assert_eq!(*harness.parks.supersede_count.lock().await, 1);
    assert_eq!(
        harness.parks.parks.lock().await[0].state,
        crate::domain::entities::DelegationParkState::Superseded
    );
}
