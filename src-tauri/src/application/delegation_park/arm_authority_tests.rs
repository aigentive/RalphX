use std::collections::HashMap;

use async_trait::async_trait;

use crate::domain::entities::{
    AgentRun, AgentRunActionKind, AgentRunId, ChatConversationId, DelegationParkId,
    DelegationParkState, DelegationWakePolicy,
};
use crate::domain::repositories::{AgentRunRepository, DelegationParkRepository};

use super::delegation_park_test_support::{harness, park, FakeDelegationJobs};
use super::{ArmParkRequest, DelegationJobSource, ParkJobSnapshot};

struct MappedDelegationJobSource(HashMap<String, ParkJobSnapshot>);

#[async_trait]
impl DelegationJobSource for MappedDelegationJobSource {
    async fn park_job_snapshot(&self, job_id: &str) -> Option<ParkJobSnapshot> {
        self.0.get(job_id).cloned()
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
async fn insert_resumed_run(
    harness: &super::delegation_park_test_support::Harness,
    conversation_id: &ChatConversationId,
    park_id: &DelegationParkId,
) -> AgentRun {
    let mut run = AgentRun::new(*conversation_id);
    run.action_kind = Some(AgentRunActionKind::DelegationParkWake);
    run.action_context_id = Some(conversation_id.as_str());
    run.action_target_id = Some(park_id.as_str());
    harness.runs.create(run).await.unwrap()
}
#[tokio::test]
async fn resumed_run_reparks_the_exact_job_from_an_expired_park() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let original_run = AgentRunId::new();
    let delegated_run = AgentRunId::new();
    let mut prior = park(conversation, original_run, delegated_run);
    prior.state = DelegationParkState::Expired;
    harness.parks.insert(prior.clone()).await;
    let resumed = insert_resumed_run(&harness, &conversation, &prior.id).await;
    let delegation =
        FakeDelegationJobs::running("job", &conversation, &original_run, Some(&delegated_run));

    let armed = service
        .arm(
            request(conversation, resumed.id, vec!["job".to_string()]),
            &delegation,
        )
        .await
        .expect("exact backend-resumed ownership must re-arm");

    assert_ne!(armed.id, prior.id);
    assert_eq!(armed.parent_agent_run_id, resumed.id);
    assert_eq!(armed.state, DelegationParkState::Armed);
    assert_eq!(armed.jobs[0].delegated_agent_run_id, delegated_run);
}

#[tokio::test]
async fn resumed_run_reparks_the_exact_job_from_a_woken_park() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let original_run = AgentRunId::new();
    let delegated_run = AgentRunId::new();
    let mut prior = park(conversation, original_run, delegated_run);
    prior.state = DelegationParkState::Woken;
    harness.parks.insert(prior.clone()).await;
    let resumed = insert_resumed_run(&harness, &conversation, &prior.id).await;
    let delegation =
        FakeDelegationJobs::running("job", &conversation, &original_run, Some(&delegated_run));

    let armed = service
        .arm(
            request(conversation, resumed.id, vec!["job".to_string()]),
            &delegation,
        )
        .await
        .expect("exact backend-resumed ownership from a woken park must re-arm");

    assert_ne!(armed.id, prior.id);
    assert_eq!(armed.parent_agent_run_id, resumed.id);
    assert_eq!(armed.state, DelegationParkState::Armed);
    assert_eq!(armed.jobs[0].delegated_agent_run_id, delegated_run);
}

#[tokio::test]
async fn resumed_run_reparks_a_waking_park_and_late_settlement_is_ignored() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let original_run = AgentRunId::new();
    let delegated_run = AgentRunId::new();
    let mut prior = park(conversation, original_run, delegated_run);
    prior.state = DelegationParkState::Waking;
    harness.parks.insert(prior.clone()).await;
    let resumed = insert_resumed_run(&harness, &conversation, &prior.id).await;
    let delegation =
        FakeDelegationJobs::running("job", &conversation, &original_run, Some(&delegated_run));

    let armed = service
        .arm(
            request(conversation, resumed.id, vec!["job".to_string()]),
            &delegation,
        )
        .await
        .expect("the exact waking park must transfer authority");
    harness
        .parks
        .settle(&prior.id, DelegationParkState::Woken, None)
        .await
        .unwrap();

    assert_eq!(
        harness.parks.get(&prior.id).await.unwrap().unwrap().state,
        DelegationParkState::Superseded
    );
    assert_eq!(
        harness.parks.get(&armed.id).await.unwrap().unwrap().state,
        DelegationParkState::Armed
    );
}

#[tokio::test]
async fn ordinary_later_run_cannot_inherit_an_expired_park() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let original_run = AgentRunId::new();
    let delegated_run = AgentRunId::new();
    let mut prior = park(conversation, original_run, delegated_run);
    prior.state = DelegationParkState::Expired;
    harness.parks.insert(prior.clone()).await;
    let ordinary = harness
        .runs
        .create(AgentRun::new(conversation))
        .await
        .unwrap();
    let delegation =
        FakeDelegationJobs::running("job", &conversation, &original_run, Some(&delegated_run));

    let error = service
        .arm(
            request(conversation, ordinary.id, vec!["job".to_string()]),
            &delegation,
        )
        .await
        .unwrap_err();

    assert!(matches!(error, crate::error::AppError::Validation(_)));
    assert_eq!(*harness.parks.supersede_count.lock().await, 0);
    assert_eq!(harness.parks.parks.lock().await.len(), 1);
}

#[tokio::test]
async fn inherited_job_identity_mismatch_rejects_before_mutation() {
    enum Mismatch {
        Job,
        Session,
        DelegatedRun,
    }

    for mismatch in [Mismatch::Job, Mismatch::Session, Mismatch::DelegatedRun] {
        let harness = harness();
        let service = harness.service();
        let conversation = ChatConversationId::new();
        let original_run = AgentRunId::new();
        let delegated_run = AgentRunId::new();
        let mut prior = park(conversation, original_run, delegated_run);
        prior.state = DelegationParkState::Woken;
        match mismatch {
            Mismatch::Job => prior.jobs[0].job_id = "different-job".to_string(),
            Mismatch::Session => {
                prior.jobs[0].delegated_session_id = "different-session".to_string()
            }
            Mismatch::DelegatedRun => prior.jobs[0].delegated_agent_run_id = AgentRunId::new(),
        }
        harness.parks.insert(prior.clone()).await;
        let resumed = insert_resumed_run(&harness, &conversation, &prior.id).await;
        let delegation =
            FakeDelegationJobs::running("job", &conversation, &original_run, Some(&delegated_run));

        let error = service
            .arm(
                request(conversation, resumed.id, vec!["job".to_string()]),
                &delegation,
            )
            .await
            .unwrap_err();

        assert!(matches!(error, crate::error::AppError::Validation(_)));
        assert_eq!(*harness.parks.supersede_count.lock().await, 0);
        assert_eq!(harness.parks.parks.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn invalid_wake_action_metadata_rejects_before_mutation() {
    enum InvalidField {
        Kind,
        Context,
        Target,
    }

    for invalid_field in [
        InvalidField::Kind,
        InvalidField::Context,
        InvalidField::Target,
    ] {
        let harness = harness();
        let service = harness.service();
        let conversation = ChatConversationId::new();
        let original_run = AgentRunId::new();
        let delegated_run = AgentRunId::new();
        let mut prior = park(conversation, original_run, delegated_run);
        prior.state = DelegationParkState::Woken;
        harness.parks.insert(prior.clone()).await;
        let mut resumed = AgentRun::new(conversation);
        resumed.action_kind = Some(AgentRunActionKind::DelegationParkWake);
        resumed.action_context_id = Some(conversation.as_str());
        resumed.action_target_id = Some(prior.id.as_str());
        match invalid_field {
            InvalidField::Kind => resumed.action_kind = Some(AgentRunActionKind::VerifyPlan),
            InvalidField::Context => {
                resumed.action_context_id = Some(ChatConversationId::new().as_str())
            }
            InvalidField::Target => resumed.action_target_id = Some("not-a-park-id".to_string()),
        }
        let resumed = harness.runs.create(resumed).await.unwrap();
        let delegation =
            FakeDelegationJobs::running("job", &conversation, &original_run, Some(&delegated_run));

        let error = service
            .arm(
                request(conversation, resumed.id, vec!["job".to_string()]),
                &delegation,
            )
            .await
            .unwrap_err();

        assert!(matches!(error, crate::error::AppError::Validation(_)));
        assert_eq!(*harness.parks.supersede_count.lock().await, 0);
        assert_eq!(harness.parks.parks.lock().await.len(), 1);
    }
}

#[tokio::test]
async fn inherited_authority_read_failure_rejects_before_mutation() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let original_run = AgentRunId::new();
    let delegated_run = AgentRunId::new();
    let mut prior = park(conversation, original_run, delegated_run);
    prior.state = DelegationParkState::Woken;
    harness.parks.insert(prior.clone()).await;
    let resumed = insert_resumed_run(&harness, &conversation, &prior.id).await;
    let delegation =
        FakeDelegationJobs::running("job", &conversation, &original_run, Some(&delegated_run));
    harness
        .parks
        .fail_get
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let error = service
        .arm(
            request(conversation, resumed.id, vec!["job".to_string()]),
            &delegation,
        )
        .await
        .unwrap_err();

    assert!(matches!(error, crate::error::AppError::Infrastructure(_)));
    assert_eq!(*harness.parks.supersede_count.lock().await, 0);
    assert_eq!(harness.parks.parks.lock().await.len(), 1);
}

#[tokio::test]
async fn mixed_direct_and_inherited_jobs_arm_after_independent_authorization() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let original_run = AgentRunId::new();
    let inherited_delegate = AgentRunId::new();
    let mut prior = park(conversation, original_run, inherited_delegate);
    prior.state = DelegationParkState::Woken;
    prior.jobs[0].job_id = "inherited".to_string();
    harness.parks.insert(prior.clone()).await;
    let resumed = insert_resumed_run(&harness, &conversation, &prior.id).await;
    let direct_delegate = AgentRunId::new();
    let source = MappedDelegationJobSource(HashMap::from([
        (
            "inherited".to_string(),
            ParkJobSnapshot {
                status: "running".to_string(),
                parent_conversation_id: Some(conversation.as_str()),
                parent_agent_run_id: Some(original_run.as_str()),
                delegated_session_id: "session".to_string(),
                delegated_agent_run_id: Some(inherited_delegate.as_str()),
            },
        ),
        (
            "direct".to_string(),
            ParkJobSnapshot {
                status: "running".to_string(),
                parent_conversation_id: Some(conversation.as_str()),
                parent_agent_run_id: Some(resumed.id.as_str()),
                delegated_session_id: "direct-session".to_string(),
                delegated_agent_run_id: Some(direct_delegate.as_str()),
            },
        ),
    ]));

    let armed = service
        .arm(
            request(
                conversation,
                resumed.id,
                vec!["direct".to_string(), "inherited".to_string()],
            ),
            &source,
        )
        .await
        .expect("each mixed-wave job has independent direct or inherited authority");

    assert_eq!(armed.parent_agent_run_id, resumed.id);
    assert_eq!(armed.jobs.len(), 2);
    assert_eq!(armed.jobs[0].delegated_agent_run_id, direct_delegate);
    assert_eq!(armed.jobs[1].delegated_agent_run_id, inherited_delegate);
}

#[tokio::test]
async fn superseded_park_cannot_authorize_inherited_jobs() {
    let harness = harness();
    let service = harness.service();
    let conversation = ChatConversationId::new();
    let original_run = AgentRunId::new();
    let delegated_run = AgentRunId::new();
    let mut prior = park(conversation, original_run, delegated_run);
    prior.state = DelegationParkState::Superseded;
    harness.parks.insert(prior.clone()).await;
    let resumed = insert_resumed_run(&harness, &conversation, &prior.id).await;
    let delegation =
        FakeDelegationJobs::running("job", &conversation, &original_run, Some(&delegated_run));

    let error = service
        .arm(
            request(conversation, resumed.id, vec!["job".to_string()]),
            &delegation,
        )
        .await
        .unwrap_err();

    assert!(matches!(error, crate::error::AppError::Validation(_)));
    assert_eq!(*harness.parks.supersede_count.lock().await, 0);
    assert_eq!(harness.parks.parks.lock().await.len(), 1);
}
