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
use tracing::{info, warn};

use crate::application::agent_workspace_continuation::{
    classify_agent_workspace_continuation_with_plan_branch, AgentWorkspaceContinuationBlock,
};
use crate::application::chat_service::{
    should_recover_silent_completion, silent_completion_recovery_attempt,
    silent_completion_recovery_backoff_ms, silent_completion_recovery_max_attempts,
    silent_completion_recovery_metadata, silent_completion_recovery_prompt,
    team_intent_for_persisted_coordination_mode, SendCallerContext, SendMessageOptions,
};
use crate::application::interactive_process_registry::InteractiveProcessKey;
use crate::application::runtime_factory::{build_chat_service_from_deps, ChatRuntimeFactoryDeps};
use crate::application::{AppChatService, ChatService, InteractiveProcessRegistry};
use crate::application::execution_state::{ExecutionState, AGENT_ACTIVE_STATUSES};
use crate::domain::entities::{
    AgentRunStatus, ChatContextType, ChatConversation, ChatMessage, InterruptedConversation,
    MessageRole, TaskId,
};
use crate::domain::repositories::{
    AgentLaneSettingsRepository, AgentProviderSettingsRepository, AgentRunRepository,
    AutomationRunRepository, ExecutionSettingsRepository, PlanBranchRepository, TaskRepository,
};
use crate::domain::services::RunningAgentKey;
use crate::infrastructure::agents::claude::{ContentBlockItem, ToolCall};

const DURABLE_SILENT_COMPLETION_RECOVERY_SCAN_LIMIT: u32 = 100;
const DURABLE_SILENT_COMPLETION_RECOVERY_MESSAGE_LIMIT: u32 = 20;

/// Runs chat resumption on startup.
///
/// Finds all conversations that were interrupted when the app shut down
/// and resumes them by sending a message with `--resume` to continue the provider session.
pub struct ChatResumptionRunner {
    agent_run_repo: Arc<dyn AgentRunRepository>,
    automation_run_repo: Arc<dyn AutomationRunRepository>,
    chat_runtime_deps: ChatRuntimeFactoryDeps,
    task_repo: Arc<dyn TaskRepository>,
    execution_state: Arc<ExecutionState>,
    execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    managed_team_barrier: Option<Arc<crate::application::managed_team::ManagedTeamStartupBarrier>>,
}

impl ChatResumptionRunner {
    /// Create a new ChatResumptionRunner with all required dependencies.
    pub(crate) fn new(
        agent_run_repo: Arc<dyn AgentRunRepository>,
        automation_run_repo: Arc<dyn AutomationRunRepository>,
        task_repo: Arc<dyn TaskRepository>,
        execution_state: Arc<ExecutionState>,
        chat_runtime_deps: ChatRuntimeFactoryDeps,
    ) -> Self {
        debug_assert!(
            Arc::ptr_eq(&automation_run_repo, &chat_runtime_deps.automation_run_repo,),
            "chat resumption automation repository must match runtime factory dependencies"
        );
        Self {
            agent_run_repo,
            automation_run_repo,
            chat_runtime_deps,
            task_repo,
            execution_state,
            execution_settings_repo: None,
            agent_lane_settings_repo: None,
            agent_provider_settings_repo: None,
            plan_branch_repo: None,
            interactive_process_registry: None,
            managed_team_barrier: None,
        }
    }

    /// Set the managed-Team startup barrier (builder pattern). Without it,
    /// Team conversations are resumed like ordinary conversations.
    pub fn with_managed_team_barrier(
        mut self,
        barrier: Arc<crate::application::managed_team::ManagedTeamStartupBarrier>,
    ) -> Self {
        self.managed_team_barrier = Some(barrier);
        self
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

        let mut resumed = 0u32;
        if interrupted.is_empty() {
            info!("[CHAT_RESUMPTION] No interrupted conversations to resume");
        } else {
            info!(
                count = interrupted.len(),
                "[CHAT_RESUMPTION] Found interrupted conversations"
            );

            // 3. Sort by priority
            let sorted = self.prioritize_resumptions(interrupted);

            // 4. Resume each (skip if handled by task resumption)
            for conv in sorted {
                if !self
                    .is_non_automation_resume_candidate(&conv.conversation)
                    .await
                {
                    continue;
                }
                if self.is_handled_by_task_resumption(&conv).await {
                    info!(
                        conversation_id = conv.conversation.id.as_str(),
                        context_type = %conv.conversation.context_type,
                        "[CHAT_RESUMPTION] Skipping - handled by task resumption"
                    );
                    continue;
                }
                if let Some(barrier) = self.managed_team_barrier.as_ref() {
                    if barrier
                        .should_fence_resumption(
                            conv.conversation.coordination_mode,
                            &conv.conversation.id,
                        )
                        .await
                    {
                        info!(
                            conversation_id = conv.conversation.id.as_str(),
                            "[CHAT_RESUMPTION] Skipping - fenced by managed-Team startup barrier"
                        );
                        continue;
                    }
                }
                if let Some(reason) = self
                    .blocked_agent_workspace_resume_reason(&conv.conversation)
                    .await
                {
                    info!(
                        conversation_id = conv.conversation.id.as_str(),
                        context_type = %conv.conversation.context_type,
                        context_id = %conv.conversation.context_id,
                        reason = reason.code(),
                        "[CHAT_RESUMPTION] Skipping non-resumable agent workspace conversation"
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
                        startup_resumption_send_options(&conv.conversation),
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
        }

        let durable_recovered = self.recover_durable_silent_completions().await;

        info!(
            interrupted_resumed = resumed,
            durable_silent_recovered = durable_recovered,
            "[CHAT_RESUMPTION] Chat resumption complete"
        );
    }

    async fn recover_durable_silent_completions(&self) -> u32 {
        let mut conversations = match self
            .chat_runtime_deps
            .conversation_repo
            .list_recent_resumable_by_context_type(
                ChatContextType::Project,
                DURABLE_SILENT_COMPLETION_RECOVERY_SCAN_LIMIT,
            )
            .await
        {
            Ok(conversations) => conversations,
            Err(error) => {
                warn!(
                    error = %error,
                    "[CHAT_RESUMPTION] Failed to list durable silent-completion recovery candidates"
                );
                return 0;
            }
        };
        match self
            .chat_runtime_deps
            .conversation_repo
            .list_recent_resumable_by_context_type(
                ChatContextType::Standalone,
                DURABLE_SILENT_COMPLETION_RECOVERY_SCAN_LIMIT,
            )
            .await
        {
            Ok(standalone) => conversations.extend(standalone),
            Err(error) => {
                warn!(error = %error, "[CHAT_RESUMPTION] Failed to list standalone durable silent-completion recovery candidates");
            }
        }

        let mut recovered = 0u32;
        for conversation in conversations {
            if !self.is_non_automation_resume_candidate(&conversation).await {
                continue;
            }
            let runtime_context_id = conversation.id.as_str();
            if self
                .has_active_runtime_for_context(conversation.context_type, &runtime_context_id)
                .await
            {
                info!(
                    conversation_id = conversation.id.as_str(),
                    "[CHAT_RESUMPTION] Skipping durable silent-completion recovery; runtime already active"
                );
                continue;
            }
            if let Some(reason) = self
                .blocked_agent_workspace_resume_reason(&conversation)
                .await
            {
                info!(
                    conversation_id = conversation.id.as_str(),
                    reason = reason.code(),
                    "[CHAT_RESUMPTION] Skipping durable silent-completion recovery for non-resumable agent workspace"
                );
                continue;
            }

            let latest_run = match self
                .agent_run_repo
                .get_latest_for_conversation(&conversation.id)
                .await
            {
                Ok(Some(run)) => run,
                Ok(None) => continue,
                Err(error) => {
                    warn!(
                        conversation_id = conversation.id.as_str(),
                        error = %error,
                        "[CHAT_RESUMPTION] Failed to load latest run for durable silent-completion recovery"
                    );
                    continue;
                }
            };

            let messages = match self
                .chat_runtime_deps
                .chat_message_repo
                .get_recent_by_conversation_paginated(
                    &conversation.id,
                    DURABLE_SILENT_COMPLETION_RECOVERY_MESSAGE_LIMIT,
                    0,
                )
                .await
            {
                Ok(messages) => messages,
                Err(error) => {
                    warn!(
                        conversation_id = conversation.id.as_str(),
                        error = %error,
                        "[CHAT_RESUMPTION] Failed to load messages for durable silent-completion recovery"
                    );
                    continue;
                }
            };

            let queued_recovery_exists = self
                .chat_runtime_deps
                .message_queue
                .get_queued(conversation.context_type, &runtime_context_id)
                .iter()
                .any(|queued| {
                    silent_completion_recovery_attempt(queued.metadata_override.as_deref()) > 0
                });
            let decision = durable_silent_completion_recovery_decision(
                conversation.context_type,
                conversation.provider_session_ref().is_some(),
                latest_run.status,
                &messages,
                queued_recovery_exists,
            );
            let DurableSilentCompletionRecoveryDecision::Recover {
                attempt,
                metadata,
                prompt,
            } = decision
            else {
                match decision {
                    DurableSilentCompletionRecoveryDecision::AlreadyQueued => info!(
                        conversation_id = conversation.id.as_str(),
                        "[CHAT_RESUMPTION] Skipping durable silent-completion recovery; recovery already queued"
                    ),
                    DurableSilentCompletionRecoveryDecision::Exhausted { attempts } => warn!(
                        conversation_id = conversation.id.as_str(),
                        attempts,
                        "[CHAT_RESUMPTION] Durable silent-completion recovery attempts exhausted"
                    ),
                    DurableSilentCompletionRecoveryDecision::NotNeeded
                    | DurableSilentCompletionRecoveryDecision::Recover { .. } => {}
                }
                continue;
            };

            let chat_service = self.create_chat_service();
            match chat_service
                .send_message(
                    conversation.context_type,
                    &conversation.context_id,
                    &prompt,
                    durable_silent_completion_recovery_send_options(&conversation, metadata),
                )
                .await
            {
                Ok(_result) => {
                    recovered += 1;
                    warn!(
                        conversation_id = conversation.id.as_str(),
                        attempt,
                        max_attempts = silent_completion_recovery_max_attempts(),
                        "[CHAT_RESUMPTION] Started durable silent-completion recovery"
                    );
                }
                Err(error) => {
                    warn!(
                        conversation_id = conversation.id.as_str(),
                        error = %error,
                        "[CHAT_RESUMPTION] Failed to start durable silent-completion recovery"
                    );
                }
            }
        }
        recovered
    }

    async fn is_non_automation_resume_candidate(&self, conversation: &ChatConversation) -> bool {
        if conversation.automation_id.is_some() {
            info!(
                conversation_id = conversation.id.as_str(),
                "[CHAT_RESUMPTION] Skipping automation-owned conversation; recovery is scheduler-owned"
            );
            return false;
        }

        match self
            .automation_run_repo
            .find_run_by_conversation_id(&conversation.id)
            .await
        {
            Ok(Some(_)) => {
                info!(
                    conversation_id = conversation.id.as_str(),
                    "[CHAT_RESUMPTION] Skipping automation-owned conversation found by run lookup; recovery is scheduler-owned"
                );
                false
            }
            Ok(None) => true,
            Err(error) => {
                warn!(
                    conversation_id = conversation.id.as_str(),
                    error = %error,
                    "[CHAT_RESUMPTION] Failed to determine automation ownership; skipping candidate"
                );
                false
            }
        }
    }

    async fn blocked_agent_workspace_resume_reason(
        &self,
        conversation: &ChatConversation,
    ) -> Option<AgentWorkspaceContinuationBlock> {
        if conversation.context_type != ChatContextType::Project {
            return None;
        }
        let workspace_repo = self
            .chat_runtime_deps
            .agent_conversation_workspace_repo
            .as_ref()?;
        let workspace = match workspace_repo
            .get_by_conversation_id(&conversation.id)
            .await
        {
            Ok(Some(workspace)) => workspace,
            Ok(None) => return None,
            Err(error) => {
                warn!(
                    conversation_id = conversation.id.as_str(),
                    error = %error,
                    "[CHAT_RESUMPTION] Failed to load agent workspace for resume candidate; skipping"
                );
                return Some(AgentWorkspaceContinuationBlock::UnknownRequiresManualCheck(
                    error.to_string(),
                ));
            }
        };
        let project = match self
            .chat_runtime_deps
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
        {
            Ok(Some(project)) => project,
            Ok(None) => {
                warn!(
                    conversation_id = conversation.id.as_str(),
                    project_id = workspace.project_id.as_str(),
                    "[CHAT_RESUMPTION] Agent workspace project not found for resume candidate; skipping"
                );
                return Some(AgentWorkspaceContinuationBlock::UnknownRequiresManualCheck(
                    "project not found".to_string(),
                ));
            }
            Err(error) => {
                warn!(
                    conversation_id = conversation.id.as_str(),
                    error = %error,
                    "[CHAT_RESUMPTION] Failed to load agent workspace project for resume candidate; skipping"
                );
                return Some(AgentWorkspaceContinuationBlock::UnknownRequiresManualCheck(
                    error.to_string(),
                ));
            }
        };

        classify_agent_workspace_continuation_with_plan_branch(
            &project,
            &workspace,
            self.plan_branch_repo.as_deref(),
        )
        .await
        .blocked_reason()
        .cloned()
    }

    async fn has_active_runtime_for_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> bool {
        if self
            .chat_runtime_deps
            .running_agent_registry
            .is_running(&RunningAgentKey::new(context_type.to_string(), context_id))
            .await
        {
            return true;
        }
        if let Some(registry) = self.interactive_process_registry.as_ref() {
            return registry
                .has_process(&InteractiveProcessKey::new(
                    context_type.to_string(),
                    context_id,
                ))
                .await;
        }
        false
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
            ChatContextType::TaskExecution
            | ChatContextType::Review
            | ChatContextType::Merge
            | ChatContextType::BranchUpdate => {
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
            ChatContextType::Delegation
            | ChatContextType::Task
            | ChatContextType::Project
            | ChatContextType::Standalone => false,
        }
    }

    /// Create a ChatService instance for resumption.
    fn create_chat_service(&self) -> AppChatService {
        let deps = self.chat_runtime_deps.clone().with_runtime_support(
            self.execution_settings_repo.as_ref().map(Arc::clone),
            self.agent_lane_settings_repo.as_ref().map(Arc::clone),
            self.agent_provider_settings_repo.as_ref().map(Arc::clone),
            self.plan_branch_repo.as_ref().map(Arc::clone),
            self.interactive_process_registry.as_ref().map(Arc::clone),
        );
        build_chat_service_from_deps(Some(Arc::clone(&self.execution_state)), &deps)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DurableSilentCompletionRecoveryDecision {
    NotNeeded,
    AlreadyQueued,
    Exhausted {
        attempts: u32,
    },
    Recover {
        attempt: u32,
        metadata: String,
        prompt: String,
    },
}

fn durable_silent_completion_recovery_decision(
    context_type: ChatContextType,
    has_session_for_queue: bool,
    latest_run_status: AgentRunStatus,
    messages: &[ChatMessage],
    queued_recovery_exists: bool,
) -> DurableSilentCompletionRecoveryDecision {
    if queued_recovery_exists {
        return DurableSilentCompletionRecoveryDecision::AlreadyQueued;
    }
    if latest_run_status != AgentRunStatus::Completed {
        return DurableSilentCompletionRecoveryDecision::NotNeeded;
    }

    let Some(assistant_message) = latest_assistant_message(messages) else {
        return DurableSilentCompletionRecoveryDecision::NotNeeded;
    };
    let tool_calls = parse_tool_calls(assistant_message.tool_calls.as_deref());
    let content_blocks = parse_content_blocks(assistant_message.content_blocks.as_deref());
    if !should_recover_silent_completion(
        context_type,
        &assistant_message.content,
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        has_session_for_queue,
    ) {
        return DurableSilentCompletionRecoveryDecision::NotNeeded;
    }

    let prior_metadata = latest_silent_completion_recovery_metadata(messages);
    let prior_attempt = silent_completion_recovery_attempt(prior_metadata);
    if prior_attempt >= silent_completion_recovery_max_attempts() {
        return DurableSilentCompletionRecoveryDecision::Exhausted {
            attempts: prior_attempt,
        };
    }

    let attempt = prior_attempt + 1;
    let backoff_ms = silent_completion_recovery_backoff_ms(attempt);
    DurableSilentCompletionRecoveryDecision::Recover {
        attempt,
        metadata: silent_completion_recovery_metadata(attempt, backoff_ms),
        prompt: silent_completion_recovery_prompt(attempt),
    }
}

fn latest_assistant_message(messages: &[ChatMessage]) -> Option<&ChatMessage> {
    messages.iter().rev().find(|message| {
        matches!(
            message.role,
            MessageRole::Orchestrator
                | MessageRole::Worker
                | MessageRole::Reviewer
                | MessageRole::Merger
        )
    })
}

fn latest_silent_completion_recovery_metadata(messages: &[ChatMessage]) -> Option<&str> {
    messages
        .iter()
        .rev()
        .filter_map(|message| message.metadata.as_deref())
        .find(|metadata| silent_completion_recovery_attempt(Some(metadata)) > 0)
}

fn parse_tool_calls(raw: Option<&str>) -> Vec<ToolCall> {
    raw.and_then(|value| serde_json::from_str::<Vec<ToolCall>>(value).ok())
        .unwrap_or_default()
}

fn parse_content_blocks(raw: Option<&str>) -> Vec<ContentBlockItem> {
    raw.and_then(|value| serde_json::from_str::<Vec<ContentBlockItem>>(value).ok())
        .unwrap_or_default()
}

/// Get priority value for a context type (lower = higher priority).
fn context_type_priority(context_type: ChatContextType) -> u8 {
    match context_type {
        ChatContextType::TaskExecution => 0, // Highest priority
        ChatContextType::Review => 1,
        ChatContextType::Merge => 2, // Same priority as review (agent-active)
        ChatContextType::BranchUpdate => 2,
        ChatContextType::Task => 3,
        ChatContextType::Ideation => 4,
        ChatContextType::Delegation => 5,
        ChatContextType::Project => 6, // Lowest priority
        ChatContextType::Standalone => 6,
    }
}

fn startup_resumption_send_options(conversation: &ChatConversation) -> SendMessageOptions {
    SendMessageOptions {
        conversation_id_override: Some(conversation.id),
        team_intent: team_intent_for_persisted_coordination_mode(conversation.coordination_mode),
        caller_context: SendCallerContext::StartupResumption,
        ..Default::default()
    }
}

fn durable_silent_completion_recovery_send_options(
    conversation: &ChatConversation,
    metadata: String,
) -> SendMessageOptions {
    SendMessageOptions {
        metadata: Some(metadata),
        conversation_id_override: Some(conversation.id),
        team_intent: team_intent_for_persisted_coordination_mode(conversation.coordination_mode),
        caller_context: SendCallerContext::StartupResumption,
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "chat_resumption_tests.rs"]
mod tests;
