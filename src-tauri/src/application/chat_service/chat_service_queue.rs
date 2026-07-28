// Message Queue Processing
//
// Handles queued messages that were sent while an agent was running.
// These messages are automatically processed via --resume after the initial run completes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::chat_service_context;
use super::chat_service_helpers::get_assistant_role;
use super::chat_service_streaming::{
    persist_message_text_timeline_item, process_stream_background,
};
use super::chat_service_types::{
    AgentErrorPayload, AgentMessageCreatedPayload, AgentQueueSentPayload, AgentRunStartedPayload,
};
use super::has_meaningful_output;
use super::{
    persona_resolve_flags_for_conversation, team_intent_for_persisted_coordination_mode,
    ChatService, SendMessageOptions,
};
use crate::application::integration_reference_expansion::{
    expand_integration_references_for_prompt, log_skipped_integration_references,
};
use crate::application::persona_resolver::resolve_persona_for_send;
use crate::application::question_state::QuestionState;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::agents::{
    default_effort_for_provider, default_model_for_provider, AgentHarnessKind,
    AgentProviderSettings, LogicalEffort as AgentLogicalEffort, ManualRoleRuntimeOverride,
    ManualServiceTier,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunId,
    ChatContextType, ChatConversation, ChatConversationId, ChatMessageId, CoordinationMode,
    IdeationSessionId, InternalStatus, MessageRole, Persona, PersonaDirective, ProjectId,
    SessionPurpose, TaskId, TeamIntent,
};
use crate::domain::repositories::{
    ActivityEventRepository, AgentProviderSettingsRepository, AgentRunRepository,
    ArtifactRepository, ChatMessageRepository, ChatTimelineRepository, IdeationSessionRepository,
    QueuedMessageRepository, TaskRepository,
};
use crate::domain::services::{
    AttachProcessResult, MessageQueue, QueueKey, QueuedMessage, RunningAgentKey,
    RunningAgentRegistry, TryRegisterError,
};
use crate::utils::secret_redactor::redact;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
pub(super) struct QueueProcessingOutcome {
    pub total_processed: u32,
    pub last_run_id: Option<String>,
}

pub(super) struct CompleteRuntimeQueueSnapshot {
    pub harness: AgentHarnessKind,
    pub model: Option<String>,
    pub effort: Option<AgentLogicalEffort>,
    pub service_tier: Option<String>,
}

pub(super) fn resolve_complete_runtime_for_queue(
    runtime: &ManualRoleRuntimeOverride,
    provider: &AgentProviderSettings,
) -> CompleteRuntimeQueueSnapshot {
    let service_tier = match runtime.service_tier {
        ManualServiceTier::ProviderDefault => Some(
            provider
                .service_tier
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("standard")
                .to_ascii_lowercase(),
        ),
        ManualServiceTier::Standard => Some("standard".to_string()),
        ManualServiceTier::Fast => Some("fast".to_string()),
    };
    CompleteRuntimeQueueSnapshot {
        harness: runtime.harness,
        model: Some(
            runtime
                .model
                .clone()
                .or_else(|| provider.model.clone())
                .unwrap_or_else(|| default_model_for_provider(runtime.harness).to_string()),
        ),
        effort: Some(
            runtime
                .effort
                .or(provider.effort)
                .unwrap_or_else(|| default_effort_for_provider(runtime.harness)),
        ),
        service_tier,
    }
}

impl QueueProcessingOutcome {
    pub(super) fn terminal_run_id(&self, fallback_run_id: &str) -> String {
        self.last_run_id
            .clone()
            .unwrap_or_else(|| fallback_run_id.to_string())
    }
}

async fn durable_queue_len(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    key: &QueueKey,
) -> usize {
    match queued_message_repo {
        Some(repo) => repo
            .list(key)
            .await
            .map(|messages| messages.len())
            .unwrap_or_else(|error| {
                tracing::warn!(
                    error = %error,
                    context_type = %key.context_type,
                    context_id = %key.context_id,
                    "[QUEUE] Failed to list durable queued messages"
                );
                0
            }),
        None => 0,
    }
}

async fn provider_env_for_harness<R: Runtime>(
    app_handle: Option<&AppHandle<R>>,
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    harness: AgentHarnessKind,
) -> Result<HashMap<String, String>, String> {
    let app_state_provider_repo = app_handle
        .and_then(|handle| handle.try_state::<AppState>())
        .map(|app_state| Arc::clone(&app_state.agent_provider_settings_repo));
    let provider_repo = agent_provider_settings_repo
        .as_ref()
        .map(Arc::clone)
        .or(app_state_provider_repo);
    crate::application::provider_env_file::load_provider_custom_env_file_for_harness(
        provider_repo.as_ref(),
        harness,
    )
    .await
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum QueueProviderDecision {
    ApplyEnv(HashMap<String, String>),
    AllowWithoutProviderSettings,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum QueueProviderBlock {
    Disabled(String),
    Env(String),
    MissingProviderSettings,
}

fn queue_missing_provider_settings_message(context_type: ChatContextType) -> String {
    format!(
        "Provider settings were unavailable for {} runtime; spawn blocked to avoid bypassing disabled-provider policy.",
        context_type
    )
}

fn queue_provider_block_message(
    block: &QueueProviderBlock,
    context_type: ChatContextType,
) -> String {
    match block {
        QueueProviderBlock::Disabled(error) | QueueProviderBlock::Env(error) => error.clone(),
        QueueProviderBlock::MissingProviderSettings => {
            queue_missing_provider_settings_message(context_type)
        }
    }
}

pub(super) async fn queue_provider_decision<R: Runtime>(
    app_handle: Option<&AppHandle<R>>,
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    harness: AgentHarnessKind,
    context_type: ChatContextType,
) -> Result<QueueProviderDecision, QueueProviderBlock> {
    let app_state_provider_repo = app_handle
        .and_then(|handle| handle.try_state::<AppState>())
        .map(|app_state| Arc::clone(&app_state.agent_provider_settings_repo));
    let provider_repo = agent_provider_settings_repo
        .as_ref()
        .map(Arc::clone)
        .or(app_state_provider_repo);
    let Some(provider_repo) = provider_repo else {
        return if super::uses_execution_slot(context_type) {
            Err(QueueProviderBlock::MissingProviderSettings)
        } else {
            Ok(QueueProviderDecision::AllowWithoutProviderSettings)
        };
    };

    crate::application::ensure_provider_spawn_enabled(&provider_repo, harness, "queue_resume")
        .await
        .map_err(QueueProviderBlock::Disabled)?;

    let provider_env =
        provider_env_for_harness(app_handle, &Some(Arc::clone(&provider_repo)), harness)
            .await
            .map_err(QueueProviderBlock::Env)?;

    Ok(QueueProviderDecision::ApplyEnv(provider_env))
}

async fn queue_count(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    message_queue: &MessageQueue,
    key: &QueueKey,
) -> usize {
    let memory = message_queue.get_queued_with_key(key).len();
    if memory > 0 {
        memory
    } else {
        durable_queue_len(queued_message_repo, key).await
    }
}

async fn delete_durable_queued_message(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    key: &QueueKey,
    message_id: &str,
) -> bool {
    match queued_message_repo {
        Some(repo) => match repo.delete(key, message_id).await {
            Ok(deleted) => deleted,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    context_type = %key.context_type,
                    context_id = %key.context_id,
                    queued_message_id = %message_id,
                    "[QUEUE] Failed to delete durable queued message"
                );
                false
            }
        },
        None => false,
    }
}

async fn persist_durable_front(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    key: &QueueKey,
    message: &QueuedMessage,
) {
    if let Some(repo) = queued_message_repo {
        if let Err(error) = repo.enqueue_front(key, message).await {
            tracing::warn!(
                error = %error,
                context_type = %key.context_type,
                context_id = %key.context_id,
                queued_message_id = %message.id,
                "[QUEUE] Failed to restore durable queued message"
            );
        }
    }
}

async fn restore_queue_front(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    message_queue: &MessageQueue,
    key: &QueueKey,
    message: QueuedMessage,
) {
    message_queue.queue_front_existing(key.context_type, key.context_id.clone(), message.clone());
    persist_durable_front(queued_message_repo, key, &message).await;
}

async fn clear_durable_queue(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    key: &QueueKey,
) {
    if let Some(repo) = queued_message_repo {
        if let Err(error) = repo.clear(key).await {
            tracing::warn!(
                error = %error,
                context_type = %key.context_type,
                context_id = %key.context_id,
                "[QUEUE] Failed to clear durable queued messages"
            );
        }
    }
}

async fn pop_next_queued_message(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    message_queue: &MessageQueue,
    key: &QueueKey,
) -> Option<QueuedMessage> {
    if let Some(message) = message_queue.pop_with_key(key) {
        let _ = delete_durable_queued_message(queued_message_repo, key, &message.id).await;
        return Some(message);
    }

    let repo = queued_message_repo?;
    match repo.pop_front(key).await {
        Ok(message) => message,
        Err(error) => {
            tracing::warn!(
                error = %error,
                context_type = %key.context_type,
                context_id = %key.context_id,
                "[QUEUE] Failed to pop durable queued message"
            );
            None
        }
    }
}

pub(super) const HIDDEN_RESUME_IN_PLACE_MARKER_CONTENT: &str =
    "RalphX hidden resume-in-place message was delivered.";

pub(super) fn queue_processing_blocked_by_pause(
    context_type: ChatContextType,
    execution_state: Option<&Arc<ExecutionState>>,
) -> bool {
    super::uses_execution_slot(context_type) && execution_state.is_some_and(|exec| exec.is_paused())
}

pub(super) fn queued_message_resume_in_place(metadata_override: Option<&str>) -> bool {
    metadata_override
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| value.get("resume_in_place").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

fn with_resume_in_place_metadata(metadata_override: Option<String>) -> Option<String> {
    let mut value = metadata_override
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = value.as_object_mut() {
        obj.insert("resume_in_place".to_string(), serde_json::json!(true));
    }
    Some(value.to_string())
}

pub(super) fn hidden_resume_in_place_marker_metadata(
    metadata_override: Option<&str>,
) -> Option<String> {
    let raw = metadata_override?;
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return None;
    };
    let obj = value.as_object_mut()?;
    if obj
        .get("persist_hidden_marker")
        .and_then(|value| value.as_bool())
        != Some(true)
    {
        return None;
    }
    obj.remove("resume_in_place");
    obj.remove("persist_hidden_marker");
    obj.insert("hidden_from_ui".to_string(), serde_json::json!(true));
    obj.insert("recovery_context".to_string(), serde_json::json!(true));
    Some(value.to_string())
}

fn queued_persisted_metadata(
    queued_msg: &crate::domain::services::QueuedMessage,
) -> Option<String> {
    let metadata = queued_msg.metadata_override.clone();
    let excerpt_references = super::chat_service_composer_references::normalize_excerpt_references(
        &queued_msg.composer_excerpt_references,
    );
    if queued_msg.composer_project_references.is_empty()
        && queued_msg.composer_integration_references.is_empty()
        && queued_msg.composer_artifact_references.is_empty()
        && queued_msg.composer_selection_snapshot.is_none()
        && excerpt_references.is_empty()
    {
        return metadata;
    }

    let mut value = match metadata {
        Some(raw) => serde_json::from_str::<serde_json::Value>(&raw)
            .unwrap_or_else(|_| serde_json::json!({ "raw_metadata": raw })),
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    if !value.is_object() {
        value = serde_json::json!({ "metadata": value });
    }
    let Some(object) = value.as_object_mut() else {
        return Some(value.to_string());
    };
    if !queued_msg.composer_project_references.is_empty() {
        let references = serde_json::to_value(&queued_msg.composer_project_references).ok()?;
        object.insert("composer_project_references".to_string(), references);
    }
    if !queued_msg.composer_integration_references.is_empty() {
        let references = serde_json::to_value(&queued_msg.composer_integration_references).ok()?;
        object.insert("composer_integration_references".to_string(), references);
    }
    if !queued_msg.composer_artifact_references.is_empty() {
        let references = serde_json::to_value(&queued_msg.composer_artifact_references).ok()?;
        object.insert("composer_artifact_references".to_string(), references);
    }
    if let Some(snapshot) = queued_msg.composer_selection_snapshot.as_ref() {
        let snapshot = serde_json::to_value(snapshot).ok()?;
        object.insert(
            super::chat_service_selection_snapshot::SELECTION_SNAPSHOT_METADATA_KEY.to_string(),
            snapshot,
        );
    }
    if !excerpt_references.is_empty() {
        let references = serde_json::to_value(&excerpt_references).ok()?;
        object.insert("composer_excerpt_references".to_string(), references);
    }
    Some(value.to_string())
}

pub(super) fn queued_message_requires_fresh_provider_session(
    queued_msg: &crate::domain::services::QueuedMessage,
    current_harness: AgentHarnessKind,
) -> bool {
    queued_msg.force_new_provider_session
        || queued_msg
            .harness_override
            .is_some_and(|queued_harness| queued_harness != current_harness)
}

fn queued_created_at_override(queued_msg: &QueuedMessage) -> Option<chrono::DateTime<chrono::Utc>> {
    queued_msg
        .created_at_override
        .as_deref()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|ts| ts.with_timezone(&chrono::Utc))
}

fn queued_persisted_created_at(
    queued_msg: &QueuedMessage,
) -> Option<chrono::DateTime<chrono::Utc>> {
    queued_created_at_override(queued_msg).or_else(|| {
        chrono::DateTime::parse_from_rfc3339(&queued_msg.created_at)
            .ok()
            .map(|ts| ts.with_timezone(&chrono::Utc))
    })
}

fn provider_switch_send_options_for_queued_message(
    queued_msg: &QueuedMessage,
    conversation_id: ChatConversationId,
    force_new_provider_session: bool,
    team_intent: Option<TeamIntent>,
) -> SendMessageOptions {
    SendMessageOptions {
        metadata: queued_msg.metadata_override.clone(),
        created_at: queued_persisted_created_at(queued_msg),
        harness_override: queued_msg.harness_override,
        agent_name_override: queued_msg.agent_name_override.clone(),
        persona_directive: queued_msg.persona_directive.clone(),
        model_override: queued_msg.model_override.clone(),
        conversation_id_override: Some(conversation_id),
        logical_effort_override: queued_msg.logical_effort_override,
        service_tier_override: queued_msg.service_tier_override.clone(),
        preserve_conversation_provider_session_ref: queued_msg
            .preserve_conversation_provider_session_ref,
        composer_project_references: queued_msg.composer_project_references.clone(),
        composer_integration_references: queued_msg.composer_integration_references.clone(),
        composer_artifact_references: queued_msg.composer_artifact_references.clone(),
        composer_selection_snapshot: queued_msg.composer_selection_snapshot.clone(),
        composer_excerpt_references: queued_msg.composer_excerpt_references.clone(),
        attachment_ids: queued_msg.attachment_ids.clone(),
        team_intent,
        force_new_provider_session,
        ..Default::default()
    }
}

fn queued_target_harness(
    queued_msg: &QueuedMessage,
    fallback_harness: AgentHarnessKind,
) -> AgentHarnessKind {
    queued_msg.harness_override.unwrap_or(fallback_harness)
}

fn can_reuse_fresh_provider_run(
    queued_msg: &QueuedMessage,
    fresh_provider_harness: Option<AgentHarnessKind>,
) -> bool {
    queued_msg.force_new_provider_session
        && queued_msg
            .harness_override
            .is_some_and(|harness| Some(harness) == fresh_provider_harness)
}

async fn persist_hidden_resume_in_place_marker(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: ChatConversationId,
    metadata_override: Option<&str>,
) {
    let Some(marker_metadata) = hidden_resume_in_place_marker_metadata(metadata_override) else {
        return;
    };
    let mut marker = chat_service_context::create_user_message(
        context_type,
        context_id,
        HIDDEN_RESUME_IN_PLACE_MARKER_CONTENT,
        conversation_id,
        Some(marker_metadata),
        None,
    );
    marker.role = MessageRole::System;
    if let Err(error) = chat_message_repo.create(marker).await {
        tracing::warn!(
            error = %error,
            %conversation_id,
            "failed to persist hidden resume-in-place marker"
        );
    }
}

fn build_queued_agent_run(
    conversation_id: ChatConversationId,
    harness: AgentHarnessKind,
    provider_session_id: &str,
    run_chain_id: Option<&str>,
    parent_run_id: Option<&str>,
    metadata: Option<&str>,
    runtime: &super::continuation_runtime::ContinuationRuntime,
    queued_message: &QueuedMessage,
    launch_security: super::conversation_launch_security::ConversationLaunchSecurityClass,
) -> AgentRun {
    let mut run = match (run_chain_id, parent_run_id) {
        (Some(chain_id), Some(parent_id)) => {
            AgentRun::new_continuation(conversation_id, chain_id.to_string(), parent_id.to_string())
        }
        _ => AgentRun::new(conversation_id),
    };
    run.harness = Some(harness);
    run.provider_session_id = Some(provider_session_id.to_string());
    run.logical_model = queued_message
        .model_override
        .clone()
        .or_else(|| runtime.logical_model.clone());
    run.effective_model_id = queued_message
        .model_override
        .clone()
        .or_else(|| runtime.effective_model_id.clone())
        .or_else(|| runtime.logical_model.clone());
    run.logical_effort = queued_message
        .logical_effort_override
        .or(runtime.logical_effort);
    run.effective_effort = run
        .logical_effort
        .map(|effort| effort.to_legacy_claude_effort().to_string());
    run.service_tier = queued_message
        .service_tier_override
        .as_deref()
        .and_then(super::normalize_service_tier_override)
        .or_else(|| runtime.service_tier.clone());
    run.approval_policy = runtime.approval_policy.clone();
    run.sandbox_mode = runtime.sandbox_mode.clone();
    run.apply_action_metadata_json(metadata);
    launch_security.apply_to_agent_run(&mut run);
    run
}

fn build_queued_preflight_failure_run(
    conversation_id: ChatConversationId,
    harness: AgentHarnessKind,
    provider_session_id: &str,
    run_chain_id: Option<&str>,
    parent_run_id: Option<&str>,
    metadata: Option<&str>,
    queued_message: &QueuedMessage,
) -> AgentRun {
    let mut run = match (run_chain_id, parent_run_id) {
        (Some(chain_id), Some(parent_id)) => {
            AgentRun::new_continuation(conversation_id, chain_id.to_string(), parent_id.to_string())
        }
        _ => AgentRun::new(conversation_id),
    };
    run.harness = Some(harness);
    run.provider_session_id = Some(provider_session_id.to_string());
    run.logical_model = queued_message.model_override.clone();
    run.effective_model_id = queued_message.model_override.clone();
    run.logical_effort = queued_message.logical_effort_override;
    run.effective_effort = run
        .logical_effort
        .map(|effort| effort.to_legacy_claude_effort().to_string());
    run.service_tier = queued_message
        .service_tier_override
        .as_deref()
        .and_then(super::normalize_service_tier_override);
    run.apply_action_metadata_json(metadata);
    run
}

async fn persist_failed_queued_run<R: Runtime>(
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    app_handle: Option<&AppHandle<R>>,
    run: AgentRun,
    error: &str,
) -> Option<String> {
    let run_id = run.id.as_str().to_string();
    if let Err(persist_error) = agent_run_repo.create(run).await {
        tracing::error!(
            queued_run_id = %run_id,
            error = %persist_error,
            "Failed to persist queued preflight failure run"
        );
        return None;
    }
    if let Err(persist_error) = agent_run_repo
        .fail(&AgentRunId::from_string(run_id.clone()), error)
        .await
    {
        tracing::error!(
            queued_run_id = %run_id,
            error = %persist_error,
            "Failed to mark queued preflight run failed"
        );
    }
    settle_terminal_queued_plan_verification(app_handle, &run_id).await;
    Some(run_id)
}

fn emit_queued_preflight_error<R: Runtime>(
    app_handle: Option<&AppHandle<R>>,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    agent_run_id: Option<String>,
    error: String,
) {
    if let Some(handle) = app_handle {
        let _ = handle.emit(
            "agent:error",
            AgentErrorPayload {
                conversation_id: Some(conversation_id.as_str().to_string()),
                context_type: context_type.to_string(),
                context_id: context_id.to_string(),
                agent_run_id,
                error,
                stderr: None,
            },
        );
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct QueuedAgentIdentity {
    agent_name: Option<String>,
    agent_profile: Option<&'static str>,
}

#[derive(Debug, Clone, Default)]
struct QueuedAgentContext {
    identity: QueuedAgentIdentity,
    workspace: Option<AgentConversationWorkspace>,
    effective_mode: Option<AgentConversationWorkspaceMode>,
    conversation: Option<ChatConversation>,
    builder_draft: Option<Persona>,
    builder_context_error: Option<String>,
}

fn queued_agent_identity_for_mode(
    mode: Option<AgentConversationWorkspaceMode>,
) -> QueuedAgentIdentity {
    let Some(mode) = mode else {
        return QueuedAgentIdentity::default();
    };

    QueuedAgentIdentity {
        agent_name: Some(super::agent_name_for_conversation_mode(mode).to_string()),
        agent_profile: super::agent_profile_for_conversation_mode(mode),
    }
}

fn queued_agent_identity_for_conversation(
    conversation: Option<&ChatConversation>,
    mode: Option<AgentConversationWorkspaceMode>,
) -> QueuedAgentIdentity {
    if let Some(bound_agent_name) =
        conversation.and_then(|conversation| conversation.bound_agent_name.as_deref())
    {
        return QueuedAgentIdentity {
            agent_name: Some(bound_agent_name.to_string()),
            agent_profile: None,
        };
    }
    queued_agent_identity_for_mode(mode)
}

async fn resolve_queued_agent_context<R: Runtime + 'static>(
    app_handle: Option<&AppHandle<R>>,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
) -> Result<QueuedAgentContext, String> {
    let Some(handle) = app_handle else {
        return if matches!(
            context_type,
            ChatContextType::Project | ChatContextType::Standalone
        ) {
            Err(format!(
                "Queued {} conversation {conversation_id} cannot be validated without app state",
                context_type
            ))
        } else {
            Ok(QueuedAgentContext::default())
        };
    };

    let app_state = handle.state::<AppState>();
    let mut builder_context_error = None;
    let conversation = match app_state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
    {
        Ok(conversation) => conversation,
        Err(error) => {
            return Err(format!(
                "Queued conversation lookup failed for {conversation_id}: {error}"
            ))
        }
    };
    if conversation.is_none()
        && matches!(
            context_type,
            ChatContextType::Project | ChatContextType::Standalone
        )
    {
        return Err(format!(
            "Queued {} conversation {conversation_id} was not found",
            context_type
        ));
    }
    let conversation_mode = conversation
        .as_ref()
        .and_then(|conversation| conversation.agent_mode);
    if let Some(conversation) = conversation.as_ref() {
        let requested_conversation_id = conversation_id.as_str();
        super::conversation_launch_security::validate_conversation_launch_identity(
            conversation,
            requested_conversation_id.as_str(),
            context_type,
            context_id,
        )?;
    }
    if !matches!(
        context_type,
        ChatContextType::Project | ChatContextType::Standalone
    ) {
        return Ok(QueuedAgentContext {
            identity: queued_agent_identity_for_conversation(
                conversation.as_ref(),
                conversation_mode,
            ),
            effective_mode: conversation_mode,
            conversation,
            ..QueuedAgentContext::default()
        });
    }
    let builder_draft = if let Some(draft_id) = conversation
        .as_ref()
        .and_then(|conversation| conversation.builder_draft_id.as_deref())
    {
        match app_state
            .persona_repo
            .get_by_id(&crate::domain::entities::PersonaId::from(draft_id))
            .await
        {
            Ok(Some(draft)) => Some(draft),
            Ok(None) => {
                builder_context_error = Some(format!(
                    "Bound PersonaBuilder draft {draft_id} was not found"
                ));
                None
            }
            Err(error) => {
                builder_context_error =
                    Some(format!("PersonaBuilder draft lookup failed: {error}"));
                None
            }
        }
    } else {
        None
    };
    let workspace = match app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
    {
        Ok(workspace) => workspace,
        Err(error) => {
            tracing::warn!(
                error = %error,
                %conversation_id,
                "[QUEUE] Failed to resolve queued workspace mode"
            );
            None
        }
    };
    let mode = conversation_mode.or_else(|| workspace.as_ref().map(|workspace| workspace.mode));

    Ok(QueuedAgentContext {
        identity: queued_agent_identity_for_conversation(conversation.as_ref(), mode.clone()),
        workspace,
        effective_mode: mode,
        conversation,
        builder_draft,
        builder_context_error,
    })
}

async fn resolve_queue_resume_persona<R: Runtime>(
    app_handle: Option<&AppHandle<R>>,
    feature_enabled: bool,
    context_type: ChatContextType,
    conversation_id: &ChatConversationId,
    directive: &PersonaDirective,
    agent_name_override_set: bool,
) -> Result<Option<crate::application::persona_prompt::ResolvedPersona>, String> {
    if !feature_enabled {
        return Ok(None);
    }

    let Some(handle) = app_handle else {
        return Ok(None);
    };
    let Some(app_state) = handle.try_state::<AppState>() else {
        return Ok(None);
    };
    let conversation = app_state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|error| format!("Persona conversation lookup failed: {error}"))?
        .ok_or_else(|| {
            format!(
                "Persona conversation {} was not found",
                conversation_id.as_str()
            )
        })?;
    let workspace_mode = app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| format!("Persona workspace lookup failed: {error}"))?
        .map(|workspace| workspace.mode);

    resolve_persona_for_send(
        &conversation,
        directive,
        persona_resolve_flags_for_conversation(
            feature_enabled,
            false,
            agent_name_override_set || conversation.bound_agent_name.is_some(),
            context_type,
            &conversation,
            workspace_mode,
        ),
        Arc::clone(&app_state.persona_repo),
    )
    .await
    .map_err(|error| error.to_string())
}

pub(super) async fn settle_terminal_queued_plan_verification<R: Runtime>(
    app_handle: Option<&AppHandle<R>>,
    run_id: &str,
) {
    let Some(state) = app_handle.and_then(|handle| handle.try_state::<AppState>()) else {
        return;
    };
    if let Err(error) = crate::application::plan_approval_notification_service::release_deferred_plan_approval_for_run(
        state.inner(),
        &AgentRunId::from_string(run_id.to_string()),
    )
    .await
    {
        tracing::warn!(error = %error, queued_run_id = run_id, "Failed to release deferred plan approval for terminal queued verification run");
    }
}

async fn fail_queued_agent_run<R: Runtime>(
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    registry_key: &RunningAgentKey,
    app_handle: Option<&AppHandle<R>>,
    run_id: &str,
    error: &str,
) {
    let _ = agent_run_repo
        .fail(&AgentRunId::from_string(run_id.to_string()), error)
        .await;
    running_agent_registry
        .unregister(registry_key, run_id)
        .await;
    settle_terminal_queued_plan_verification(app_handle, run_id).await;
}

async fn reconcile_queued_verification_child_completion<R: Runtime>(
    context_type: ChatContextType,
    context_id: &str,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    message_queue: &Arc<MessageQueue>,
    app_handle: Option<&AppHandle<R>>,
) {
    if context_type != ChatContextType::Ideation {
        return;
    }

    let child_id = IdeationSessionId::from_string(context_id.to_string());
    let child_session = match ideation_session_repo.get_by_id(&child_id).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            tracing::debug!(
                context_id,
                "[QUEUE] Ideation session not found for queued verification reconciliation"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(
                context_id,
                error = %error,
                "[QUEUE] Failed to fetch ideation session for queued verification reconciliation"
            );
            return;
        }
    };

    if child_session.session_purpose != SessionPurpose::Verification {
        return;
    }

    let Some(parent_id) = child_session.parent_session_id else {
        tracing::warn!(
            context_id,
            "[QUEUE] Verification child has no parent for queued completion reconciliation"
        );
        return;
    };

    let Some(handle) = app_handle else {
        tracing::warn!(
            context_id,
            parent_id = %parent_id.as_str(),
            "[QUEUE] Cannot reconcile queued verification child completion without app handle"
        );
        return;
    };

    let app_state = handle.state::<AppState>();
    let verification_child_registry = None;
    super::chat_service_handlers::handle_verification_child_completion(
        &child_id,
        &parent_id,
        ideation_session_repo,
        &app_state.chat_conversation_repo,
        chat_message_repo,
        message_queue,
        &Some(handle.clone()),
        &verification_child_registry,
    )
    .await;
}

/// Process all queued messages for a context with retry loop.
///
/// Returns the total number of messages processed.
///
/// This handles race conditions where messages can be queued while we're processing,
/// so it keeps checking until the queue is stable-empty (50ms late-arrival check).
#[allow(clippy::too_many_arguments)]
pub(super) async fn process_queued_messages<R: Runtime + 'static>(
    context_type: ChatContextType,
    harness: AgentHarnessKind,
    context_id: &str,
    queue_context_id: &str,
    conversation_id: ChatConversationId,
    session_id: &str,
    persona_feature_enabled: bool,
    message_queue: &Arc<MessageQueue>,
    queued_message_repo: Option<Arc<dyn QueuedMessageRepository>>,
    agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_timeline_repo: Option<Arc<dyn ChatTimelineRepository>>,
    chat_attachment_repo: &Arc<dyn crate::domain::repositories::ChatAttachmentRepository>,
    artifact_repo: &Arc<dyn ArtifactRepository>,
    activity_event_repo: &Arc<dyn ActivityEventRepository>,
    task_repo: &Arc<dyn TaskRepository>,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    cli_path: &Path,
    plugin_dir: &Path,
    working_directory: &Path,
    question_state: Option<Arc<QuestionState>>,
    execution_state: Option<Arc<ExecutionState>>,
    app_handle: Option<AppHandle<R>>,
    project_id: Option<&str>,
    conversation_coordination_mode: Option<CoordinationMode>,
    cancellation_token: CancellationToken,
    run_chain_id: Option<&str>,
    parent_run_id: Option<&str>,
    streaming_state_cache: super::StreamingStateCache,
) -> QueueProcessingOutcome {
    let mut total_processed = 0u32;
    let mut last_run_id: Option<String> = None;
    let mut fresh_provider_harness: Option<AgentHarnessKind> = None;
    let queue_key = QueueKey::new(context_type, queue_context_id);
    let queue_team_intent =
        conversation_coordination_mode.and_then(team_intent_for_persisted_coordination_mode);

    // Outer loop: keep processing until queue is stable-empty
    loop {
        if queue_processing_blocked_by_pause(context_type, execution_state.as_ref()) {
            let pending =
                queue_count(queued_message_repo.as_ref(), message_queue, &queue_key).await;
            tracing::info!(
                %context_type,
                context_id,
                queue_context_id,
                pending,
                "[QUEUE] Execution paused, leaving queued messages pending"
            );
            break;
        }

        // Check cancellation before each iteration
        if cancellation_token.is_cancelled() {
            tracing::info!(
                "[QUEUE] Cancellation requested, stopping queue processing after {} messages",
                total_processed
            );
            break;
        }

        let pending_count =
            queue_count(queued_message_repo.as_ref(), message_queue, &queue_key).await;

        if pending_count == 0 {
            // Queue is empty, wait briefly then check once more for race condition
            if total_processed > 0 {
                // We processed messages, give a small window for late arrivals
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let final_count =
                    queue_count(queued_message_repo.as_ref(), message_queue, &queue_key).await;
                if final_count == 0 {
                    tracing::info!(
                        "[QUEUE] Queue processing complete: {} total messages processed",
                        total_processed
                    );
                    break;
                }
                tracing::info!(
                    "[QUEUE] Found {} late-arriving messages, continuing...",
                    final_count
                );
            } else {
                tracing::info!("[QUEUE] No queued messages to process");
                break;
            }
        }

        tracing::info!(
            "[QUEUE] Processing queue: session_id={}, context={}/{}, queue_context_id={}, pending={}",
            session_id,
            context_type,
            context_id,
            queue_context_id,
            pending_count
        );

        // Inner loop: process all currently queued messages
        while let Some(queued_msg) =
            pop_next_queued_message(queued_message_repo.as_ref(), message_queue, &queue_key).await
        {
            if queue_processing_blocked_by_pause(context_type, execution_state.as_ref()) {
                restore_queue_front(
                    queued_message_repo.as_ref(),
                    message_queue,
                    &queue_key,
                    queued_msg,
                )
                .await;
                tracing::info!(
                    %context_type,
                    context_id,
                    queue_context_id,
                    "[QUEUE] Execution paused after dequeue, restored message to queue front"
                );
                break;
            }

            if cancellation_token.is_cancelled() {
                restore_queue_front(
                    queued_message_repo.as_ref(),
                    message_queue,
                    &queue_key,
                    queued_msg,
                )
                .await;
                tracing::info!("[QUEUE] Cancellation requested mid-queue, stopping");
                break;
            }

            if let Some(backoff) =
                super::chat_service_send_background::silent_completion_recovery_backoff(
                    queued_msg.metadata_override.as_deref(),
                )
            {
                tracing::info!(
                    %context_type,
                    context_id,
                    queue_context_id,
                    queued_message_id = %queued_msg.id,
                    backoff_ms = backoff.as_millis(),
                    "[QUEUE] Delaying hidden silent-completion recovery"
                );
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = cancellation_token.cancelled() => {
                        restore_queue_front(
                            queued_message_repo.as_ref(),
                            message_queue,
                            &queue_key,
                            queued_msg,
                        ).await;
                        tracing::info!(
                            %context_type,
                            context_id,
                            queue_context_id,
                            "[QUEUE] Cancellation requested during recovery backoff, restored message to queue front"
                        );
                        break;
                    }
                }
            }

            // Guard: for task execution, verify task is still in Executing/ReExecuting state
            if context_type == ChatContextType::TaskExecution {
                let task_id = TaskId::from_string(context_id.to_string());
                match task_repo.get_by_id(&task_id).await {
                    Ok(Some(task)) => {
                        if task.internal_status != InternalStatus::Executing
                            && task.internal_status != InternalStatus::ReExecuting
                        {
                            let remaining = queue_count(
                                queued_message_repo.as_ref(),
                                message_queue,
                                &queue_key,
                            )
                            .await;
                            tracing::info!(
                                "[QUEUE] Task {} has transitioned to {:?}, draining {} queued messages without spawning",
                                context_id,
                                task.internal_status,
                                remaining + 1,
                            );
                            while message_queue.pop_with_key(&queue_key).is_some() {}
                            clear_durable_queue(queued_message_repo.as_ref(), &queue_key).await;
                            break;
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(
                            "[QUEUE] Task {} not found, draining queued messages",
                            context_id
                        );
                        while message_queue.pop_with_key(&queue_key).is_some() {}
                        clear_durable_queue(queued_message_repo.as_ref(), &queue_key).await;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[QUEUE] Failed to check task state for {}: {}, proceeding cautiously",
                            context_id,
                            e
                        );
                    }
                }
            }

            tracing::info!(
                "[QUEUE] Processing queued message id={}, content_len={}",
                queued_msg.id,
                queued_msg.content.len()
            );

            let resolved_persona = match resolve_queue_resume_persona(
                app_handle.as_ref(),
                persona_feature_enabled,
                context_type,
                &conversation_id,
                &queued_msg.persona_directive,
                queued_msg.agent_name_override.is_some(),
            )
            .await
            {
                Ok(persona) => persona,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        %context_type,
                        context_id,
                        "queue resume persona resolution blocked spawn"
                    );
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit(
                            "agent:error",
                            AgentErrorPayload {
                                conversation_id: Some(conversation_id.as_str().to_string()),
                                context_type: context_type.to_string(),
                                context_id: context_id.to_string(),
                                agent_run_id: None,
                                error,
                                stderr: None,
                            },
                        );
                    }
                    total_processed += 1;
                    continue;
                }
            };

            let queued_agent_context = match resolve_queued_agent_context(
                app_handle.as_ref(),
                context_type,
                context_id,
                &conversation_id,
            )
            .await
            {
                Ok(context) => context,
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        %context_type,
                        context_id,
                        queued_message_id = %queued_msg.id,
                        "[QUEUE] Queued conversation lookup blocked spawn"
                    );
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit(
                            "agent:error",
                            AgentErrorPayload {
                                conversation_id: Some(conversation_id.as_str().to_string()),
                                context_type: context_type.to_string(),
                                context_id: context_id.to_string(),
                                agent_run_id: None,
                                error,
                                stderr: None,
                            },
                        );
                    }
                    total_processed += 1;
                    continue;
                }
            };
            if let Some(conversation) = queued_agent_context.conversation.as_ref() {
                if let Err(error) = super::validate_persona_builder_feature_for_conversation(
                    persona_feature_enabled,
                    conversation,
                ) {
                    let error = error.to_string();
                    tracing::warn!(
                        error,
                        %context_type,
                        context_id,
                        queued_message_id = %queued_msg.id,
                        "queue resume blocked because PersonaBuilder is disabled"
                    );
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit(
                            "agent:error",
                            AgentErrorPayload {
                                conversation_id: Some(conversation_id.as_str().to_string()),
                                context_type: context_type.to_string(),
                                context_id: context_id.to_string(),
                                agent_run_id: None,
                                error,
                                stderr: None,
                            },
                        );
                    }
                    total_processed += 1;
                    continue;
                }
            }
            if let Some(error) = queued_agent_context.builder_context_error.as_ref() {
                tracing::warn!(
                    error,
                    %context_type,
                    context_id,
                    "queue resume blocked because PersonaBuilder context could not be loaded"
                );
                if let Some(ref handle) = app_handle {
                    let _ = handle.emit(
                        "agent:error",
                        AgentErrorPayload {
                            conversation_id: Some(conversation_id.as_str().to_string()),
                            context_type: context_type.to_string(),
                            context_id: context_id.to_string(),
                            agent_run_id: None,
                            error: error.clone(),
                            stderr: None,
                        },
                    );
                }
                total_processed += 1;
                continue;
            }
            let target_harness = queued_target_harness(&queued_msg, harness);

            // Emit queue sent event (removes from frontend optimistic UI)
            if let Some(ref handle) = app_handle {
                let _ = handle.emit(
                    "agent:queue_sent",
                    AgentQueueSentPayload {
                        message_id: queued_msg.id.clone(),
                        conversation_id: conversation_id.as_str().to_string(),
                        context_type: context_type.to_string(),
                        context_id: queue_context_id.to_string(),
                    },
                );
            }
            if queued_message_requires_fresh_provider_session(&queued_msg, harness) {
                let Some(ref handle) = app_handle else {
                    restore_queue_front(
                        queued_message_repo.as_ref(),
                        message_queue,
                        &queue_key,
                        queued_msg,
                    )
                    .await;
                    tracing::warn!(
                        %context_type,
                        context_id,
                        queue_context_id,
                        "[QUEUE] Provider switch queued message requires chat service replay but no app handle is available"
                    );
                    return QueueProcessingOutcome {
                        total_processed,
                        last_run_id,
                    };
                };

                let app_state = handle.state::<AppState>();
                let service = app_state.build_chat_service_for_runtime(
                    execution_state.as_ref().map(Arc::clone),
                    Some(handle.clone()),
                );
                let force_new_provider_session =
                    !can_reuse_fresh_provider_run(&queued_msg, fresh_provider_harness);
                let send_result = service
                    .send_message(
                        context_type,
                        context_id,
                        &queued_msg.content,
                        provider_switch_send_options_for_queued_message(
                            &queued_msg,
                            conversation_id.clone(),
                            force_new_provider_session,
                            queue_team_intent.clone(),
                        ),
                    )
                    .await;

                match send_result {
                    Ok(result) => {
                        total_processed += 1;
                        if !result.agent_run_id.is_empty() {
                            last_run_id = Some(result.agent_run_id.clone());
                        }
                        if !result.was_queued {
                            fresh_provider_harness = Some(target_harness);
                        }
                        tracing::info!(
                            %context_type,
                            context_id,
                            queue_context_id,
                            queued_message_id = %queued_msg.id,
                            agent_run_id = %result.agent_run_id,
                            was_queued = result.was_queued,
                            force_new_provider_session,
                            "[QUEUE] Replayed provider-switch queued message through chat service"
                        );
                        if result.was_queued {
                            return QueueProcessingOutcome {
                                total_processed,
                                last_run_id,
                            };
                        }
                        continue;
                    }
                    Err(error) => {
                        let error_string = error.to_string();
                        tracing::error!(
                            %context_type,
                            context_id,
                            queue_context_id,
                            queued_message_id = %queued_msg.id,
                            error = %error_string,
                            "[QUEUE] Failed to replay provider-switch queued message"
                        );
                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                "agent:error",
                                AgentErrorPayload {
                                    conversation_id: Some(conversation_id.as_str().to_string()),
                                    context_type: context_type.to_string(),
                                    context_id: context_id.to_string(),
                                    agent_run_id: None,
                                    error: error_string,
                                    stderr: None,
                                },
                            );
                        }
                        total_processed += 1;
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                }
            }

            total_processed += 1;

            let continuation_runtime =
                match super::continuation_runtime::resolve_for_provider_session(
                    agent_run_repo,
                    &conversation_id,
                    harness,
                    session_id,
                )
                .await
                {
                    Ok(Some(runtime)) => runtime,
                    Ok(None) => {
                        let error = format!(
                            "No completed {harness} run owns provider session {session_id}; queued continuation blocked"
                        );
                        tracing::error!(
                            %conversation_id,
                            %harness,
                            provider_session_id = session_id,
                            "{error}"
                        );
                        let failed_run = build_queued_preflight_failure_run(
                            conversation_id.clone(),
                            harness,
                            session_id,
                            run_chain_id,
                            parent_run_id,
                            queued_msg.metadata_override.as_deref(),
                            &queued_msg,
                        );
                        let failed_run_id = persist_failed_queued_run(
                            agent_run_repo,
                            app_handle.as_ref(),
                            failed_run,
                            &error,
                        )
                        .await;
                        emit_queued_preflight_error(
                            app_handle.as_ref(),
                            &conversation_id,
                            context_type,
                            context_id,
                            failed_run_id.clone(),
                            error,
                        );
                        last_run_id = failed_run_id.or(last_run_id);
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                    Err(error) => {
                        let error = format!(
                            "Failed to resolve runtime for queued {harness} provider session {session_id}: {error}"
                        );
                        tracing::error!(
                            %conversation_id,
                            %harness,
                            provider_session_id = session_id,
                            error = %error,
                            "Failed to resolve queued continuation runtime"
                        );
                        let failed_run = build_queued_preflight_failure_run(
                            conversation_id.clone(),
                            harness,
                            session_id,
                            run_chain_id,
                            parent_run_id,
                            queued_msg.metadata_override.as_deref(),
                            &queued_msg,
                        );
                        let failed_run_id = persist_failed_queued_run(
                            agent_run_repo,
                            app_handle.as_ref(),
                            failed_run,
                            &error,
                        )
                        .await;
                        emit_queued_preflight_error(
                            app_handle.as_ref(),
                            &conversation_id,
                            context_type,
                            context_id,
                            failed_run_id.clone(),
                            error,
                        );
                        last_run_id = failed_run_id.or(last_run_id);
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                };
            let (launch_context_type, launch_context_id) = queued_agent_context
                .conversation
                .as_ref()
                .map(|conversation| (conversation.context_type, conversation.context_id.as_str()))
                .unwrap_or((context_type, context_id));
            let launch_security =
                super::conversation_launch_security::conversation_launch_security_class(
                    launch_context_type,
                    queued_agent_context.effective_mode,
                );
            let requested_model = queued_msg
                .model_override
                .as_deref()
                .or_else(|| continuation_runtime.effective_model());
            if let Some(model) = requested_model {
                if let Err(error) =
                    crate::application::agent_lane_resolution::validate_model_harness_compatibility(
                        harness, model,
                    )
                {
                    let error = error.to_string();
                    tracing::error!(
                        %conversation_id,
                        %harness,
                        model,
                        error = %error,
                        "Queued continuation runtime validation failed"
                    );
                    let failed_run = build_queued_agent_run(
                        conversation_id.clone(),
                        harness,
                        session_id,
                        run_chain_id,
                        parent_run_id,
                        queued_msg.metadata_override.as_deref(),
                        &continuation_runtime,
                        &queued_msg,
                        launch_security,
                    );
                    let failed_run_id = persist_failed_queued_run(
                        agent_run_repo,
                        app_handle.as_ref(),
                        failed_run,
                        &error,
                    )
                    .await;
                    emit_queued_preflight_error(
                        app_handle.as_ref(),
                        &conversation_id,
                        context_type,
                        context_id,
                        failed_run_id.clone(),
                        error,
                    );
                    last_run_id = failed_run_id.or(last_run_id);
                    return QueueProcessingOutcome {
                        total_processed,
                        last_run_id,
                    };
                }
            }
            // Emit run_started for the queued message (so frontend shows activity)
            let queued_run = build_queued_agent_run(
                conversation_id.clone(),
                harness,
                session_id,
                run_chain_id,
                parent_run_id,
                queued_msg.metadata_override.as_deref(),
                &continuation_runtime,
                &queued_msg,
                launch_security,
            );
            let queued_run_id = queued_run.id.as_str().to_string();
            if let Err(error) = agent_run_repo.create(queued_run).await {
                let error_string =
                    format!("Failed to persist queued continuation agent run: {error}");
                tracing::warn!(
                    error = %error,
                    queued_run_id,
                    conversation_id = %conversation_id,
                    "[QUEUE] Failed to persist queued continuation agent run"
                );
                emit_queued_preflight_error(
                    app_handle.as_ref(),
                    &conversation_id,
                    context_type,
                    context_id,
                    Some(queued_run_id.clone()),
                    error_string,
                );
                return QueueProcessingOutcome {
                    total_processed,
                    last_run_id: Some(queued_run_id),
                };
            }
            let queue_registry_key =
                RunningAgentKey::new(context_type.to_string(), queue_context_id);
            let queue_conversation_id = conversation_id.as_str().to_string();
            if let Err(error) = running_agent_registry
                .try_register(
                    queue_registry_key.clone(),
                    queue_conversation_id.clone(),
                    queued_run_id.clone(),
                )
                .await
            {
                let error_string = match error {
                    TryRegisterError::Occupied(existing) => format!(
                        "queued continuation launch slot is owned by agent run {}",
                        existing.agent_run_id
                    ),
                    TryRegisterError::Storage(error) => {
                        format!("failed to reserve queued continuation launch slot: {error}")
                    }
                };
                fail_queued_agent_run(
                    agent_run_repo,
                    running_agent_registry,
                    &queue_registry_key,
                    app_handle.as_ref(),
                    &queued_run_id,
                    &error_string,
                )
                .await;
                return QueueProcessingOutcome {
                    total_processed,
                    last_run_id: Some(queued_run_id),
                };
            }
            let launch_reservation_guard = super::launch_reservation::LaunchReservationGuard::new(
                Arc::clone(running_agent_registry),
                queue_registry_key.clone(),
                queued_run_id.clone(),
                std::time::Duration::from_secs(
                    crate::infrastructure::agents::claude::stream_timeouts()
                        .launch_reservation_lease_secs,
                ),
            );
            last_run_id = Some(queued_run_id.clone());
            tracing::info!(
                queued_run_id = %queued_run_id,
                run_chain_id = run_chain_id.unwrap_or("none"),
                parent_run_id = parent_run_id.unwrap_or("none"),
                agent_name = queued_agent_context.identity.agent_name.as_deref().unwrap_or("auto"),
                agent_profile = queued_agent_context.identity.agent_profile.unwrap_or("none"),
                "[QUEUE] Continuation run"
            );
            if let Some(ref handle) = app_handle {
                let _ = handle.emit(
                    "agent:run_started",
                    AgentRunStartedPayload::with_provider_session(
                        queued_run_id.clone(),
                        conversation_id.as_str().to_string(),
                        context_type.to_string(),
                        context_id.to_string(),
                        run_chain_id.map(|s| s.to_string()),
                        parent_run_id.map(|s| s.to_string()),
                        None,
                        None,
                        Some(harness),
                        Some(session_id.to_string()),
                    ),
                );
            }

            let resume_in_place =
                queued_message_resume_in_place(queued_msg.metadata_override.as_deref());
            let turn_attachments = if resume_in_place {
                Vec::new()
            } else {
                match super::load_turn_attachments_from_repo(
                    chat_attachment_repo,
                    &conversation_id,
                    &queued_msg.attachment_ids,
                )
                .await
                {
                    Ok(attachments) => attachments,
                    Err(error) => {
                        tracing::error!(
                            error = %error,
                            queued_message_id = %queued_msg.id,
                            "[QUEUE] Failed to load queued message attachments"
                        );
                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                "agent:error",
                                AgentErrorPayload {
                                    conversation_id: Some(conversation_id.as_str().to_string()),
                                    context_type: context_type.to_string(),
                                    context_id: context_id.to_string(),
                                    agent_run_id: Some(queued_run_id.clone()),
                                    error: error.clone(),
                                    stderr: None,
                                },
                            );
                        }
                        fail_queued_agent_run(
                            agent_run_repo,
                            running_agent_registry,
                            &queue_registry_key,
                            app_handle.as_ref(),
                            &queued_run_id,
                            &error,
                        )
                        .await;
                        continue;
                    }
                }
            };
            let app_data_dir = app_handle
                .as_ref()
                .and_then(|handle| handle.try_state::<AppState>())
                .map(|state| state.app_paths.app_data_dir().to_path_buf());
            let attachment_context = match chat_service_context::format_attachments_for_agent(
                &turn_attachments,
                context_type,
                queued_agent_context.effective_mode,
                app_data_dir.as_deref(),
            )
            .await
            {
                Ok(context) => context,
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        queued_message_id = %queued_msg.id,
                        "[QUEUE] Failed to format queued message attachments"
                    );
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit(
                            "agent:error",
                            AgentErrorPayload {
                                conversation_id: Some(conversation_id.as_str().to_string()),
                                context_type: context_type.to_string(),
                                context_id: context_id.to_string(),
                                agent_run_id: Some(queued_run_id.clone()),
                                error: error.clone(),
                                stderr: None,
                            },
                        );
                    }
                    fail_queued_agent_run(
                        agent_run_repo,
                        running_agent_registry,
                        &queue_registry_key,
                        app_handle.as_ref(),
                        &queued_run_id,
                        &error,
                    )
                    .await;
                    continue;
                }
            };

            // Persist user message at enqueue time so replayed timelines match live ordering.
            if !resume_in_place {
                let mut user_msg = chat_service_context::create_user_message(
                    context_type,
                    context_id,
                    &queued_msg.content,
                    conversation_id,
                    queued_persisted_metadata(&queued_msg),
                    queued_persisted_created_at(&queued_msg),
                );
                // Mark session recovery rehydration prompts so the frontend can hide them
                // (only if no metadata_override was provided — override takes precedence)
                if queued_msg.metadata_override.is_none()
                    && queued_msg.content.starts_with("<instructions>")
                {
                    user_msg.metadata = Some(r#"{"recovery_context":true}"#.to_string());
                }
                let user_msg_id = user_msg.id.as_str().to_string();
                let user_msg_created_at = user_msg.created_at.to_rfc3339();
                let user_msg_metadata = user_msg.metadata.clone();
                if chat_message_repo.create(user_msg.clone()).await.is_ok() {
                    persist_message_text_timeline_item(&chat_timeline_repo, &user_msg).await;
                }
                if let Some(handle) = app_handle.as_ref() {
                    let app_state = handle.state::<AppState>();
                    let assignment_project_id = project_id
                        .map(str::to_string)
                        .or_else(|| {
                            (context_type == ChatContextType::Project)
                                .then(|| context_id.to_string())
                        })
                        .map(ProjectId::from_string);
                    if let Some(project_id) = assignment_project_id {
                        let repo = Arc::clone(&app_state.agent_conversation_jira_issue_repo);
                        let atlassian_integration_service =
                            Arc::clone(&app_state.atlassian_integration_service);
                        let assignment_result = crate::application::agent_conversation_jira_issue::assign_primary_jira_issue_if_absent_and_refresh(
                            &repo,
                            Some(atlassian_integration_service.as_ref()),
                            &conversation_id,
                            &project_id,
                            &queued_msg.composer_integration_references,
                            Some(ChatMessageId::from_string(user_msg_id.clone())),
                            user_msg.created_at,
                        )
                        .await;
                        if let Err(error) = assignment_result {
                            tracing::warn!(
                                conversation_id = %conversation_id.as_str(),
                                error = %error,
                                "[QUEUE] failed to auto-assign primary Jira issue from composer references"
                            );
                        }
                        let repo = Arc::clone(&app_state.agent_conversation_linear_issue_repo);
                        let linear_integration_service =
                            Arc::clone(&app_state.linear_integration_service);
                        let assignment_result = crate::application::agent_conversation_linear_issue::assign_primary_linear_issue_if_absent_and_refresh(
                            &repo,
                            Some(linear_integration_service.as_ref()),
                            &conversation_id,
                            &project_id,
                            &queued_msg.composer_integration_references,
                            Some(ChatMessageId::from_string(user_msg_id.clone())),
                            user_msg.created_at,
                        )
                        .await;
                        if let Err(error) = assignment_result {
                            tracing::warn!(
                                conversation_id = %conversation_id.as_str(),
                                error = %error,
                                "[QUEUE] failed to auto-assign primary Linear issue from composer references"
                            );
                        }
                        let repo = Arc::clone(&app_state.agent_conversation_granola_note_repo);
                        let granola_integration_service =
                            Arc::clone(&app_state.granola_integration_service);
                        let assignment_result = crate::application::agent_conversation_granola_note::assign_primary_granola_note_if_absent_and_refresh(
                            &repo,
                            Some(granola_integration_service.as_ref()),
                            &conversation_id,
                            &project_id,
                            &queued_msg.composer_integration_references,
                            Some(ChatMessageId::from_string(user_msg_id.clone())),
                            user_msg.created_at,
                        )
                        .await;
                        if let Err(error) = assignment_result {
                            tracing::warn!(
                                conversation_id = %conversation_id.as_str(),
                                error = %error,
                                "[QUEUE] failed to auto-assign primary Granola note from composer references"
                            );
                        }
                    }
                }

                if context_type == ChatContextType::Ideation {
                    let _ = ideation_session_repo.touch_updated_at(context_id).await;
                }

                // Link selected attachments to the user message after capturing
                // their prompt context for this queued turn.
                if !turn_attachments.is_empty() {
                    let attachment_ids: Vec<_> = turn_attachments
                        .iter()
                        .map(|attachment| attachment.id)
                        .collect();
                    let _ = chat_attachment_repo
                        .update_message_ids(
                            &attachment_ids,
                            &crate::domain::entities::ChatMessageId::from_string(&user_msg_id),
                        )
                        .await;
                    tracing::debug!(
                        message_id = %user_msg_id,
                        attachment_count = turn_attachments.len(),
                        "[QUEUE] Linked attachments to user message"
                    );
                }

                // Emit user message created
                if let Some(ref handle) = app_handle {
                    let _ = handle.emit(
                        "agent:message_created",
                        AgentMessageCreatedPayload {
                            message_id: user_msg_id,
                            conversation_id: conversation_id.as_str().to_string(),
                            context_type: context_type.to_string(),
                            context_id: context_id.to_string(),
                            role: "user".to_string(),
                            content: queued_msg.content.clone(),
                            created_at: Some(user_msg_created_at),
                            metadata: user_msg_metadata,
                            render_ready: None,
                        },
                    );
                }
            }

            let ideation_model_settings_repo = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.ideation_model_settings_repo)
            });
            let agent_lane_settings_repo = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.agent_lane_settings_repo)
            });
            let ideation_effort_settings_repo = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.ideation_effort_settings_repo)
            });
            let delegated_session_repo = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.delegated_session_repo)
            });
            let atlassian_integration_service = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.atlassian_integration_service)
            });
            let linear_integration_service = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.linear_integration_service)
            });
            let granola_integration_service = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.granola_integration_service)
            });
            let agent_conversation_jira_issue_repo = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.agent_conversation_jira_issue_repo)
            });
            let agent_conversation_linear_issue_repo = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.agent_conversation_linear_issue_repo)
            });
            let agent_conversation_granola_note_repo = app_handle.as_ref().map(|handle| {
                let app_state = handle.state::<AppState>();
                Arc::clone(&app_state.agent_conversation_granola_note_repo)
            });
            let assigned_jira_issue =
                if let Some(repo) = agent_conversation_jira_issue_repo.as_ref() {
                    repo.get_by_conversation_id(&conversation_id)
                        .await
                        .map_err(|error| {
                            tracing::warn!(
                                conversation_id = %conversation_id.as_str(),
                                error = %error,
                                "[QUEUE] failed to load agent conversation Jira assignment"
                            );
                            error
                        })
                        .ok()
                        .flatten()
                } else {
                    None
                };
            let assigned_linear_issue =
                if let Some(repo) = agent_conversation_linear_issue_repo.as_ref() {
                    repo.get_by_conversation_id(&conversation_id)
                        .await
                        .map_err(|error| {
                            tracing::warn!(
                                conversation_id = %conversation_id.as_str(),
                                error = %error,
                                "[QUEUE] failed to load agent conversation Linear assignment"
                            );
                            error
                        })
                        .ok()
                        .flatten()
                } else {
                    None
                };
            let assigned_granola_note =
                if let Some(repo) = agent_conversation_granola_note_repo.as_ref() {
                    repo.get_by_conversation_id(&conversation_id)
                        .await
                        .map_err(|error| {
                            tracing::warn!(
                                conversation_id = %conversation_id.as_str(),
                                error = %error,
                                "[QUEUE] failed to load agent conversation Granola note assignment"
                            );
                            error
                        })
                        .ok()
                        .flatten()
                } else {
                    None
                };
            let merged_jira_references =
                crate::application::agent_conversation_jira_issue::merge_assigned_jira_reference(
                    assigned_jira_issue.as_ref(),
                    &queued_msg.composer_integration_references,
                );
            let merged_linear_references =
                crate::application::agent_conversation_linear_issue::merge_assigned_linear_reference(
                    assigned_linear_issue.as_ref(),
                    &merged_jira_references,
                );
            let merged_integration_references =
                crate::application::agent_conversation_granola_note::merge_assigned_granola_reference(
                    assigned_granola_note.as_ref(),
                    &merged_linear_references,
                );

            let runtime_content =
                super::chat_service_composer_references::expand_project_references_for_prompt(
                    &queued_msg.content,
                    &queued_msg.composer_project_references,
                    working_directory,
                );
            let integration_expansion = expand_integration_references_for_prompt(
                &runtime_content,
                &merged_integration_references,
                atlassian_integration_service,
                linear_integration_service,
                granola_integration_service,
            )
            .await;
            log_skipped_integration_references(&integration_expansion.skipped_references);
            let runtime_content = integration_expansion.rewritten_prompt;
            let runtime_content =
                super::chat_service_composer_references::append_artifact_references_for_prompt(
                    &runtime_content,
                    &queued_msg.composer_artifact_references,
                );
            let runtime_content =
                match super::chat_service_selection_snapshot::append_selection_snapshot_for_prompt(
                    &runtime_content,
                    queued_msg.composer_selection_snapshot.as_ref(),
                ) {
                    Ok(runtime_content) => runtime_content,
                    Err(error) => {
                        let error_string = error.to_string();
                        tracing::warn!(
                            error = %error_string,
                            %context_type,
                            context_id,
                            "queue selection snapshot validation failed"
                        );
                        fail_queued_agent_run(
                            agent_run_repo,
                            running_agent_registry,
                            &queue_registry_key,
                            app_handle.as_ref(),
                            &queued_run_id,
                            &error_string,
                        )
                        .await;
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                };
            let runtime_content =
                super::chat_service_composer_references::append_excerpt_references_for_prompt(
                    &runtime_content,
                    &queued_msg.composer_excerpt_references,
                );
            let runtime_content = super::plan_mode_runtime_message(
                runtime_content,
                queued_agent_context.workspace.as_ref(),
            );
            let runtime_content = super::persona_builder_runtime_message(
                runtime_content,
                queued_agent_context.conversation.as_ref(),
                queued_agent_context.builder_draft.as_ref(),
            );
            let spawn_context = if let (Some(handle), Some(conversation)) = (
                app_handle.as_ref(),
                queued_agent_context.conversation.as_ref(),
            ) {
                let state = handle.state::<AppState>();
                match chat_service_context::resolve_conversation_spawn_context(
                    conversation,
                    queued_agent_context.effective_mode,
                    project_id,
                    Arc::clone(&state.project_repo),
                    working_directory,
                    Some(state.app_paths.app_data_dir()),
                    Some(state.app_paths.app_data_dir()),
                    Some(Arc::clone(&state.conversation_folder_reference_repo)),
                )
                .await
                {
                    Ok(context) => context,
                    Err(error) => {
                        let error_string = error.to_string();
                        tracing::warn!(
                            conversation_id = conversation_id.as_str(),
                            error = %error_string,
                            "queue resume folder reference root validation blocked spawn"
                        );
                        fail_queued_agent_run(
                            agent_run_repo,
                            running_agent_registry,
                            &queue_registry_key,
                            app_handle.as_ref(),
                            &queued_run_id,
                            &error_string,
                        )
                        .await;
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                }
            } else {
                chat_service_context::ResolvedConversationSpawnContext::without_app_state(
                    launch_context_type,
                    queued_agent_context.effective_mode,
                    working_directory,
                )
            };
            let persona_for_attribution = resolved_persona.clone();
            let queued_effort_override = queued_msg
                .logical_effort_override
                .map(|effort| effort.to_string());

            let queue_agent_name = queued_agent_context
                .identity
                .agent_name
                .as_deref()
                .unwrap_or("ralphx-chat-project");
            let readiness = chat_service_context::await_required_external_mcp(
                app_handle.as_ref(),
                harness,
                plugin_dir,
                queue_agent_name,
                queued_agent_context.identity.agent_profile,
            )
            .await;
            if let Err(error_string) = readiness {
                fail_queued_agent_run(
                    agent_run_repo,
                    running_agent_registry,
                    &queue_registry_key,
                    app_handle.as_ref(),
                    &queued_run_id,
                    &error_string,
                )
                .await;
                return QueueProcessingOutcome {
                    total_processed,
                    last_run_id,
                };
            }

            // Build and spawn resume command
            let provider_spawnable =
                match chat_service_context::build_resume_command_for_harness_with_continuation(
                    harness,
                    cli_path,
                    plugin_dir,
                    launch_context_type,
                    launch_context_id,
                    conversation_coordination_mode.unwrap_or(CoordinationMode::Solo),
                    &conversation_id.as_str(),
                    queued_agent_context.effective_mode,
                    &runtime_content,
                    resolved_persona,
                    spawn_context.folder_refs_block.as_deref(),
                    queued_agent_context.identity.agent_name.as_deref(),
                    queued_agent_context.identity.agent_profile,
                    working_directory,
                    session_id,
                    project_id,
                    &spawn_context.folder_roots,
                    if launch_context_type == ChatContextType::Project {
                        Some(conversation_id.as_str())
                    } else {
                        None
                    },
                    Arc::clone(chat_attachment_repo),
                    Arc::clone(artifact_repo),
                    agent_lane_settings_repo,
                    ideation_effort_settings_repo,
                    ideation_model_settings_repo,
                    Arc::clone(ideation_session_repo),
                    Arc::clone(
                        delegated_session_repo
                            .as_ref()
                            .expect("delegated session repo available"),
                    ),
                    Arc::clone(task_repo),
                    &[],
                    0,
                    queued_effort_override.as_deref(),
                    queued_msg.model_override.as_deref(),
                    Some(&continuation_runtime),
                    queued_msg.service_tier_override.as_deref(),
                    false,
                    Some(attachment_context.as_str()),
                    chat_service_context::ProjectSkillSelectionRepositories::from_app_handle(
                        app_handle.as_ref(),
                    ),
                )
                .await
                {
                    Ok(spawnable) => spawnable,
                    Err(err) => {
                        let error_string = err.to_string();
                        tracing::warn!(
                            error = %error_string,
                            %context_type,
                            context_id,
                            harness = %harness,
                            "queue spawn blocked"
                        );
                        fail_queued_agent_run(
                            agent_run_repo,
                            running_agent_registry,
                            &queue_registry_key,
                            app_handle.as_ref(),
                            &queued_run_id,
                            &error_string,
                        )
                        .await;
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                };
            let persona_injected = provider_spawnable.spawnable.persona_injected();
            let persona_injection_skipped_reason = provider_spawnable
                .spawnable
                .persona_injection_skipped_reason();
            let provider_env = match queue_provider_decision(
                app_handle.as_ref(),
                &agent_provider_settings_repo,
                harness,
                context_type,
            )
            .await
            {
                Ok(QueueProviderDecision::ApplyEnv(provider_env)) => provider_env,
                Ok(QueueProviderDecision::AllowWithoutProviderSettings) => HashMap::new(),
                Err(block) => {
                    let error_string = queue_provider_block_message(&block, context_type);
                    tracing::warn!(
                        error = %error_string,
                        %context_type,
                        context_id,
                        harness = %harness,
                        "queue spawn blocked by provider settings"
                    );
                    fail_queued_agent_run(
                        agent_run_repo,
                        running_agent_registry,
                        &queue_registry_key,
                        app_handle.as_ref(),
                        &queued_run_id,
                        &error_string,
                    )
                    .await;
                    return QueueProcessingOutcome {
                        total_processed,
                        last_run_id,
                    };
                }
            };
            let mut provider_spawnable = provider_spawnable;
            let Some(handle) = app_handle.as_ref() else {
                let error_string = "MCP launch policy service is unavailable";
                fail_queued_agent_run(
                    agent_run_repo,
                    running_agent_registry,
                    &queue_registry_key,
                    app_handle.as_ref(),
                    &queued_run_id,
                    error_string,
                )
                .await;
                return QueueProcessingOutcome {
                    total_processed,
                    last_run_id,
                };
            };
            let app_state = handle.state::<AppState>();
            let policy = match app_state
                .mcp_policy_service()
                .resolve_launch_policy(harness, project_id, Some(working_directory))
                .await
            {
                Ok(policy) => policy,
                Err(error) => {
                    let error_string = format!("Failed to resolve MCP launch policy: {error}");
                    fail_queued_agent_run(
                        agent_run_repo,
                        running_agent_registry,
                        &queue_registry_key,
                        app_handle.as_ref(),
                        &queued_run_id,
                        &error_string,
                    )
                    .await;
                    return QueueProcessingOutcome {
                        total_processed,
                        last_run_id,
                    };
                }
            };
            provider_spawnable.apply_mcp_policy(harness, &policy);
            provider_spawnable.apply_provider_env(&provider_env);
            let spawnable = provider_spawnable.spawnable;

            tracing::info!(cmd = ?spawnable, "Spawning CLI agent (queue resume)");
            match spawnable.spawn().await {
                Ok(mut child) => {
                    super::record_persona_run_attribution(
                        agent_run_repo,
                        app_handle.as_ref(),
                        &conversation_id,
                        &queued_run_id,
                        harness,
                        persona_for_attribution.as_ref(),
                        persona_injected,
                        persona_injection_skipped_reason,
                    )
                    .await;
                    let Some(pid) = child.id() else {
                        launch_reservation_guard.stop();
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        fail_queued_agent_run(
                            agent_run_repo,
                            running_agent_registry,
                            &queue_registry_key,
                            app_handle.as_ref(),
                            &queued_run_id,
                            "spawned queued continuation has no process id",
                        )
                        .await;
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id: Some(queued_run_id),
                        };
                    };
                    launch_reservation_guard.stop();
                    match running_agent_registry
                        .attach_process(
                            &queue_registry_key,
                            &queued_run_id,
                            pid,
                            Some(working_directory.to_string_lossy().to_string()),
                            Some(cancellation_token.clone()),
                            None,
                        )
                        .await
                    {
                        Ok(AttachProcessResult::Attached) => {}
                        Ok(AttachProcessResult::ClaimLost) | Err(_) => {
                            let error_string = "queued continuation lost its launch reservation";
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                            fail_queued_agent_run(
                                agent_run_repo,
                                running_agent_registry,
                                &queue_registry_key,
                                app_handle.as_ref(),
                                &queued_run_id,
                                error_string,
                            )
                            .await;
                            return QueueProcessingOutcome {
                                total_processed,
                                last_run_id: Some(queued_run_id),
                            };
                        }
                    }
                    let split_verification_transcript =
                        super::chat_service_send_background::should_split_verification_transcript(
                            context_type,
                            context_id,
                            ideation_session_repo,
                        )
                        .await;
                    // Create empty assistant message before queue stream
                    let queue_assistant_msg = chat_service_context::create_assistant_message(
                        context_type,
                        context_id,
                        "",
                        conversation_id,
                        &[],
                        &[],
                    )
                    .with_attribution(
                        crate::domain::entities::ChatMessageAttribution {
                            attribution_source: Some("native_runtime".to_string()),
                            provider_harness: Some(harness),
                            provider_session_id: Some(session_id.to_string()),
                            upstream_provider: None,
                            provider_profile: None,
                            logical_model: None,
                            effective_model_id: None,
                            logical_effort: None,
                            effective_effort: None,
                        },
                    );
                    let queue_assistant_msg_id = queue_assistant_msg.id.as_str().to_string();
                    let _ = chat_message_repo.create(queue_assistant_msg).await;

                    let mut stop_queue_after_provider_error = false;
                    match process_stream_background(
                        child,
                        harness,
                        context_type,
                        context_id,
                        &conversation_id,
                        app_handle.clone(),
                        Some(Arc::clone(activity_event_repo)),
                        Some(Arc::clone(task_repo)),
                        Some(Arc::clone(chat_message_repo)),
                        chat_timeline_repo.clone(),
                        Some(queue_assistant_msg_id.clone()),
                        question_state.clone(),
                        cancellation_token.clone(),
                        streaming_state_cache.clone(),
                        None, // Queue processing doesn't have registry in scope
                        Some(Arc::clone(agent_run_repo)),
                        Some(queued_run_id.clone()),
                        None, // Queue processing doesn't track execution slots
                        None, // Queue processing doesn't persist session_id
                        split_verification_transcript,
                        true,
                        None,
                        None,
                        None,
                    )
                    .await
                    {
                        Ok(outcome) => {
                            let response = outcome.response_text;
                            let tools = outcome.tool_calls;
                            let blocks = outcome.content_blocks;
                            let provider_session_id = outcome.session_id;
                            let queue_stderr = outcome.stderr_text;
                            let turns_finalized = outcome.turns_finalized;
                            let turn_completion_applied = outcome.completion_applied;
                            let silent_interactive_exit = outcome.silent_interactive_exit;
                            if resume_in_place {
                                persist_hidden_resume_in_place_marker(
                                    chat_message_repo,
                                    context_type,
                                    context_id,
                                    conversation_id.clone(),
                                    queued_msg.metadata_override.as_deref(),
                                )
                                .await;
                            }
                            if let Some(ref provider_session_id) = provider_session_id {
                                let _ = chat_message_repo
                                    .update_provider_session_ref(
                                        &crate::domain::entities::ChatMessageId::from_string(
                                            queue_assistant_msg_id.clone(),
                                        ),
                                        &crate::domain::agents::ProviderSessionRef {
                                            harness,
                                            provider_session_id: provider_session_id.clone(),
                                        },
                                    )
                                    .await;
                            }
                            let meaningful_output =
                                has_meaningful_output(&response, tools.len(), &queue_stderr);
                            let assistant_message_persisted = if meaningful_output {
                                super::chat_service_send_background::finalize_structured_assistant_message(
                                    chat_message_repo,
                                    &chat_timeline_repo,
                                    app_handle.as_ref(),
                                    context_type,
                                    context_id,
                                    &conversation_id,
                                    &queue_assistant_msg_id,
                                    &get_assistant_role(&context_type).to_string(),
                                    &response,
                                    &tools,
                                    &blocks,
                                    split_verification_transcript,
                                )
                                .await
                            } else {
                                false
                            };
                            let recovery_enqueue =
                                super::chat_service_send_background::enqueue_silent_completion_recovery(
                                    message_queue.as_ref(),
                                    queued_message_repo.as_ref(),
                                    context_type,
                                    queue_context_id,
                                    &response,
                                    &tools,
                                    &blocks,
                                    turns_finalized,
                                    silent_interactive_exit,
                                    cancellation_token.is_cancelled(),
                                    true,
                                    queued_msg.metadata_override.as_deref(),
                                )
                                .await;
                            let recovery_exhausted = matches!(
                                recovery_enqueue,
                                super::chat_service_send_background::SilentCompletionRecoveryEnqueue::Exhausted { .. }
                            );
                            let mut verification_pending = false;
                            match recovery_enqueue {
                                super::chat_service_send_background::SilentCompletionRecoveryEnqueue::Queued {
                                    attempt,
                                    backoff_ms,
                                } => {
                                    tracing::warn!(
                                        %context_type,
                                        context_id,
                                        queue_context_id,
                                        queued_run_id = %queued_run_id,
                                        attempt,
                                        backoff_ms,
                                        "[QUEUE] Requeued hidden silent-completion recovery"
                                    );
                                }
                                super::chat_service_send_background::SilentCompletionRecoveryEnqueue::Exhausted { attempts } => {
                                    tracing::error!(
                                        %context_type,
                                        context_id,
                                        queue_context_id,
                                        queued_run_id = %queued_run_id,
                                        attempts,
                                        "[QUEUE] Silent-completion recovery attempts exhausted"
                                    );
                                    if let Some(ref handle) = app_handle {
                                        let _ = handle.emit(
                                            "agent:error",
                                            AgentErrorPayload {
                                                conversation_id: Some(conversation_id.as_str().to_string()),
                                                context_type: context_type.to_string(),
                                                context_id: context_id.to_string(),
                                                agent_run_id: Some(queued_run_id.clone()),
                                                error: "Agent stopped after tool activity without a final response after automated recovery attempts".to_string(),
                                                stderr: None,
                                            },
                                        );
                                    }
                                }
                                super::chat_service_send_background::SilentCompletionRecoveryEnqueue::NotNeeded => {}
                            }

                            // NOTE: Don't emit run_completed here for each queued message.
                            // We emit a single run_completed after ALL queue processing is done,
                            // to prevent UI flickering between messages.
                            if recovery_exhausted {
                                let _ = agent_run_repo
                                    .fail(
                                        &AgentRunId::from_string(queued_run_id.clone()),
                                        "Agent stopped after automated silent-completion recovery attempts",
                                    )
                                    .await;
                            } else if meaningful_output && !assistant_message_persisted {
                                let _ = agent_run_repo
                                    .fail(
                                        &AgentRunId::from_string(queued_run_id.clone()),
                                        "Failed to persist the final assistant message",
                                    )
                                    .await;
                            } else {
                                let completion_applied = if turn_completion_applied {
                                    true
                                } else {
                                    agent_run_repo
                                        .complete_if_running(&AgentRunId::from_string(
                                            queued_run_id.clone(),
                                        ))
                                        .await
                                        .unwrap_or_else(|error| {
                                            tracing::error!(
                                                error = %error,
                                                queued_run_id,
                                                "Queue: guarded run completion failed"
                                            );
                                            false
                                        })
                                };
                                if completion_applied
                                    && ((meaningful_output && assistant_message_persisted)
                                        || turns_finalized > 0)
                                {
                                    if let Some(handle) = app_handle.as_ref() {
                                        if let Some(state) =
                                            handle.try_state::<crate::application::AppState>()
                                        {
                                            let chat_service = state
                                                .build_chat_service_for_runtime(
                                                    execution_state.clone(),
                                                    Some(handle.clone()),
                                                );
                                            match crate::application::plan_verification_service::admit_automatic_plan_verification(
                                                state.inner(),
                                                &chat_service,
                                                &conversation_id,
                                                &AgentRunId::from_string(queued_run_id.clone()),
                                                true,
                                            )
                                            .await
                                            {
                                                Ok(disposition) => {
                                                    verification_pending =
                                                        disposition.verification_pending();
                                                }
                                                Err(error) => {
                                                    tracing::error!(
                                                        error = %error,
                                                        conversation_id = %conversation_id,
                                                        queued_run_id,
                                                        "Queue: automatic plan verification admission failed"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                if completion_applied {
                                    reconcile_queued_verification_child_completion(
                                        context_type,
                                        context_id,
                                        ideation_session_repo,
                                        chat_message_repo,
                                        message_queue,
                                        app_handle.as_ref(),
                                    )
                                    .await;
                                }
                            }
                            if let Some(handle) = app_handle.as_ref() {
                                if let Some(state) =
                                    handle.try_state::<crate::application::AppState>()
                                {
                                    if !verification_pending {
                                        if let Err(error) = crate::application::plan_approval_notification_service::release_deferred_plan_approval_for_conversation(
                                            state.inner(),
                                            &conversation_id,
                                        )
                                        .await
                                        {
                                            tracing::warn!(error = %error, conversation_id = %conversation_id, "Failed to release deferred plan approval after queued admission settled");
                                        }
                                    }
                                    if let Err(error) = crate::application::plan_approval_notification_service::release_deferred_plan_approval_for_run(
                                        state.inner(),
                                        &AgentRunId::from_string(queued_run_id.clone()),
                                    )
                                    .await
                                    {
                                        tracing::warn!(error = %error, queued_run_id, "Failed to release deferred plan approval for terminal queued verification run");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            if let crate::application::chat_service::StreamError::ProviderError {
                                category,
                                message,
                                retry_after,
                            } = &e
                            {
                                let mut resumed_msg = queued_msg.clone();
                                resumed_msg.metadata_override = with_resume_in_place_metadata(
                                    resumed_msg.metadata_override.clone(),
                                );
                                restore_queue_front(
                                    queued_message_repo.as_ref(),
                                    message_queue,
                                    &queue_key,
                                    resumed_msg,
                                )
                                .await;
                                super::chat_service_handlers::apply_system_wide_provider_pause(
                                    &app_handle,
                                    category,
                                    message,
                                    retry_after,
                                    context_type,
                                    context_id,
                                )
                                .await;
                                stop_queue_after_provider_error = true;
                            }
                            let error_string = redact(&e.to_string());
                            tracing::error!(
                                "Failed to process queued message stream: {}",
                                error_string
                            );
                            match &e {
                                crate::application::chat_service::StreamError::Cancelled {
                                    ..
                                } => {
                                    let _ = agent_run_repo
                                        .cancel(&AgentRunId::from_string(queued_run_id.clone()))
                                        .await;
                                }
                                _ => {
                                    let _ = agent_run_repo
                                        .fail(
                                            &AgentRunId::from_string(queued_run_id.clone()),
                                            &error_string,
                                        )
                                        .await;
                                }
                            }
                            settle_terminal_queued_plan_verification(
                                app_handle.as_ref(),
                                &queued_run_id,
                            )
                            .await;
                            // Emit error event
                            if let Some(ref handle) = app_handle {
                                let _ = handle.emit(
                                    "agent:error",
                                    AgentErrorPayload {
                                        conversation_id: Some(conversation_id.as_str().to_string()),
                                        context_type: context_type.to_string(),
                                        context_id: context_id.to_string(),
                                        agent_run_id: Some(queued_run_id.clone()),
                                        error: error_string.clone(),
                                        stderr: Some(error_string),
                                    },
                                );
                            }
                        }
                    }
                    running_agent_registry
                        .unregister(&queue_registry_key, &queued_run_id)
                        .await;
                    if stop_queue_after_provider_error {
                        tracing::info!(
                            %context_type,
                            context_id,
                            queue_context_id,
                            queued_run_id = %queued_run_id,
                            "[QUEUE] Provider error restored queued message; stopping queue processing"
                        );
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to spawn queued message command: {}", e);
                    let error_string = e.to_string();
                    fail_queued_agent_run(
                        agent_run_repo,
                        running_agent_registry,
                        &queue_registry_key,
                        app_handle.as_ref(),
                        &queued_run_id,
                        &error_string,
                    )
                    .await;
                    // Emit error event
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit(
                            "agent:error",
                            AgentErrorPayload {
                                conversation_id: Some(conversation_id.as_str().to_string()),
                                context_type: context_type.to_string(),
                                context_id: context_id.to_string(),
                                agent_run_id: Some(queued_run_id.clone()),
                                error: e.to_string(),
                                stderr: None,
                            },
                        );
                    }
                }
            }
        }
        // End of inner while loop, outer loop continues to check for more
    }

    QueueProcessingOutcome {
        total_processed,
        last_run_id,
    }
}

#[doc(hidden)]
pub async fn process_queued_messages_for_test<R: Runtime + 'static>(
    app_handle: AppHandle<R>,
    context_type: ChatContextType,
    harness: AgentHarnessKind,
    context_id: &str,
    conversation_id: ChatConversationId,
    session_id: &str,
    cli_path: &Path,
) -> (u32, Option<String>) {
    process_queued_messages_for_test_with_persona_feature(
        app_handle,
        context_type,
        harness,
        context_id,
        conversation_id,
        session_id,
        cli_path,
        true,
    )
    .await
}

#[doc(hidden)]
pub async fn process_queued_messages_for_test_with_persona_feature<R: Runtime + 'static>(
    app_handle: AppHandle<R>,
    context_type: ChatContextType,
    harness: AgentHarnessKind,
    context_id: &str,
    conversation_id: ChatConversationId,
    session_id: &str,
    cli_path: &Path,
    persona_feature_enabled: bool,
) -> (u32, Option<String>) {
    let (
        message_queue,
        running_agent_registry,
        agent_run_repo,
        chat_message_repo,
        chat_attachment_repo,
        artifact_repo,
        activity_event_repo,
        task_repo,
        ideation_session_repo,
        app_data_dir,
    ) = {
        let state = app_handle.state::<AppState>();
        (
            Arc::clone(&state.message_queue),
            Arc::clone(&state.running_agent_registry),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.chat_message_repo),
            Arc::clone(&state.chat_attachment_repo),
            Arc::clone(&state.artifact_repo),
            Arc::clone(&state.activity_event_repo),
            Arc::clone(&state.task_repo),
            Arc::clone(&state.ideation_session_repo),
            state.app_paths.app_data_dir().to_path_buf(),
        )
    };
    let streaming_state_cache = super::StreamingStateCache::new();
    let queue_context_id = conversation_id.as_str();
    let current_dir = std::env::current_dir().expect("resolve queue test working directory");
    let working_directory = if context_type == ChatContextType::Standalone {
        crate::application::standalone_workspace::resolve_workspace(
            &app_data_dir,
            &conversation_id.as_str(),
        )
        .expect("resolve standalone queue test workspace")
    } else {
        current_dir.clone()
    };

    let outcome = process_queued_messages(
        context_type,
        harness,
        context_id,
        &queue_context_id,
        conversation_id,
        session_id,
        persona_feature_enabled,
        &message_queue,
        None,
        None,
        &running_agent_registry,
        &agent_run_repo,
        &chat_message_repo,
        None,
        &chat_attachment_repo,
        &artifact_repo,
        &activity_event_repo,
        &task_repo,
        &ideation_session_repo,
        cli_path,
        &current_dir,
        &working_directory,
        None,
        None,
        Some(app_handle),
        None,
        None,
        CancellationToken::new(),
        None,
        None,
        streaming_state_cache,
    )
    .await;

    (outcome.total_processed, outcome.last_run_id)
}

#[cfg(test)]
#[path = "chat_service_queue_tests.rs"]
mod tests;
