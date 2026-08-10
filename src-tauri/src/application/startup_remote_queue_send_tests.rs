use super::{startup_background::dispatch_one_remote_queued_send, AppState};
use crate::{
    commands::ExecutionState,
    domain::entities::{
        AgentRun, ChatContextType, ChatConversation, ProjectId, RemoteQueuedSendRequest,
        RemoteQueuedSendRequestStatus,
    },
    domain::{agents::AgentHarnessKind, services::QueuedMessage},
    infrastructure::memory::MemoryAgentProviderSettingsRepository,
};
use std::sync::Arc;

async fn seed_claim(state: &AppState, message_id: &str) -> RemoteQueuedSendRequest {
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "project-queue-send".to_string(),
        )))
        .await
        .unwrap();
    let now = chrono::Utc::now();
    state
        .remote_queued_send_request_repo
        .create_remote_queued_send_request(RemoteQueuedSendRequest {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: conversation.id.as_str(),
            queued_message_id: message_id.into(),
            expected_active_run_id: None,
            status: RemoteQueuedSendRequestStatus::Pending,
            error_code: None,
            result: None,
            claimed_at: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn remote_queue_send_entry_gone_settles_and_reentry_is_noop() {
    let state = AppState::new_test();
    let row = seed_claim(&state, "gone").await;
    dispatch_one_remote_queued_send(&state, &Arc::new(ExecutionState::new()))
        .await
        .unwrap();
    let settled = state
        .remote_queued_send_request_repo
        .get(&row.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(settled.status, RemoteQueuedSendRequestStatus::Failed);
    assert_eq!(
        settled.error_code.as_deref(),
        Some(crate::application::remote_queue_send_intent::REMOTE_QUEUE_SEND_ENTRY_GONE)
    );
    dispatch_one_remote_queued_send(&state, &Arc::new(ExecutionState::new()))
        .await
        .unwrap();
    assert_eq!(
        state
            .remote_queued_send_request_repo
            .get(&row.id)
            .await
            .unwrap()
            .unwrap(),
        settled
    );
}

#[tokio::test]
async fn remote_queue_send_stale_claim_is_terminal_and_never_dispatched() {
    let state = AppState::new_test();
    let row = seed_claim(&state, "still-queued").await;
    let old = chrono::Utc::now() - chrono::Duration::seconds(301);
    state
        .remote_queued_send_request_repo
        .claim_pending(old)
        .await
        .unwrap();
    assert_eq!(
        state
            .remote_queued_send_request_repo
            .fail_stale(
                chrono::Utc::now() - chrono::Duration::seconds(300),
                chrono::Utc::now()
            )
            .await
            .unwrap(),
        1
    );
    dispatch_one_remote_queued_send(&state, &Arc::new(ExecutionState::new()))
        .await
        .unwrap();
    assert_eq!(
        state
            .remote_queued_send_request_repo
            .get(&row.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        RemoteQueuedSendRequestStatus::FailedStale
    );
}

#[tokio::test]
async fn remote_queue_send_run_changed_preserves_active_run_and_queue() {
    let state = AppState::new_test();
    let mut row = seed_claim(&state, "queued").await;
    row.expected_active_run_id = Some("00000000-0000-0000-0000-000000000001".to_string());
    let claimed = state
        .remote_queued_send_request_repo
        .claim_pending(chrono::Utc::now())
        .await
        .unwrap()
        .unwrap();
    state
        .remote_queued_send_request_repo
        .fail(&claimed.id, "replace-fixture", None, chrono::Utc::now())
        .await
        .unwrap();
    let now = chrono::Utc::now();
    row.id = uuid::Uuid::new_v4().to_string();
    row.status = RemoteQueuedSendRequestStatus::Pending;
    row.claimed_at = None;
    row.created_at = now;
    row.updated_at = now;
    state
        .remote_queued_send_request_repo
        .create_remote_queued_send_request(row.clone())
        .await
        .unwrap();
    state.message_queue.queue_back_existing(
        ChatContextType::Project,
        &row.conversation_id,
        QueuedMessage::with_id("queued".into(), "payload".into()),
    );
    let active = AgentRun::new(crate::domain::entities::ChatConversationId::from_string(
        row.conversation_id.clone(),
    ));
    state.agent_run_repo.create(active.clone()).await.unwrap();
    dispatch_one_remote_queued_send(&state, &Arc::new(ExecutionState::new()))
        .await
        .unwrap();
    let settled = state
        .remote_queued_send_request_repo
        .get(&row.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        settled.error_code.as_deref(),
        Some(crate::application::remote_queue_send_intent::REMOTE_QUEUE_SEND_RUN_CHANGED)
    );
    assert_eq!(
        state
            .agent_run_repo
            .get_active_for_conversation(&active.conversation_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        active.id
    );
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &row.conversation_id)
            .len(),
        1
    );
}

#[tokio::test]
async fn remote_queue_send_provider_disabled_preserves_queue() {
    let mut state = AppState::new_test();
    state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let row = seed_claim(&state, "queued-provider").await;
    let mut queued = QueuedMessage::with_id("queued-provider".into(), "payload".into());
    queued.harness_override = Some(AgentHarnessKind::Codex);
    state
        .message_queue
        .queue_back_existing(ChatContextType::Project, &row.conversation_id, queued);
    dispatch_one_remote_queued_send(&state, &Arc::new(ExecutionState::new()))
        .await
        .unwrap();
    let settled = state
        .remote_queued_send_request_repo
        .get(&row.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        settled.error_code.as_deref(),
        Some(crate::application::remote_queue_send_intent::REMOTE_QUEUE_SEND_PROVIDER_DISABLED)
    );
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &row.conversation_id)
            .len(),
        1
    );
}
