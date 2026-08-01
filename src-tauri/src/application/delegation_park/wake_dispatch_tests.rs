use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatConversationId, DelegationParkState,
    DelegationWakeReason,
};
use crate::domain::repositories::{AgentRunRepository, DelegationParkRepository};
use crate::infrastructure::agents::claude::delegation_config;
use chrono::{Duration, Utc};

use super::delegation_park_test_support::{harness, insert_parent_and_delegate, park};

#[tokio::test]
async fn dispatch_wake_skips_when_claim_is_lost() {
    let harness = harness();
    harness
        .parks
        .claim_result
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let service = harness.service();
    let park = park(
        ChatConversationId::new(),
        AgentRunId::new(),
        AgentRunId::new(),
    );
    service
        .dispatch_wake(&park, DelegationWakeReason::AllSettled)
        .await
        .unwrap();
    assert!(harness.chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn dispatch_wake_supersedes_when_another_run_is_active() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    let mut newer = AgentRun::new(conversation.clone());
    newer.status = AgentRunStatus::Running;
    harness.runs.create(newer).await.unwrap();
    let park = park(conversation, parent.id, delegate.id);
    harness.parks.insert(park.clone()).await;

    harness
        .service()
        .dispatch_wake(&park, DelegationWakeReason::AllSettled)
        .await
        .unwrap();
    assert!(harness.chat.get_sent_messages().await.is_empty());
    assert_eq!(
        harness.parks.settled.lock().await.as_slice(),
        &[DelegationParkState::Superseded]
    );
}

#[tokio::test]
async fn wake_uses_hidden_resume_metadata_and_parent_runtime_overrides() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    let park = park(conversation, parent.id, delegate.id);
    harness.parks.insert(park.clone()).await;

    harness
        .service()
        .dispatch_wake(&park, DelegationWakeReason::AllSettled)
        .await
        .unwrap();
    let options = harness.chat.get_sent_options().await;
    let options = options.first().unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(options.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["hidden_from_ui"], true);
    assert_eq!(metadata["resume_in_place"], true);
    assert_eq!(metadata["persist_hidden_marker"], true);
    assert_eq!(options.harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(options.model_override.as_deref(), Some("gpt-5.6"));
    assert_eq!(options.logical_effort_override, Some(LogicalEffort::High));
    assert_eq!(options.service_tier_override.as_deref(), Some("priority"));
}

#[tokio::test(start_paused = true)]
async fn repeated_enqueue_failures_settle_failed_and_emit_attention() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    harness.chat.set_available(false).await;
    let park = park(conversation, parent.id, delegate.id);
    harness.parks.insert(park.clone()).await;

    harness
        .service()
        .dispatch_wake(&park, DelegationWakeReason::AllSettled)
        .await
        .unwrap();
    assert_eq!(
        harness.parks.settled.lock().await.as_slice(),
        &[DelegationParkState::Failed]
    );
    assert_eq!(
        harness.chat.call_count(),
        delegation_config().park_wake_retry_max.max(1)
    );
    assert_eq!(harness.events.events().len(), 1);
}

#[tokio::test]
async fn wake_retry_budget_uses_the_existing_durable_attempt_count() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    harness.chat.set_available(false).await;
    let mut park = park(conversation, parent.id, delegate.id);
    park.wake_attempts = i32::try_from(delegation_config().park_wake_retry_max).unwrap();
    harness.parks.insert(park.clone()).await;

    harness
        .service()
        .dispatch_wake(&park, DelegationWakeReason::AllSettled)
        .await
        .unwrap();

    assert_eq!(harness.chat.call_count(), 1);
    assert_eq!(
        harness.parks.settled.lock().await.as_slice(),
        &[DelegationParkState::Failed]
    );
    assert_eq!(
        harness
            .parks
            .get(&park.id)
            .await
            .unwrap()
            .unwrap()
            .wake_attempts,
        park.wake_attempts + 1
    );
}

#[tokio::test]
async fn deadline_reconciliation_sends_an_explicit_timeout_notice() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    let mut park = park(conversation, parent.id, delegate.id);
    park.deadline_at = Utc::now() - Duration::seconds(1);
    park.jobs[0].settled_status = None;
    harness.parks.insert(park).await;

    harness
        .service()
        .reconcile_all(harness.runs.as_ref())
        .await
        .unwrap();
    assert!(harness.chat.get_sent_messages().await[0].contains("Timeout notice"));
    assert_eq!(
        harness.parks.settled.lock().await.as_slice(),
        &[DelegationParkState::Expired]
    );
}

#[tokio::test]
async fn reconciliation_recovers_a_stale_waking_park() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    let config = delegation_config();
    let retry_backoff_secs = i64::try_from(config.park_wake_retry_backoff_secs).unwrap();
    let stale_threshold = Duration::seconds(
        i64::from(config.park_wake_retry_max)
            .saturating_mul(retry_backoff_secs)
            .saturating_add(retry_backoff_secs),
    );
    let mut park = park(conversation, parent.id, delegate.id);
    park.state = DelegationParkState::Waking;
    park.updated_at = Utc::now() - stale_threshold - stale_threshold;
    harness.parks.insert(park.clone()).await;

    harness
        .service()
        .reconcile_all(harness.runs.as_ref())
        .await
        .unwrap();

    assert_eq!(
        harness.parks.get(&park.id).await.unwrap().unwrap().state,
        DelegationParkState::Woken
    );
    assert!(!harness.chat.get_sent_messages().await.is_empty());
}
