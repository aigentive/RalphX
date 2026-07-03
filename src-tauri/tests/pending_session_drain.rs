//! Integration tests for PendingSessionDrainService.
//!
//! Covers the four core behaviors of the drain loop:
//!   - Empty queue → exits cleanly with no side effects
//!   - No capacity (paused) → prompt is re-persisted on the session
//!   - send_message failure → prompt is re-persisted on the session
//!   - Success → loop continues draining additional pending sessions

use std::sync::Arc;

use ralphx_lib::application::chat_service::{ChatService, MockChatService};
use ralphx_lib::application::pending_session_drain::PendingSessionDrainService;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{IdeationSession, IdeationSessionId, ProjectId};
use ralphx_lib::domain::repositories::IdeationSessionRepository;
use ralphx_lib::domain::services::{MemoryRunningAgentRegistry, MessageQueue};
use ralphx_lib::infrastructure::memory::{
    MemoryChatConversationRepository, MemoryExecutionSettingsRepository,
    MemoryIdeationSessionRepository, MemoryProjectRepository, MemoryTaskRepository,
};

fn make_service(
    session_repo: Arc<MemoryIdeationSessionRepository>,
    execution_state: Arc<ExecutionState>,
    chat_service: Arc<MockChatService>,
) -> PendingSessionDrainService {
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let settings_repo = Arc::new(MemoryExecutionSettingsRepository::new());
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let message_queue = Arc::new(MessageQueue::new());
    PendingSessionDrainService::new(
        session_repo,
        project_repo,
        task_repo,
        conversation_repo,
        settings_repo,
        execution_state,
        registry,
        message_queue,
        chat_service as Arc<dyn ChatService>,
    )
}

async fn create_pending_session(
    repo: &MemoryIdeationSessionRepository,
    project_id: &ProjectId,
    prompt: &str,
) -> IdeationSessionId {
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();
    repo.set_pending_initial_prompt(session_id.as_str(), Some(prompt.to_string()))
        .await
        .unwrap();
    session_id
}

// ============================================================================
// Test 1: Empty queue exits immediately without panicking or sending messages.
// ============================================================================

#[tokio::test]
async fn test_empty_queue_exits_immediately() {
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let execution_state = Arc::new(ExecutionState::new());
    let chat_service = Arc::new(MockChatService::new());
    let project_id = ProjectId::new();

    let service = make_service(
        Arc::clone(&session_repo),
        execution_state,
        Arc::clone(&chat_service),
    );

    // No sessions → drain should return immediately with no send_message calls.
    service
        .try_drain_pending_for_project(project_id.as_str())
        .await;

    assert_eq!(chat_service.call_count(), 0);
}

// ============================================================================
// Test 2: No capacity (paused) → prompt is re-persisted, nothing sent.
// ============================================================================

#[tokio::test]
async fn test_no_capacity_re_persists_prompt() {
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let execution_state = Arc::new(ExecutionState::new());
    let chat_service = Arc::new(MockChatService::new());
    let project_id = ProjectId::new();

    // Pause execution to force can_start_ideation() → false.
    execution_state.pause();

    let session_id = create_pending_session(&session_repo, &project_id, "hello from pending").await;

    let service = make_service(
        Arc::clone(&session_repo),
        execution_state,
        Arc::clone(&chat_service),
    );

    service
        .try_drain_pending_for_project(project_id.as_str())
        .await;

    // No message should have been sent.
    assert_eq!(chat_service.call_count(), 0);

    // The prompt must be re-persisted on the session (not lost).
    let session = session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("session must exist");
    assert_eq!(
        session.pending_initial_prompt.as_deref(),
        Some("hello from pending"),
        "prompt must be re-persisted when capacity is unavailable"
    );
}

// ============================================================================
// Test 3: send_message failure → prompt is re-persisted on the session.
// ============================================================================

#[tokio::test]
async fn test_send_message_failure_re_persists_prompt() {
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let execution_state = Arc::new(ExecutionState::new());
    let chat_service = Arc::new(MockChatService::new());
    let project_id = ProjectId::new();

    // Make the chat service unavailable so send_message returns an error.
    chat_service.set_available(false).await;

    let session_id =
        create_pending_session(&session_repo, &project_id, "deferred launch prompt").await;

    let service = make_service(
        Arc::clone(&session_repo),
        execution_state,
        Arc::clone(&chat_service),
    );

    service
        .try_drain_pending_for_project(project_id.as_str())
        .await;

    // send_message was called once (the attempt that failed).
    assert_eq!(chat_service.call_count(), 1);

    // The prompt must be re-persisted so it's not lost.
    let session = session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("session must exist");
    assert_eq!(
        session.pending_initial_prompt.as_deref(),
        Some("deferred launch prompt"),
        "prompt must be re-persisted after send_message failure"
    );
}

// ============================================================================
// Test 4: Success → loop continues to drain the next pending session.
// ============================================================================

#[tokio::test]
async fn test_success_continues_loop() {
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let execution_state = Arc::new(ExecutionState::new());
    let chat_service = Arc::new(MockChatService::new());
    let project_id = ProjectId::new();

    // Seed two pending sessions in the same project.
    let session_id_1 = create_pending_session(&session_repo, &project_id, "prompt-one").await;
    let session_id_2 = create_pending_session(&session_repo, &project_id, "prompt-two").await;

    let service = make_service(
        Arc::clone(&session_repo),
        execution_state,
        Arc::clone(&chat_service),
    );

    service
        .try_drain_pending_for_project(project_id.as_str())
        .await;

    // Both sessions should have been launched (send_message called twice).
    assert_eq!(
        chat_service.call_count(),
        2,
        "drain must loop and launch all pending sessions in the project"
    );

    // Neither session should have a pending prompt left (both cleared on success).
    let s1 = session_repo
        .get_by_id(&session_id_1)
        .await
        .unwrap()
        .expect("session 1 must exist");
    let s2 = session_repo
        .get_by_id(&session_id_2)
        .await
        .unwrap()
        .expect("session 2 must exist");

    assert!(
        s1.pending_initial_prompt.is_none(),
        "session 1 prompt must be cleared after successful launch"
    );
    assert!(
        s2.pending_initial_prompt.is_none(),
        "session 2 prompt must be cleared after successful launch"
    );
}
