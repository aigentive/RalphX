use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::verification_child_session::*;
use super::AppState;
use crate::application::chat_service::{
    AgentRunningState, ChatConversationWithMessages, ChatService, ChatServiceError,
    SendMessageOptions, SendResult,
};
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    build_child_session, AgentRun, ChatContextType, ChatConversation, ChatConversationId,
    ChildSessionDraftInput, IdeationSession, IdeationSessionId, IdeationSessionStatus, ProjectId,
    SessionPurpose, VerificationStatus,
};
use crate::domain::services::{build_blank_verification_snapshot, QueuedMessage};

#[derive(Clone, Copy, Default)]
enum VerificationSendBehavior {
    #[default]
    Sent,
    Queued,
    Fail,
}

#[derive(Clone, Default)]
struct RecordingVerificationChatService {
    sent_options: Arc<Mutex<Vec<SendMessageOptions>>>,
    sent_messages: Arc<Mutex<Vec<String>>>,
    behavior: VerificationSendBehavior,
}

impl RecordingVerificationChatService {
    fn queued() -> Self {
        Self {
            behavior: VerificationSendBehavior::Queued,
            ..Default::default()
        }
    }

    fn failing() -> Self {
        Self {
            behavior: VerificationSendBehavior::Fail,
            ..Default::default()
        }
    }

    async fn sent_options(&self) -> Vec<SendMessageOptions> {
        self.sent_options.lock().await.clone()
    }

    async fn sent_messages(&self) -> Vec<String> {
        self.sent_messages.lock().await.clone()
    }
}

#[async_trait]
impl ChatService for RecordingVerificationChatService {
    async fn send_message(
        &self,
        _context_type: ChatContextType,
        context_id: &str,
        message: &str,
        options: SendMessageOptions,
    ) -> Result<SendResult, ChatServiceError> {
        self.sent_options.lock().await.push(options);
        self.sent_messages.lock().await.push(message.to_string());
        match self.behavior {
            VerificationSendBehavior::Sent => Ok(SendResult {
                conversation_id: context_id.to_string(),
                agent_run_id: "agent-run-1".to_string(),
                ..Default::default()
            }),
            VerificationSendBehavior::Queued => Ok(SendResult {
                conversation_id: context_id.to_string(),
                agent_run_id: "agent-run-1".to_string(),
                queued_as_pending: true,
                ..Default::default()
            }),
            VerificationSendBehavior::Fail => Err(ChatServiceError::SpawnFailed(
                "capacity worker crashed".to_string(),
            )),
        }
    }

    async fn queue_message(
        &self,
        _context_type: ChatContextType,
        _context_id: &str,
        _content: &str,
        _client_id: Option<&str>,
    ) -> Result<QueuedMessage, ChatServiceError> {
        panic!("queue_message is not used by verification child spawn tests")
    }

    async fn get_queued_messages(
        &self,
        _context_type: ChatContextType,
        _context_id: &str,
    ) -> Result<Vec<QueuedMessage>, ChatServiceError> {
        Ok(Vec::new())
    }

    async fn delete_queued_message(
        &self,
        _context_type: ChatContextType,
        _context_id: &str,
        _message_id: &str,
    ) -> Result<bool, ChatServiceError> {
        Ok(false)
    }

    async fn send_queued_message_now(
        &self,
        _context_type: ChatContextType,
        _context_id: &str,
        _message_id: &str,
    ) -> Result<SendResult, ChatServiceError> {
        panic!("send_queued_message_now is not used by verification child spawn tests")
    }

    async fn get_or_create_conversation(
        &self,
        _context_type: ChatContextType,
        context_id: &str,
    ) -> Result<(ChatConversation, bool), ChatServiceError> {
        Ok((
            ChatConversation::new_ideation(IdeationSessionId::from_string(context_id)),
            true,
        ))
    }

    async fn get_conversation_with_messages(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> Result<Option<ChatConversationWithMessages>, ChatServiceError> {
        Ok(None)
    }

    async fn list_conversations(
        &self,
        _context_type: ChatContextType,
        _context_id: &str,
    ) -> Result<Vec<ChatConversation>, ChatServiceError> {
        Ok(Vec::new())
    }

    async fn get_active_run(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> Result<Option<AgentRun>, ChatServiceError> {
        Ok(None)
    }

    async fn is_available(&self) -> bool {
        true
    }

    async fn stop_agent(
        &self,
        _context_type: ChatContextType,
        _context_id: &str,
    ) -> Result<bool, ChatServiceError> {
        Ok(false)
    }

    async fn is_agent_running(&self, _context_type: ChatContextType, _context_id: &str) -> bool {
        false
    }

    async fn get_agent_running_states(
        &self,
        _context_type: ChatContextType,
        _context_ids: &[String],
    ) -> HashMap<String, AgentRunningState> {
        HashMap::new()
    }
}

#[tokio::test]
async fn spawn_verification_child_session_forwards_provider_harness_override() {
    let state = AppState::new_test();
    let parent = IdeationSession::builder()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .build();
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();
    let chat_service = RecordingVerificationChatService::default();
    let captured = chat_service.clone();

    let outcome = spawn_verification_child_session(
        &state,
        &parent_id,
        "Run verification",
        "Verifier",
        Some(AgentHarnessKind::Codex),
        &[],
        |_| chat_service,
    )
    .await
    .unwrap();

    assert!(outcome.orchestration_triggered);
    let options = captured.sent_options().await;
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].harness_override, Some(AgentHarnessKind::Codex));
}

#[test]
fn blank_orphaned_active_generation_requires_no_summary_or_child_activity() {
    let blank_snapshot = build_blank_verification_snapshot(3, VerificationStatus::Reviewing, true);
    let child_state = VerificationChildState {
        latest_child: None,
        has_active_child: false,
    };

    assert!(is_blank_orphaned_active_generation(
        false,
        Some(&blank_snapshot),
        &child_state,
    ));
    assert!(!is_blank_orphaned_active_generation(
        true,
        Some(&blank_snapshot),
        &child_state,
    ));
    assert!(!is_blank_orphaned_active_generation(
        false,
        None,
        &child_state,
    ));

    let active_child_state = VerificationChildState {
        latest_child: None,
        has_active_child: true,
    };
    assert!(!is_blank_orphaned_active_generation(
        false,
        Some(&blank_snapshot),
        &active_child_state,
    ));

    let terminal_snapshot =
        build_blank_verification_snapshot(3, VerificationStatus::Verified, false);
    assert!(!is_blank_orphaned_active_generation(
        false,
        Some(&terminal_snapshot),
        &child_state,
    ));

    let mut nonblank_snapshot = blank_snapshot;
    nonblank_snapshot.convergence_reason = Some("still has state".to_string());
    assert!(!is_blank_orphaned_active_generation(
        false,
        Some(&nonblank_snapshot),
        &child_state,
    ));
}

#[tokio::test]
async fn repair_blank_orphaned_verification_generation_clears_archived_child_snapshot() {
    let state = AppState::new_test();
    let mut parent = IdeationSession::builder()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .verification_generation(7)
        .build();
    parent.verification_in_progress = true;
    let parent_id = parent.id.clone();
    state
        .ideation_session_repo
        .create(parent.clone())
        .await
        .unwrap();
    let snapshot = build_blank_verification_snapshot(7, VerificationStatus::Reviewing, true);
    state
        .ideation_session_repo
        .save_verification_run_snapshot(&parent_id, &snapshot)
        .await
        .unwrap();

    let mut child = build_child_session(
        parent_id.clone(),
        &parent,
        ChildSessionDraftInput {
            title: Some("Verifier".to_string()),
            inherit_context: true,
            team_mode: None,
            team_config_json: None,
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            purpose: SessionPurpose::Verification,
            is_external_trigger: false,
        },
    );
    child.status = IdeationSessionStatus::Archived;
    state.ideation_session_repo.create(child).await.unwrap();

    assert!(
        repair_blank_orphaned_verification_generation(&state, &parent)
            .await
            .unwrap()
    );
    let repaired = state
        .ideation_session_repo
        .get_verification_run_snapshot(&parent_id, 7)
        .await
        .unwrap()
        .expect("repaired snapshot");
    assert_eq!(repaired.status, VerificationStatus::Unverified);
    assert!(!repaired.in_progress);
    assert!(repaired.current_gaps.is_empty());
    assert!(repaired.rounds.is_empty());
    assert!(
        !repair_blank_orphaned_verification_generation(&state, &parent)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn spawn_verification_child_session_persists_prompt_when_capacity_deferred() {
    let state = AppState::new_test();
    let parent = IdeationSession::builder()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .build();
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();
    let chat_service = RecordingVerificationChatService::queued();
    let captured = chat_service.clone();

    let outcome = spawn_verification_child_session(
        &state,
        &parent_id,
        "Run verification",
        "Verifier",
        Some(AgentHarnessKind::Codex),
        &["security".to_string(), "qa".to_string()],
        |_| chat_service,
    )
    .await
    .unwrap();

    assert!(!outcome.orchestration_triggered);
    assert_eq!(
        outcome.pending_initial_prompt.as_deref(),
        Some("Run verification")
    );
    let child = state
        .ideation_session_repo
        .get_by_id(&outcome.child_session_id)
        .await
        .unwrap()
        .expect("child session");
    assert_eq!(
        child.pending_initial_prompt.as_deref(),
        Some("Run verification\nDISABLED_SPECIALISTS: security, qa")
    );
    let messages = captured.sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("DISABLED_SPECIALISTS: security, qa"));
    let options = captured.sent_options().await;
    assert_eq!(options[0].harness_override, Some(AgentHarnessKind::Codex));
}

#[tokio::test]
async fn spawn_verification_child_session_archives_child_when_send_fails() {
    let state = AppState::new_test();
    let parent = IdeationSession::builder()
        .project_id(ProjectId::from_string("project-1".to_string()))
        .build();
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();
    let chat_service = RecordingVerificationChatService::failing();
    let captured = chat_service.clone();

    let outcome = spawn_verification_child_session(
        &state,
        &parent_id,
        "Run verification",
        "Verifier",
        None,
        &[],
        |_| chat_service,
    )
    .await
    .unwrap();

    assert!(!outcome.orchestration_triggered);
    assert_eq!(
        outcome.pending_initial_prompt.as_deref(),
        Some("Run verification")
    );
    let child = state
        .ideation_session_repo
        .get_by_id(&outcome.child_session_id)
        .await
        .unwrap()
        .expect("child session");
    assert_eq!(child.status, IdeationSessionStatus::Archived);
    assert_eq!(child.pending_initial_prompt, None);
    assert_eq!(
        captured.sent_messages().await,
        vec!["Run verification".to_string()]
    );
}
