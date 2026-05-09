// Chat Resumption Runner
//
// Handles automatic resumption of interrupted chat conversations on app startup.
// Conversations that were interrupted during app shutdown (Ideation, Task, Project,
// TaskExecution, Review) are automatically resumed, respecting pause state and
// deduplicating against StartupJobRunner for task-based chats.
//
// Usage:
// - Called once during app initialization after StartupJobRunner completes
// - Queries for interrupted conversations that still carry a resumable provider session
// - Prioritizes by context type: TaskExecution > Review > Task > Ideation > Project
// - Skips TaskExecution/Review if task is in AGENT_ACTIVE_STATUSES (handled by StartupJobRunner)
// - Sends "Continue where you left off." message to resume the stored provider session

use std::sync::Arc;
use tauri::{AppHandle, Runtime};
use tracing::{info, warn};

use crate::application::runtime_factory::{
    build_chat_service_with_fallback, ChatRuntimeFactoryDeps,
};
use crate::application::{AppChatService, ChatService, InteractiveProcessRegistry};
use crate::commands::execution_commands::{ExecutionState, AGENT_ACTIVE_STATUSES};
use crate::domain::entities::{ChatContextType, InterruptedConversation, TaskId};
use crate::domain::repositories::{
    AgentLaneSettingsRepository, AgentProviderSettingsRepository, AgentRunRepository,
    ExecutionSettingsRepository, PlanBranchRepository, TaskRepository,
};

/// Runs chat resumption on startup.
///
/// Finds all conversations that were interrupted when the app shut down
/// and resumes them by sending a message with `--resume` to continue the provider session.
pub struct ChatResumptionRunner<R: Runtime = tauri::Wry> {
    agent_run_repo: Arc<dyn AgentRunRepository>,
    chat_runtime_deps: ChatRuntimeFactoryDeps,
    task_repo: Arc<dyn TaskRepository>,
    execution_state: Arc<ExecutionState>,
    execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    app_handle: Option<AppHandle<R>>,
}

impl<R: Runtime> ChatResumptionRunner<R> {
    /// Create a new ChatResumptionRunner with all required dependencies.
    pub(crate) fn new(
        agent_run_repo: Arc<dyn AgentRunRepository>,
        task_repo: Arc<dyn TaskRepository>,
        execution_state: Arc<ExecutionState>,
        chat_runtime_deps: ChatRuntimeFactoryDeps,
    ) -> Self {
        Self {
            agent_run_repo,
            chat_runtime_deps,
            task_repo,
            execution_state,
            execution_settings_repo: None,
            agent_lane_settings_repo: None,
            agent_provider_settings_repo: None,
            plan_branch_repo: None,
            interactive_process_registry: None,
            app_handle: None,
        }
    }

    pub fn with_plan_branch_repo(mut self, repo: Arc<dyn PlanBranchRepository>) -> Self {
        self.plan_branch_repo = Some(repo);
        self
    }

    pub fn with_execution_settings_repo(
        mut self,
        repo: Arc<dyn ExecutionSettingsRepository>,
    ) -> Self {
        self.execution_settings_repo = Some(repo);
        self
    }

    pub fn with_agent_lane_settings_repo(
        mut self,
        repo: Arc<dyn AgentLaneSettingsRepository>,
    ) -> Self {
        self.agent_lane_settings_repo = Some(repo);
        self
    }

    pub fn with_agent_provider_settings_repo(
        mut self,
        repo: Arc<dyn AgentProviderSettingsRepository>,
    ) -> Self {
        self.agent_provider_settings_repo = Some(repo);
        self
    }

    /// Set the shared InteractiveProcessRegistry (builder pattern).
    pub fn with_interactive_process_registry(
        mut self,
        ipr: Arc<InteractiveProcessRegistry>,
    ) -> Self {
        self.interactive_process_registry = Some(ipr);
        self
    }

    /// Set the Tauri app handle (builder pattern).
    pub fn with_app_handle(mut self, app_handle: AppHandle<R>) -> Self {
        self.app_handle = Some(app_handle);
        self
    }

    /// Run chat resumption, resuming interrupted conversations.
    ///
    /// Skips if execution is paused. For TaskExecution/Review contexts,
    /// skips if the task is in an AGENT_ACTIVE_STATUS (handled by StartupJobRunner).
    pub async fn run(&self) {
        info!("[CHAT_RESUMPTION] ChatResumptionRunner::run() called");

        // 1. Skip if paused
        if self.execution_state.is_paused() {
            info!("[CHAT_RESUMPTION] Execution paused, skipping chat resumption");
            return;
        }

        // 2. Get interrupted conversations
        let interrupted = match self.agent_run_repo.get_interrupted_conversations().await {
            Ok(convs) => convs,
            Err(e) => {
                warn!(error = %e, "[CHAT_RESUMPTION] Failed to get interrupted conversations");
                return;
            }
        };

        if interrupted.is_empty() {
            info!("[CHAT_RESUMPTION] No interrupted conversations to resume");
            return;
        }

        info!(
            count = interrupted.len(),
            "[CHAT_RESUMPTION] Found interrupted conversations"
        );

        // 3. Sort by priority
        let sorted = self.prioritize_resumptions(interrupted);

        // 4. Resume each (skip if handled by task resumption)
        let mut resumed = 0u32;
        for conv in sorted {
            if self.is_handled_by_task_resumption(&conv).await {
                info!(
                    conversation_id = conv.conversation.id.as_str(),
                    context_type = %conv.conversation.context_type,
                    "[CHAT_RESUMPTION] Skipping - handled by task resumption"
                );
                continue;
            }

            info!(
                conversation_id = conv.conversation.id.as_str(),
                context_type = %conv.conversation.context_type,
                context_id = %conv.conversation.context_id,
                "[CHAT_RESUMPTION] Resuming conversation"
            );

            // Create ChatService and send resume message
            let chat_service = self.create_chat_service();
            match chat_service
                .send_message(
                    conv.conversation.context_type,
                    &conv.conversation.context_id,
                    "Continue where you left off.",
                    Default::default(),
                )
                .await
            {
                Ok(_result) => {
                    info!(
                        conversation_id = conv.conversation.id.as_str(),
                        "[CHAT_RESUMPTION] Successfully resumed conversation"
                    );
                    resumed += 1;
                }
                Err(e) => {
                    warn!(
                        conversation_id = conv.conversation.id.as_str(),
                        error = %e,
                        "[CHAT_RESUMPTION] Failed to resume conversation"
                    );
                }
            }
        }

        info!(
            count = resumed,
            "[CHAT_RESUMPTION] Chat resumption complete"
        );
    }

    /// Sort interrupted conversations by priority.
    ///
    /// Priority order: TaskExecution > Review > Task > Ideation > Project
    fn prioritize_resumptions(
        &self,
        mut conversations: Vec<InterruptedConversation>,
    ) -> Vec<InterruptedConversation> {
        conversations.sort_by_key(|conv| context_type_priority(conv.conversation.context_type));
        conversations
    }

    /// Check if this conversation is handled by StartupJobRunner.
    ///
    /// TaskExecution and Review contexts with tasks in AGENT_ACTIVE_STATUSES
    /// are already handled by StartupJobRunner via entry actions.
    async fn is_handled_by_task_resumption(&self, conv: &InterruptedConversation) -> bool {
        match conv.conversation.context_type {
            ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge => {
                // Check if the task is in an agent-active status
                let task_id = TaskId::from_string(conv.conversation.context_id.clone());
                match self.task_repo.get_by_id(&task_id).await {
                    Ok(Some(task)) => {
                        let is_agent_active = AGENT_ACTIVE_STATUSES.contains(&task.internal_status);
                        if is_agent_active {
                            info!(
                                task_id = task.id.as_str(),
                                status = ?task.internal_status,
                                "[CHAT_RESUMPTION] Task in agent-active status, handled by StartupJobRunner"
                            );
                            return true;
                        }
                        if task.internal_status.is_terminal() {
                            info!(
                                task_id = task.id.as_str(),
                                status = ?task.internal_status,
                                "[CHAT_RESUMPTION] Task in terminal state, skipping"
                            );
                            return true;
                        }
                        false
                    }
                    Ok(None) => {
                        // Task doesn't exist, skip this conversation
                        warn!(
                            task_id = %task_id,
                            "[CHAT_RESUMPTION] Task not found, skipping conversation"
                        );
                        true // Treat as "handled" to skip it
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            "[CHAT_RESUMPTION] Failed to get task, skipping conversation"
                        );
                        true // Treat as "handled" to skip it on error
                    }
                }
            }
            // Ideation is handled by the dedicated recovery loop (Phase N+1 in StartupJobRunner),
            // which provides stagger, priority ordering, and 24-hour cutoff that this runner lacks.
            // ChatResumptionRunner must unconditionally skip ideation to prevent double-spawn.
            ChatContextType::Ideation => true,
            // Other context types are not handled by StartupJobRunner
            ChatContextType::Delegation | ChatContextType::Task | ChatContextType::Project => false,
        }
    }

    /// Create a ChatService instance for resumption.
    fn create_chat_service(&self) -> AppChatService<R> {
        let deps = self.chat_runtime_deps.clone().with_runtime_support(
            self.execution_settings_repo.as_ref().map(Arc::clone),
            self.agent_lane_settings_repo.as_ref().map(Arc::clone),
            self.agent_provider_settings_repo.as_ref().map(Arc::clone),
            self.plan_branch_repo.as_ref().map(Arc::clone),
            self.interactive_process_registry.as_ref().map(Arc::clone),
        );
        build_chat_service_with_fallback(
            &self.app_handle,
            Some(Arc::clone(&self.execution_state)),
            &deps,
        )
    }
}

/// Get priority value for a context type (lower = higher priority).
fn context_type_priority(context_type: ChatContextType) -> u8 {
    match context_type {
        ChatContextType::TaskExecution => 0, // Highest priority
        ChatContextType::Review => 1,
        ChatContextType::Merge => 2, // Same priority as review (agent-active)
        ChatContextType::Task => 3,
        ChatContextType::Ideation => 4,
        ChatContextType::Delegation => 5,
        ChatContextType::Project => 6, // Lowest priority
    }
}

#[cfg(test)]
#[path = "chat_resumption_tests.rs"]
mod tests;
