use crate::application::chat_service::{ChatService, SendMessageOptions};
use crate::application::harness_runtime_registry::default_verification_max_rounds;
use crate::application::verification_event_emitters::{
    emit_verification_started, emit_verification_status_changed,
};
use crate::application::AppState;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    build_child_session, ChatContextType, ChildSessionDraftInput, IdeationSession,
    IdeationSessionId, IdeationSessionStatus, SessionLink, SessionPurpose, SessionRelationship,
    VerificationRunSnapshot, VerificationStatus,
};
use crate::domain::repositories::IdeationSessionRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::SqliteIdeationSessionRepository as SessionRepo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationChildSessionSpawnOutcome {
    pub child_session_id: IdeationSessionId,
    pub child_title: String,
    pub orchestration_triggered: bool,
    pub pending_initial_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationAgentSpawnOutcome {
    pub spawned: bool,
    pub failure_detail: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct VerificationChildState {
    pub latest_child: Option<IdeationSession>,
    pub has_active_child: bool,
}

pub(crate) async fn load_verification_child_state(
    repo: &std::sync::Arc<dyn IdeationSessionRepository>,
    session_id: &IdeationSessionId,
) -> AppResult<VerificationChildState> {
    let latest_child = repo.get_latest_verification_child(session_id).await?;
    let has_active_child = !repo.get_verification_children(session_id).await?.is_empty();
    Ok(VerificationChildState {
        latest_child,
        has_active_child,
    })
}

pub(crate) fn is_blank_in_progress_snapshot(snapshot: &VerificationRunSnapshot) -> bool {
    snapshot.status == VerificationStatus::Reviewing
        && snapshot.in_progress
        && snapshot.current_gaps.is_empty()
        && snapshot.rounds.is_empty()
        && snapshot.convergence_reason.is_none()
}

pub(crate) fn is_blank_orphaned_active_generation(
    summary_in_progress: bool,
    snapshot: Option<&VerificationRunSnapshot>,
    child_state: &VerificationChildState,
) -> bool {
    !summary_in_progress
        && !child_state.has_active_child
        && snapshot.is_some_and(is_blank_in_progress_snapshot)
}

pub(crate) async fn repair_blank_orphaned_verification_generation(
    app_state: &AppState,
    session: &IdeationSession,
) -> AppResult<bool> {
    if !session.verification_in_progress {
        return Ok(false);
    }

    let child_state =
        load_verification_child_state(&app_state.ideation_session_repo, &session.id).await?;
    let latest_child_archived = child_state
        .latest_child
        .as_ref()
        .is_some_and(|child| child.status == IdeationSessionStatus::Archived);
    if child_state.has_active_child || !latest_child_archived {
        return Ok(false);
    }

    let Some(snapshot) = app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session.id, session.verification_generation)
        .await?
    else {
        return Ok(false);
    };

    if !is_blank_in_progress_snapshot(&snapshot) {
        return Ok(false);
    }

    let mut repaired_snapshot = snapshot;
    crate::domain::services::clear_verification_snapshot(
        &mut repaired_snapshot,
        VerificationStatus::Unverified,
        false,
    );
    app_state
        .ideation_session_repo
        .save_verification_run_snapshot(&session.id, &repaired_snapshot)
        .await?;

    tracing::info!(
        session_id = %session.id.as_str(),
        generation = session.verification_generation,
        "Repaired blank orphaned verification generation before fresh start"
    );
    Ok(true)
}

pub async fn trigger_auto_verify_generation(
    state: &AppState,
    session_id: &IdeationSessionId,
) -> AppResult<Option<i32>> {
    let sid = session_id.as_str().to_string();
    state
        .db
        .run_transaction(move |conn| {
            let session_id = IdeationSessionId::from_string(sid);
            let _session =
                SessionRepo::get_by_id_sync(conn, session_id.as_str())?.ok_or_else(|| {
                    AppError::NotFound(format!("Session {} not found", session_id.as_str()))
                })?;
            SessionRepo::trigger_auto_verify_sync(conn, session_id.as_str())
        })
        .await
}

pub async fn handle_verification_spawn_failure(
    state: &AppState,
    session_id: &IdeationSessionId,
    generation: i32,
    error: Option<&str>,
) {
    if let Some(message) = error {
        tracing::error!(
            "Verifier spawn failed for session {}: {}",
            session_id.as_str(),
            message
        );
    } else {
        tracing::warn!(
            "Verification agent failed to spawn for session {}",
            session_id.as_str()
        );
    }

    let sid = session_id.as_str().to_string();
    if let Err(reset_error) = state
        .db
        .run(move |conn| SessionRepo::reset_auto_verify_sync(conn, &sid))
        .await
    {
        tracing::error!(
            "Failed to reset auto-verify state for session {} after spawn failure: {}",
            session_id.as_str(),
            reset_error
        );
        return;
    }

    emit_verification_status_changed(
        state.events.as_ref(),
        session_id.as_str(),
        VerificationStatus::Unverified,
        false,
        None,
        Some("spawn_failed"),
        Some(generation),
    );
}

pub async fn spawn_verification_agent<S, F>(
    state: &AppState,
    session_id: &IdeationSessionId,
    generation: i32,
    provider_harness: Option<AgentHarnessKind>,
    disabled_specialists: &[String],
    chat_service_for_session: F,
) -> VerificationAgentSpawnOutcome
where
    S: ChatService,
    F: FnOnce(&IdeationSession) -> S,
{
    let max_rounds = default_verification_max_rounds();
    emit_verification_started(
        state.events.as_ref(),
        session_id.as_str(),
        generation,
        max_rounds,
    );
    let title = format!("Auto-verification (gen {generation})");
    let description = format!(
        "Run verification round loop. parent_session_id: {}, generation: {generation}, max_rounds: {}",
        session_id.as_str(),
        max_rounds
    );

    match spawn_verification_child_session(
        state,
        session_id,
        &description,
        &title,
        provider_harness,
        disabled_specialists,
        chat_service_for_session,
    )
    .await
    {
        Ok(outcome) if outcome.orchestration_triggered => VerificationAgentSpawnOutcome {
            spawned: true,
            failure_detail: None,
        },
        Ok(_) => {
            handle_verification_spawn_failure(state, session_id, generation, None).await;
            VerificationAgentSpawnOutcome {
                spawned: false,
                failure_detail: Some(
                    "verification agent launch was deferred by capacity".to_string(),
                ),
            }
        }
        Err(error) => {
            let detail = error.to_string();
            handle_verification_spawn_failure(state, session_id, generation, Some(&detail)).await;
            VerificationAgentSpawnOutcome {
                spawned: false,
                failure_detail: Some(format!("verification agent failed to spawn: {detail}")),
            }
        }
    }
}

pub async fn spawn_verification_child_session<S, F>(
    state: &AppState,
    parent_session_id: &IdeationSessionId,
    description: &str,
    title: &str,
    provider_harness: Option<AgentHarnessKind>,
    disabled_specialists: &[String],
    chat_service_for_session: F,
) -> AppResult<VerificationChildSessionSpawnOutcome>
where
    S: ChatService,
    F: FnOnce(&crate::domain::entities::IdeationSession) -> S,
{
    let effective_description = if disabled_specialists.is_empty() {
        description.to_string()
    } else {
        format!(
            "{}\nDISABLED_SPECIALISTS: {}",
            description,
            disabled_specialists.join(", ")
        )
    };

    let parent = state
        .ideation_session_repo
        .get_by_id(parent_session_id)
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("Failed to fetch parent session: {error}"))
        })?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Parent session {} not found",
                parent_session_id.as_str()
            ))
        })?;

    let child_session = build_child_session(
        parent_session_id.clone(),
        &parent,
        ChildSessionDraftInput {
            title: Some(title.to_string()),
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

    let child_id = child_session.id.clone();
    let created_session = state.ideation_session_repo.create(child_session).await?;

    let link = SessionLink::new(
        parent_session_id.clone(),
        child_id.clone(),
        SessionRelationship::FollowOn,
    );
    state
        .session_link_repo
        .create(link)
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("Failed to create session link: {error}"))
        })?;

    let child_session_str = child_id.as_str().to_string();
    let chat_service = chat_service_for_session(&created_session);
    let orchestration_triggered = match chat_service
        .send_message(
            ChatContextType::Ideation,
            &child_session_str,
            effective_description.as_str(),
            SendMessageOptions {
                harness_override: provider_harness,
                ..Default::default()
            },
        )
        .await
    {
        Ok(send_result) if send_result.queued_as_pending => {
            tracing::info!(
                session_id = child_session_str,
                "Verification child launch deferred because ideation capacity is full"
            );
            if let Err(error) = state
                .ideation_session_repo
                .set_pending_initial_prompt(&child_session_str, Some(effective_description.clone()))
                .await
            {
                tracing::error!(
                    session_id = child_session_str,
                    error = %error,
                    "Failed to persist pending_initial_prompt for capacity-deferred verification child"
                );
            }
            false
        }
        Ok(_) => true,
        Err(error) => {
            tracing::error!(
                session_id = child_session_str,
                error = %error,
                "Failed to spawn plan verifier on verification child session"
            );
            if let Err(archive_error) = state
                .ideation_session_repo
                .update_status(&child_id, IdeationSessionStatus::Archived)
                .await
            {
                tracing::error!(
                    session_id = child_session_str,
                    error = %archive_error,
                    "Failed to archive verification child session after spawn failure"
                );
            }
            false
        }
    };

    let child_title = created_session.title.unwrap_or_else(|| title.to_string());
    let pending_initial_prompt = (!orchestration_triggered).then_some(description.to_string());
    state.events.emit(
        "ideation:child_session_created",
        serde_json::json!({
            "sessionId": child_session_str,
            "parentSessionId": parent_session_id.as_str(),
            "title": child_title.clone(),
            "purpose": "verification",
            "orchestrationTriggered": orchestration_triggered,
            "pendingInitialPrompt": pending_initial_prompt.clone()
        }),
    );

    Ok(VerificationChildSessionSpawnOutcome {
        child_session_id: child_id,
        child_title,
        orchestration_triggered,
        pending_initial_prompt,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;
    use crate::application::chat_service::{
        AgentRunningState, ChatConversationWithMessages, ChatServiceError, SendMessageOptions,
        SendResult,
    };
    use crate::domain::agents::AgentHarnessKind;
    use crate::domain::entities::{
        AgentRun, ChatConversation, ChatConversationId, IdeationSession, ProjectId,
    };
    use crate::domain::services::QueuedMessage;

    #[derive(Clone, Default)]
    struct RecordingVerificationChatService {
        sent_options: Arc<Mutex<Vec<SendMessageOptions>>>,
    }

    impl RecordingVerificationChatService {
        async fn sent_options(&self) -> Vec<SendMessageOptions> {
            self.sent_options.lock().await.clone()
        }
    }

    #[async_trait]
    impl ChatService for RecordingVerificationChatService {
        async fn send_message(
            &self,
            _context_type: ChatContextType,
            context_id: &str,
            _message: &str,
            options: SendMessageOptions,
        ) -> Result<SendResult, ChatServiceError> {
            self.sent_options.lock().await.push(options);
            Ok(SendResult {
                conversation_id: context_id.to_string(),
                agent_run_id: "agent-run-1".to_string(),
                ..Default::default()
            })
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

        async fn is_agent_running(
            &self,
            _context_type: ChatContextType,
            _context_id: &str,
        ) -> bool {
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
}
