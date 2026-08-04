use async_trait::async_trait;
use chrono::Duration;

use crate::domain::entities::{
    AgentRun, AgentRunId, ChatConversationId, DelegationParkJob, DelegationParkState,
    DelegationWakePolicy,
};
use crate::domain::repositories::{AgentRunRepository, DelegationParkRepository};
use crate::infrastructure::agents::claude::delegation_config;

use super::delegation_park_test_support::{
    harness, insert_parent_and_delegate, park, FakeDelegationJobs,
};
use super::{ArmParkRequest, DelegationJobSource, ParkJobSnapshot, MAX_PARK_JOB_IDS};

struct StaticDelegationJobSource(ParkJobSnapshot);

#[async_trait]
impl DelegationJobSource for StaticDelegationJobSource {
    async fn park_job_snapshot(&self, _job_id: &str) -> Option<ParkJobSnapshot> {
        Some(self.0.clone())
    }
}

fn request(
    parent_conversation_id: ChatConversationId,
    parent_agent_run_id: AgentRunId,
    job_ids: Vec<String>,
) -> ArmParkRequest {
    ArmParkRequest {
        parent_conversation_id,
        parent_agent_run_id,
        job_ids,
        wake_policy: DelegationWakePolicy::AllSettled,
        wake_on_failure: false,
        max_wait_secs: None,
    }
}

#[tokio::test]
async fn arm_rejects_an_empty_job_set_before_repository_work() {
    let harness = harness();
    let error = harness
        .service()
        .arm(
            request(ChatConversationId::new(), AgentRunId::new(), Vec::new()),
            &StaticDelegationJobSource(ParkJobSnapshot {
                status: "running".to_string(),
                parent_conversation_id: None,
                parent_agent_run_id: None,
                delegated_session_id: "unused".to_string(),
                delegated_agent_run_id: None,
            }),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, crate::error::AppError::Validation(_)));
    assert_eq!(*harness.parks.supersede_count.lock().await, 0);
}

#[tokio::test]
async fn arm_rejects_non_running_and_invalid_durable_runs() {
    let harness = harness();
    let conversation = ChatConversationId::new();
    let parent = AgentRunId::new();
    let non_running = StaticDelegationJobSource(ParkJobSnapshot {
        status: "completed".to_string(),
        parent_conversation_id: Some(conversation.as_str()),
        parent_agent_run_id: Some(parent.as_str()),
        delegated_session_id: "delegate".to_string(),
        delegated_agent_run_id: Some(AgentRunId::new().as_str()),
    });
    let error = harness
        .service()
        .arm(
            request(conversation.clone(), parent, vec!["job".to_string()]),
            &non_running,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::error::AppError::Validation(_)));

    let invalid_run = StaticDelegationJobSource(ParkJobSnapshot {
        status: "running".to_string(),
        parent_conversation_id: Some(conversation.as_str()),
        parent_agent_run_id: Some(parent.as_str()),
        delegated_session_id: "delegate".to_string(),
        delegated_agent_run_id: Some("not-a-run-id".to_string()),
    });
    let error = harness
        .service()
        .arm(
            request(conversation, parent, vec!["job".to_string()]),
            &invalid_run,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::error::AppError::Validation(_)));
    assert_eq!(*harness.parks.supersede_count.lock().await, 0);
}

#[tokio::test]
async fn arm_rejects_more_job_ids_than_the_park_bound() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let run = AgentRunId::new();
    let delegation =
        FakeDelegationJobs::running("job", &conversation, &run, Some(&AgentRunId::new()));

    let error = service
        .arm(
            ArmParkRequest {
                parent_conversation_id: conversation,
                parent_agent_run_id: run,
                job_ids: vec!["job".to_string(); MAX_PARK_JOB_IDS + 1],
                wake_policy: DelegationWakePolicy::AllSettled,
                wake_on_failure: false,
                max_wait_secs: None,
            },
            &delegation,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::error::AppError::Validation(_)));
    assert_eq!(*harness.parks.supersede_count.lock().await, 0);
}

#[tokio::test]
async fn arm_rejects_a_job_owned_by_a_different_parent() {
    let harness = harness();
    let service = harness.service();
    let expected_conversation = ChatConversationId::new();
    let expected_run = AgentRunId::new();
    let delegation = FakeDelegationJobs::running(
        "job",
        &ChatConversationId::new(),
        &AgentRunId::new(),
        Some(&AgentRunId::new()),
    );

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
    let conversation = ChatConversationId::new();
    let run = AgentRunId::new();
    let delegation = FakeDelegationJobs::running("job", &conversation, &run, None);

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
async fn arm_rejects_an_unknown_job() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let run = AgentRunId::new();
    let delegation =
        FakeDelegationJobs::running("job", &conversation, &run, Some(&AgentRunId::new()));

    let error = service
        .arm(
            ArmParkRequest {
                parent_conversation_id: conversation,
                parent_agent_run_id: run,
                job_ids: vec!["other-job".to_string()],
                wake_policy: DelegationWakePolicy::AllSettled,
                wake_on_failure: false,
                max_wait_secs: None,
            },
            &delegation,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::error::AppError::NotFound(_)));
}

#[tokio::test]
async fn arm_clamps_wait_and_supersedes_an_armed_park() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let caller_run = AgentRun::new(conversation.clone());
    let parent = caller_run.id;
    harness.runs.create(caller_run).await.unwrap();
    let delegated = AgentRunId::new();
    let delegation = FakeDelegationJobs::running("job", &conversation, &parent, Some(&delegated));
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

#[tokio::test]
async fn arm_accepts_a_job_whose_lineage_anchor_is_not_the_calling_conversation() {
    // Nested delegates and ideation verification children register jobs whose
    // `parent_conversation_id` is an ancestor conversation, not the runtime that called
    // `delegate_start`. The caller run still belongs to the calling conversation, so the park
    // must arm.
    let harness = harness();
    let service = harness.service();
    let caller_conversation = ChatConversationId::new();
    let caller_run = AgentRun::new(caller_conversation.clone());
    let caller_run_id = caller_run.id;
    harness.runs.create(caller_run).await.unwrap();
    let lineage_anchor_conversation = ChatConversationId::new();
    let delegation = FakeDelegationJobs::running(
        "job",
        &lineage_anchor_conversation,
        &caller_run_id,
        Some(&AgentRunId::new()),
    );

    let armed = service
        .arm(
            request(
                caller_conversation.clone(),
                caller_run_id,
                vec!["job".to_string()],
            ),
            &delegation,
        )
        .await
        .expect("a caller-run-owned job must arm even when the job's lineage anchor differs");
    assert_eq!(armed.parent_conversation_id, caller_conversation);
    assert_eq!(armed.state, DelegationParkState::Armed);
}

#[tokio::test]
async fn arm_rejects_a_caller_run_from_another_conversation() {
    let harness = harness();
    let service = harness.service();
    let caller_run = AgentRun::new(ChatConversationId::new());
    let caller_run_id = caller_run.id;
    harness.runs.create(caller_run).await.unwrap();
    let claimed_conversation = ChatConversationId::new();
    let delegation = FakeDelegationJobs::running(
        "job",
        &claimed_conversation,
        &caller_run_id,
        Some(&AgentRunId::new()),
    );

    let error = service
        .arm(
            request(claimed_conversation, caller_run_id, vec!["job".to_string()]),
            &delegation,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::error::AppError::Validation(_)));
    assert_eq!(*harness.parks.supersede_count.lock().await, 0);
}

#[tokio::test]
async fn arm_rejects_a_caller_run_that_does_not_exist() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let missing_run = AgentRunId::new();
    let delegation =
        FakeDelegationJobs::running("job", &conversation, &missing_run, Some(&AgentRunId::new()));

    let error = service
        .arm(
            request(conversation, missing_run, vec!["job".to_string()]),
            &delegation,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, crate::error::AppError::Validation(_)));
    assert_eq!(*harness.parks.supersede_count.lock().await, 0);
}

#[tokio::test]
async fn settled_job_wakes_the_park_through_the_service_entry_point() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    let mut armed = park(conversation, parent.id, delegate.id);
    armed.jobs[0].settled_status = None;
    harness.parks.insert(armed.clone()).await;

    harness
        .service()
        .note_job_settled(&delegate.id, "completed")
        .await
        .unwrap();

    assert_eq!(
        harness.parks.get(&armed.id).await.unwrap().unwrap().state,
        DelegationParkState::Woken
    );
    assert_eq!(harness.chat.get_sent_messages().await.len(), 1);
}

#[tokio::test]
async fn partial_settlement_keeps_an_all_settled_park_armed() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    let mut armed = park(conversation, parent.id, delegate.id);
    armed.jobs[0].settled_status = None;
    armed.jobs.push(DelegationParkJob {
        job_id: "other-job".to_string(),
        delegated_session_id: "other-session".to_string(),
        delegated_agent_run_id: AgentRunId::new(),
        settled_status: None,
    });
    harness.parks.insert(armed.clone()).await;

    harness
        .service()
        .note_job_settled(&delegate.id, "completed")
        .await
        .unwrap();

    let updated = harness.parks.get(&armed.id).await.unwrap().unwrap();
    assert_eq!(updated.state, DelegationParkState::Armed);
    assert_eq!(updated.jobs[0].settled_status.as_deref(), Some("completed"));
    assert!(updated.jobs[1].settled_status.is_none());
    assert!(harness.chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn terminal_parent_disarm_prevents_a_later_delegate_completion_from_waking_it() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    let mut armed = park(conversation.clone(), parent.id, delegate.id);
    armed.jobs[0].settled_status = None;
    harness.parks.insert(armed.clone()).await;

    assert_eq!(
        super::DelegationParkService::disarm_armed_for_terminal_parent(
            harness.parks.as_ref(),
            &conversation,
            &parent.id,
        )
        .await
        .unwrap(),
        1
    );

    harness
        .service()
        .note_job_settled(&delegate.id, "completed")
        .await
        .unwrap();

    assert_eq!(
        harness.parks.get(&armed.id).await.unwrap().unwrap().state,
        DelegationParkState::Disarmed
    );
    assert!(
        harness.chat.get_sent_messages().await.is_empty(),
        "a disarmed parent must not receive resume_in_place after its delegate settles"
    );
    assert_eq!(
        harness
            .runs
            .get_by_id(&delegate.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        crate::domain::entities::AgentRunStatus::Completed,
        "disarming the parent park must not alter the delegate's terminal run"
    );
}
