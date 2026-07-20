//! Service for auto-launching deferred pending sessions when ideation capacity frees up.
//!
//! `PendingSessionDrainService` is triggered after an ideation agent stream completes.
//! It claims the oldest pending session for the project (atomic via `BEGIN IMMEDIATE`),
//! checks whether a slot is still available, and sends a message to start the agent.
//! On any failure, the prompt is re-persisted so no data is lost.
//!
//! Pattern: mirrors `resume_paused_ideation_queues_with_chat_service()` in
//! `execution_commands/control_helpers.rs`.

use std::sync::Arc;

use crate::application::chat_service::{
    decode_pending_initial_prompt, uses_execution_slot, ChatService, SendCallerContext,
    SendMessageOptions,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    ChatContextType, IdeationSessionId, InternalStatus, ProjectId, TaskId,
};
use crate::domain::repositories::{
    ChatConversationRepository, ExecutionSettingsRepository, IdeationSessionRepository,
    ProjectRepository, TaskRepository,
};
use crate::domain::services::{MessageQueue, RunningAgentRegistry};

/// Drains deferred pending ideation sessions for a project when capacity frees up.
///
/// On each call to `try_drain_pending_for_project`, the service loops:
/// 1. Atomically claim the oldest pending session (BEGIN IMMEDIATE).
/// 2. Check ideation capacity via `can_start_ideation()`.
///    - No capacity → re-persist prompt, stop.
/// 3. Send a message to start the agent (clearing the pending prompt on success).
///    - Failure → re-persist prompt, stop.
/// 4. Continue loop for additional pending sessions.
pub struct PendingSessionDrainService {
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    task_repo: Arc<dyn TaskRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
    execution_state: Arc<ExecutionState>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    message_queue: Arc<MessageQueue>,
    chat_service: Arc<dyn ChatService>,
}

impl PendingSessionDrainService {
    pub fn new(
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        task_repo: Arc<dyn TaskRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
        execution_state: Arc<ExecutionState>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        message_queue: Arc<MessageQueue>,
        chat_service: Arc<dyn ChatService>,
    ) -> Self {
        Self {
            ideation_session_repo,
            project_repo,
            task_repo,
            conversation_repo,
            execution_settings_repo,
            execution_state,
            running_agent_registry,
            message_queue,
            chat_service,
        }
    }

    /// Drain pending sessions for `project_id` until capacity is exhausted or the
    /// queue is empty.
    pub async fn try_drain_pending_for_project(&self, project_id: &str) {
        loop {
            // Step 1: Atomically claim the oldest pending session for this project.
            let (session_id, prompt) = match self
                .ideation_session_repo
                .claim_pending_session_for_project(project_id)
                .await
            {
                Ok(Some(item)) => item,
                Ok(None) => break, // nothing pending
                Err(e) => {
                    tracing::warn!(
                        project_id = %project_id,
                        error = %e,
                        "PendingSessionDrainService: claim failed"
                    );
                    break;
                }
            };

            // Step 2: Check capacity using the full can_start_ideation parameter set.
            // map_err converts AppError (contains Box<dyn StdError>, non-Send) to String
            // (Send) so the match arm can safely cross the .await in the error path.
            let pid_typed = ProjectId::from_string(project_id.to_string());
            let project_settings = match self
                .execution_settings_repo
                .get_settings(Some(&pid_typed))
                .await
                .map_err(|e| e.to_string())
            {
                Ok(s) => s,
                Err(err_msg) => {
                    tracing::warn!(
                        project_id = %project_id,
                        session_id = %session_id,
                        error = %err_msg,
                        "PendingSessionDrainService: get_settings failed, re-persisting"
                    );
                    self.restore_prompt(&session_id, &prompt).await;
                    break;
                }
            };

            let running_global_ideation = self.count_global_ideation_slots().await;
            let running_project_ideation = self.count_project_ideation_slots(project_id).await;
            let running_project_total = self.count_project_total_slots(project_id).await;
            let global_execution_waiting = self.has_runnable_execution_waiting(None).await;
            let project_execution_waiting =
                self.has_runnable_execution_waiting(Some(&pid_typed)).await;

            if !self.execution_state.can_start_ideation(
                running_global_ideation,
                running_project_ideation,
                running_project_total,
                project_settings.max_concurrent_tasks,
                project_settings.project_ideation_max,
                global_execution_waiting,
                project_execution_waiting,
            ) {
                tracing::debug!(
                    project_id = %project_id,
                    session_id = %session_id,
                    running_global_ideation,
                    running_project_ideation,
                    global_execution_waiting,
                    project_execution_waiting,
                    "PendingSessionDrainService: no capacity, re-persisting prompt"
                );
                self.restore_prompt(&session_id, &prompt).await;
                break;
            }

            // Step 3: Start the agent by sending the deferred prompt.
            // map_err converts AppError (contains Box<dyn StdError>, non-Send) to String
            // (Send) so the match arm can safely cross the .await in the error path.
            let (message, metadata) = decode_pending_initial_prompt(&prompt);
            match self
                .chat_service
                .send_message(
                    ChatContextType::Ideation,
                    &session_id,
                    &message,
                    SendMessageOptions {
                        caller_context: SendCallerContext::DrainService,
                        metadata,
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| e.to_string())
            {
                Ok(_) => {
                    tracing::info!(
                        project_id = %project_id,
                        session_id = %session_id,
                        "PendingSessionDrainService: launched pending session"
                    );
                    // Loop to check for more pending sessions in this project.
                }
                Err(err_msg) => {
                    tracing::warn!(
                        project_id = %project_id,
                        session_id = %session_id,
                        error = %err_msg,
                        "PendingSessionDrainService: send_message failed, re-persisting prompt"
                    );
                    self.restore_prompt(&session_id, &prompt).await;
                    break;
                }
            }
        }
    }

    /// Re-persist the prompt after a failed drain attempt so no data is lost.
    async fn restore_prompt(&self, session_id: &str, prompt: &str) {
        if let Err(e) = self
            .ideation_session_repo
            .set_pending_initial_prompt(session_id, Some(prompt.to_string()))
            .await
        {
            tracing::error!(
                session_id = %session_id,
                error = %e,
                "PendingSessionDrainService: CRITICAL — failed to re-persist prompt"
            );
        }
    }

    /// Count running ideation slots across all projects.
    async fn count_global_ideation_slots(&self) -> u32 {
        let entries = self.running_agent_registry.list_all().await;
        entries
            .iter()
            .filter(|(key, info)| {
                key.context_type == ChatContextType::Ideation.to_string()
                    && info.pid != 0
                    && !self
                        .execution_state
                        .is_interactive_idle(&format!("{}/{}", key.context_type, key.context_id))
            })
            .count() as u32
    }

    /// Count running ideation slots for a specific project (requires session lookups).
    async fn count_project_ideation_slots(&self, project_id: &str) -> u32 {
        let entries = self.running_agent_registry.list_all().await;
        let mut count = 0u32;
        for (key, info) in &entries {
            if key.context_type != ChatContextType::Ideation.to_string() || info.pid == 0 {
                continue;
            }
            if self
                .execution_state
                .is_interactive_idle(&format!("{}/{}", key.context_type, key.context_id))
            {
                continue;
            }
            let sid = IdeationSessionId::from_string(key.context_id.clone());
            if let Ok(Some(session)) = self.ideation_session_repo.get_by_id(&sid).await {
                if session.project_id.as_str() == project_id {
                    count += 1;
                }
            }
        }
        count
    }

    /// Count all slot-consuming contexts (ideation + task_execution + review + merge) for a
    /// project. Uses ideation_session_repo for ideation entries and task_repo for the rest.
    async fn count_project_total_slots(&self, project_id: &str) -> u32 {
        let entries = self.running_agent_registry.list_all().await;
        let mut count = 0u32;
        for (key, info) in &entries {
            if info.pid == 0 {
                continue;
            }
            let ctx_type = match key.context_type.parse::<ChatContextType>() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if !uses_execution_slot(ctx_type) {
                continue;
            }
            if self
                .execution_state
                .is_interactive_idle(&format!("{}/{}", key.context_type, key.context_id))
            {
                continue;
            }
            let belongs = if key.context_type == ChatContextType::Ideation.to_string() {
                let sid = IdeationSessionId::from_string(key.context_id.clone());
                matches!(
                    self.ideation_session_repo.get_by_id(&sid).await,
                    Ok(Some(ref s)) if s.project_id.as_str() == project_id
                )
            } else {
                // task_execution, review, merge — context_id is the task ID
                let tid = TaskId::from_string(key.context_id.clone());
                matches!(
                    self.task_repo.get_by_id(&tid).await,
                    Ok(Some(ref t)) if t.project_id.as_str() == project_id
                )
            };
            if belongs {
                count += 1;
            }
        }
        count
    }

    async fn has_runnable_execution_waiting(&self, project_filter: Option<&ProjectId>) -> bool {
        if let Some(project_id) = project_filter {
            if matches!(
                self.task_repo.get_by_project(project_id).await,
                Ok(tasks) if tasks
                    .iter()
                    .any(|task| task.internal_status == InternalStatus::Ready)
            ) {
                return true;
            }
        } else if let Ok(projects) = self.project_repo.get_all().await {
            for project in projects {
                if matches!(
                    self.task_repo.get_by_project(&project.id).await,
                    Ok(tasks) if tasks
                        .iter()
                        .any(|task| task.internal_status == InternalStatus::Ready)
                ) {
                    return true;
                }
            }
        }

        for key in self.message_queue.list_keys() {
            match key.context_type {
                ChatContextType::Project => {
                    if matches!(
                        crate::application::workspace_capacity::queue_key_matches_workspace_project(
                            &key,
                            project_filter,
                            &self.project_repo,
                            &self.conversation_repo,
                        )
                        .await,
                        Ok(true)
                    ) {
                        return true;
                    }
                }
                ChatContextType::TaskExecution
                | ChatContextType::Review
                | ChatContextType::Merge => {
                    let task_id = TaskId::from_string(key.context_id.clone());
                    if matches!(
                        self.task_repo.get_by_id(&task_id).await,
                        Ok(Some(task)) if project_filter
                            .is_none_or(|project_id| task.project_id == *project_id)
                    ) {
                        return true;
                    }
                }
                ChatContextType::Standalone
                | ChatContextType::Ideation
                | ChatContextType::Delegation
                | ChatContextType::Task
                | ChatContextType::BranchUpdate => {}
            }
        }

        false
    }
}
