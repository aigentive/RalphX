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
async fn deadline_reconciliation_omits_the_timeout_notice_for_a_job_it_just_settled() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    // The delegate is already terminal, so this pass settles the job and then finds the park
    // expired. Nothing timed out, so the wake must not say anything did.
    let mut park = park(conversation, parent.id, delegate.id);
    park.deadline_at = Utc::now() - Duration::seconds(1);
    park.jobs[0].settled_status = None;
    harness.parks.insert(park).await;

    harness
        .service()
        .reconcile_all(harness.runs.as_ref())
        .await
        .unwrap();
    let message = &harness.chat.get_sent_messages().await[0];
    assert!(!message.contains("Timeout notice"));
    assert!(message.contains("researcher: completed"));
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

#[tokio::test]
async fn missing_parent_conversation_fails_the_wake_and_emits_attention() {
    let harness = harness();
    let park = park(
        ChatConversationId::new(),
        AgentRunId::new(),
        AgentRunId::new(),
    );
    harness.parks.insert(park.clone()).await;

    let error = harness
        .service()
        .dispatch_wake(&park, DelegationWakeReason::AllSettled)
        .await
        .unwrap_err();

    assert!(matches!(error, crate::error::AppError::NotFound(_)));
    assert_eq!(
        harness.parks.get(&park.id).await.unwrap().unwrap().state,
        DelegationParkState::Failed
    );
    assert_eq!(harness.events.events().len(), 1);
    assert!(harness.chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn missing_delegate_run_fails_closed_before_wake_delivery() {
    let harness = harness();
    let (conversation, parent, _) = insert_parent_and_delegate(&harness).await;
    let park = park(conversation, parent.id, AgentRunId::new());
    harness.parks.insert(park.clone()).await;

    let error = harness
        .service()
        .dispatch_wake(&park, DelegationWakeReason::AllSettled)
        .await
        .unwrap_err();

    assert!(matches!(error, crate::error::AppError::NotFound(_)));
    assert_eq!(
        harness.parks.get(&park.id).await.unwrap().unwrap().state,
        DelegationParkState::Failed
    );
    assert_eq!(harness.events.events().len(), 1);
    assert!(harness.chat.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn deadline_wake_names_delegates_that_are_still_running() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    harness
        .runs
        .update_status(&delegate.id, AgentRunStatus::Running)
        .await
        .unwrap();
    let mut park = park(conversation, parent.id, delegate.id);
    park.deadline_at = Utc::now() - Duration::seconds(1);
    park.jobs[0].settled_status = None;
    harness.parks.insert(park).await;

    harness
        .service()
        .reconcile_all(harness.runs.as_ref())
        .await
        .unwrap();

    let message = &harness.chat.get_sent_messages().await[0];
    assert!(message.contains("Timeout notice: these jobs never settled: researcher."));
    assert_eq!(
        harness.parks.settled.lock().await.as_slice(),
        &[DelegationParkState::Expired]
    );
}

#[tokio::test(start_paused = true)]
async fn failed_wake_attention_event_identifies_the_abandoned_coordinator() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    harness.chat.set_available(false).await;
    let park = park(conversation.clone(), parent.id, delegate.id);
    harness.parks.insert(park.clone()).await;

    harness
        .service()
        .dispatch_wake(&park, DelegationWakeReason::AllSettled)
        .await
        .unwrap();

    let events = harness.events.events();
    let payload = &events[0].payload;
    assert_eq!(events[0].event, "delegation_park:needs_attention");
    assert_eq!(payload["park_id"], park.id.as_str());
    assert_eq!(payload["parent_conversation_id"], conversation.as_str());
    assert_eq!(payload["context_type"], "project");
    assert_eq!(payload["context_id"], "project");
    assert_eq!(payload["delegate_count"], 1);
    assert!(payload["error"].as_str().is_some_and(|e| !e.is_empty()));
}

#[tokio::test]
async fn failed_wake_attention_event_survives_an_unreadable_conversation() {
    let harness = harness();
    let park = park(
        ChatConversationId::new(),
        AgentRunId::new(),
        AgentRunId::new(),
    );
    harness.parks.insert(park.clone()).await;

    harness
        .service()
        .dispatch_wake(&park, DelegationWakeReason::AllSettled)
        .await
        .unwrap_err();

    let events = harness.events.events();
    let payload = &events[0].payload;
    assert_eq!(
        payload["parent_conversation_id"],
        park.parent_conversation_id.as_str()
    );
    assert!(payload["context_type"].is_null());
    assert!(payload["conversation_title"].is_null());
}

#[tokio::test]
async fn deadline_wake_omits_the_timeout_notice_when_every_job_settled() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    // `reconcile_park` checks the deadline before the wake decision, so a park whose last job
    // settles in the same pass still wakes as `Deadline`. It must not claim a timeout.
    let mut park = park(conversation, parent.id, delegate.id);
    park.deadline_at = Utc::now() - Duration::seconds(1);
    harness.parks.insert(park).await;

    harness
        .service()
        .reconcile_all(harness.runs.as_ref())
        .await
        .unwrap();

    let message = &harness.chat.get_sent_messages().await[0];
    assert!(!message.contains("Timeout notice"));
    assert!(message.contains("researcher: completed"));
    assert_eq!(
        harness.parks.settled.lock().await.as_slice(),
        &[DelegationParkState::Expired]
    );
}

#[tokio::test(start_paused = true)]
async fn reclaiming_an_abandoned_wake_restores_the_full_retry_budget() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    harness.chat.set_available(false).await;
    let config = delegation_config();
    let retry_backoff_secs = i64::try_from(config.park_wake_retry_backoff_secs).unwrap();
    let stale_threshold = Duration::seconds(
        i64::from(config.park_wake_retry_max)
            .saturating_mul(retry_backoff_secs)
            .saturating_add(retry_backoff_secs),
    );
    // A dispatcher that crashed mid-wake leaves both a held claim and its spent attempts behind.
    let mut park = park(conversation, parent.id, delegate.id);
    park.state = DelegationParkState::Waking;
    park.wake_attempts = i32::try_from(config.park_wake_retry_max).unwrap();
    park.updated_at = Utc::now() - stale_threshold - stale_threshold;
    harness.parks.insert(park.clone()).await;

    harness
        .service()
        .reconcile_all(harness.runs.as_ref())
        .await
        .unwrap();

    assert_eq!(harness.chat.call_count(), config.park_wake_retry_max.max(1));
}

#[tokio::test]
async fn wake_preview_truncates_long_delegate_errors_at_a_character_boundary() {
    let harness = harness();
    let (conversation, parent, delegate) = insert_parent_and_delegate(&harness).await;
    let long_error = "é".repeat(260);
    harness.runs.fail(&delegate.id, &long_error).await.unwrap();
    let mut park = park(conversation, parent.id, delegate.id);
    park.jobs[0].settled_status = Some("failed".to_string());
    harness.parks.insert(park.clone()).await;

    harness
        .service()
        .dispatch_wake(&park, DelegationWakeReason::DelegateFailed)
        .await
        .unwrap();

    let message = &harness.chat.get_sent_messages().await[0];
    assert!(message.contains(&format!("{}…", "é".repeat(240))));
    assert!(!message.contains(&"é".repeat(241)));
}
