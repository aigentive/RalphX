// Message Queue Processing
//
// Handles queued messages that were sent while an agent was running.
// These messages are automatically processed via --resume after the initial run completes.

use chrono::{DateTime, Utc};
use ralphx_events::{emit_serialized, EventSink};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use super::chat_service_context;
use super::chat_service_helpers::get_assistant_role;
use super::chat_service_streaming::{
    persist_message_text_timeline_item, process_stream_background,
};
use super::chat_service_types::{
    AgentErrorPayload, AgentMessageCreatedPayload, AgentMessageQueuedPayload,
    AgentMessageRenderReadyPayload, AgentQueueSentPayload, AgentRunStartedPayload,
};
use super::has_meaningful_output;
use super::{
    message_metadata_hidden_from_ui, persona_resolve_flags_for_conversation,
    team_intent_for_persisted_coordination_mode, ChatService, SendMessageOptions,
};
use crate::application::agent_runtime_context::{
    compose_agent_runtime_context, AgentRuntimeContextScope,
};
use crate::application::conversation_reference_inheritance::collect_conversation_inherited_integration_references;
use crate::application::integration_reference_expansion::{
    expand_integration_references_for_prompt, log_skipped_integration_references,
};
use crate::application::interactive_process_registry::PendingStdinTurn;
use crate::application::persona_resolver::resolve_persona_for_send;
use crate::application::plan_verification_service::PlanVerificationCompletionAdapter;
use crate::application::question_state::QuestionState;
use crate::application::runtime_factory::{build_chat_service_from_deps, ChatRuntimeFactoryDeps};
#[cfg(any(test, feature = "test-utils"))]
use crate::application::AppState;
use crate::application::execution_state::ExecutionState;
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

async fn provider_env_for_harness(
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    harness: AgentHarnessKind,
) -> Result<HashMap<String, String>, String> {
    crate::application::provider_env_file::load_provider_custom_env_file_for_harness(
        agent_provider_settings_repo.as_ref(),
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

pub(super) async fn queue_provider_decision(
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    harness: AgentHarnessKind,
    context_type: ChatContextType,
) -> Result<QueueProviderDecision, QueueProviderBlock> {
    let Some(provider_repo) = agent_provider_settings_repo.as_ref() else {
        return if super::uses_execution_slot(context_type) {
            Err(QueueProviderBlock::MissingProviderSettings)
        } else {
            Ok(QueueProviderDecision::AllowWithoutProviderSettings)
        };
    };

    crate::application::ensure_provider_spawn_enabled(provider_repo, harness, "queue_resume")
        .await
        .map_err(QueueProviderBlock::Disabled)?;

    let provider_env = provider_env_for_harness(&Some(Arc::clone(provider_repo)), harness)
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

fn emit_queue_sent(
    events: &dyn EventSink,
    message: &QueuedMessage,
    conversation_id: &ChatConversationId,
    key: &QueueKey,
) {
    let _ = emit_serialized(
        events,
        "agent:queue_sent",
        &AgentQueueSentPayload {
            message_id: message.id.clone(),
            conversation_id: conversation_id.as_str().to_string(),
            context_type: key.context_type.to_string(),
            context_id: key.context_id.clone(),
        },
    );
}

fn emit_backend_message_queued(
    events: &dyn EventSink,
    message: &QueuedMessage,
    conversation_id: Option<String>,
    key: &QueueKey,
) {
    if message_metadata_hidden_from_ui(message.metadata_override.as_deref()) {
        return;
    }
    let _ = emit_serialized(
        events,
        "agent:message_queued",
        &AgentMessageQueuedPayload {
            message_id: message.id.clone(),
            content: message.content.clone(),
            context_type: key.context_type.to_string(),
            context_id: key.context_id.clone(),
            conversation_id,
            created_at: message.created_at.clone(),
            attachment_ids: message
                .attachment_ids
                .iter()
                .map(ToString::to_string)
                .collect(),
        },
    );
}

fn emit_queue_error(
    events: &dyn EventSink,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    agent_run_id: Option<String>,
    error: String,
    stderr: Option<String>,
) {
    let _ = emit_serialized(
        events,
        "agent:error",
        &AgentErrorPayload {
            conversation_id: Some(conversation_id.as_str().to_string()),
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
            agent_run_id,
            error,
            stderr,
        },
    );
}

/// Transfer unanswered stdin turns into memory + durable queue truth, then publish it.
///
/// Durable failures retain the in-memory retry and surface an error instead of claiming
/// backend confirmation to the frontend.
///
/// Transcript evidence used to suppress recovery of an already-answered turn.
pub(crate) struct AnsweredTurnEvidence<'a> {
    pub chat_message_repo: &'a Arc<dyn ChatMessageRepository>,
    pub chat_timeline_repo: &'a Arc<dyn ChatTimelineRepository>,
    pub conversation_id: &'a ChatConversationId,
}

pub(crate) async fn requeue_pending_stdin_turns(
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    message_queue: &MessageQueue,
    events: &dyn EventSink,
    context_type: ChatContextType,
    queue_context_id: &str,
    conversation_id: Option<String>,
    pending_turns: Vec<PendingStdinTurn>,
    evidence: Option<AnsweredTurnEvidence<'_>>,
) {
    let answered_at = if let Some(evidence) = evidence {
        let assistant_role = get_assistant_role(&context_type);
        match tokio::try_join!(
            evidence
                .chat_message_repo
                .get_recent_by_conversation_paginated(evidence.conversation_id, 20, 0),
            evidence
                .chat_timeline_repo
                .latest_assistant_activity_at_for_conversation(
                    evidence.conversation_id,
                    assistant_role,
                )
        ) {
            Ok((messages, timeline_activity_at)) => messages
                .into_iter()
                .filter(|message| message.role == assistant_role)
                .map(|message| message.created_at)
                .max()
                .into_iter()
                .chain(timeline_activity_at)
                .max(),
            Err(error) => {
                tracing::warn!(
                    conversation_id = %evidence.conversation_id,
                    error = %error,
                    "[QUEUE] Could not read assistant activity evidence for recovered stdin turns"
                );
                None
            }
        }
    } else {
        None
    };

    let key = QueueKey::new(context_type, queue_context_id);
    let queued_messages = pending_turns
        .into_iter()
        .filter(|turn| {
            let Some(answered_at) = answered_at.as_ref() else {
                return true;
            };
            let Ok(queued_at) = DateTime::parse_from_rfc3339(&turn.queued_at) else {
                tracing::warn!(
                    queued_message_id = %turn.persisted_message_id,
                    queued_at = %turn.queued_at,
                    "[QUEUE] Could not parse recovered stdin timestamp; retaining turn"
                );
                return true;
            };
            let queued_at = queued_at.with_timezone(&Utc);
            if queued_at >= *answered_at {
                return true;
            }
            tracing::info!(
                queued_message_id = %turn.persisted_message_id,
                persisted_message_id = %turn.persisted_message_id,
                queued_at = %turn.queued_at,
                assistant_activity_at = %answered_at.to_rfc3339(),
                "[QUEUE] Suppressed recovered stdin turn with later assistant evidence"
            );
            false
        })
        .map(|turn| {
            let mut queued = QueuedMessage::new(turn.content);
            queued.created_at = turn.queued_at.clone();
            queued.created_at_override = Some(turn.queued_at);
            queued.metadata_override = turn.metadata_override;
            queued.persisted_message_id = Some(turn.persisted_message_id);
            queued
        })
        .collect::<Vec<_>>();
    let mut confirmed_ids = HashSet::new();
    for queued in queued_messages.iter().rev() {
        message_queue.queue_front_existing(
            context_type,
            queue_context_id.to_string(),
            queued.clone(),
        );

        let durable_result = match queued_message_repo {
            Some(repo) => repo.enqueue_front(&key, queued).await,
            None => Ok(()),
        };
        match durable_result {
            Ok(()) => {
                confirmed_ids.insert(queued.id.clone());
            }
            Err(error) => {
                tracing::warn!(
                    %context_type,
                    queue_context_id,
                    queued_message_id = %queued.id,
                    error = %error,
                    "[QUEUE] Failed to persist recovered stdin turn"
                );
                let _ = emit_serialized(
                    events,
                    "agent:error",
                    &AgentErrorPayload {
                        conversation_id: conversation_id.clone(),
                        context_type: context_type.to_string(),
                        context_id: queue_context_id.to_string(),
                        agent_run_id: None,
                        error: format!(
                            "Recovered your unanswered message in memory, but durable queue persistence failed: {error}"
                        ),
                        stderr: None,
                    },
                );
            }
        }
    }
    for queued in &queued_messages {
        if confirmed_ids.contains(&queued.id) {
            emit_backend_message_queued(events, queued, conversation_id.clone(), &key);
        }
    }
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
        persisted_message_id: queued_msg.persisted_message_id.clone(),
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

enum ReplayOutcome {
    Delivered {
        was_queued: bool,
        agent_run_id: Option<String>,
    },
    Failed {
        error: String,
    },
    NoHandle,
}

#[allow(clippy::too_many_arguments)]
async fn replay_queued_message_via_fresh_session(
    runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>,
    execution_state: Option<&Arc<ExecutionState>>,
    queued_msg: &QueuedMessage,
    conversation_id: ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    queue_context_id: &str,
    queue_team_intent: Option<TeamIntent>,
    force_new_provider_session: bool,
    message_queue: &Arc<MessageQueue>,
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    queue_key: &QueueKey,
    persona_feature_enabled: bool,
) -> ReplayOutcome {
    let Some(runtime_factory_deps) = runtime_factory_deps else {
        restore_queue_front(
            queued_message_repo,
            message_queue,
            queue_key,
            queued_msg.clone(),
        )
        .await;
        tracing::warn!(
            %context_type,
            context_id,
            queue_context_id,
            "[QUEUE] Queued message requires chat service replay but no app handle is available"
        );
        return ReplayOutcome::NoHandle;
    };

    let service =
        build_chat_service_from_deps(execution_state.map(Arc::clone), runtime_factory_deps)
            .with_persona_feature_enabled(persona_feature_enabled);
    let send_result = service
        .send_message(
            context_type,
            context_id,
            &queued_msg.content,
            provider_switch_send_options_for_queued_message(
                queued_msg,
                conversation_id,
                force_new_provider_session,
                queue_team_intent,
            ),
        )
        .await;

    match send_result {
        Ok(result) => {
            let agent_run_id = (!result.agent_run_id.is_empty()).then_some(result.agent_run_id);
            tracing::info!(
                %context_type,
                context_id,
                queue_context_id,
                queued_message_id = %queued_msg.id,
                agent_run_id = ?agent_run_id,
                was_queued = result.was_queued,
                force_new_provider_session,
                "[QUEUE] Replayed queued message through chat service"
            );
            ReplayOutcome::Delivered {
                was_queued: result.was_queued,
                agent_run_id,
            }
        }
        Err(error) => {
            let error = error.to_string();
            tracing::error!(
                %context_type,
                context_id,
                queue_context_id,
                queued_message_id = %queued_msg.id,
                error = %error,
                "[QUEUE] Failed to replay queued message through chat service"
            );
            ReplayOutcome::Failed { error }
        }
    }
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
    parent_run: Option<&AgentRun>,
    fallback_agent_name: Option<&str>,
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
    run.agent_name = parent_run
        .and_then(|parent| parent.agent_name.clone())
        .or_else(|| fallback_agent_name.map(str::to_string));
    run.launch_role = parent_run.and_then(|parent| parent.launch_role.clone());
    // Continuations inherit the parent's authoritative spawn identity so a
    // queued turn keeps exactly the tier its originating spawn resolved.
    run.routing_role = parent_run.and_then(|parent| parent.routing_role);
    run.project_id = parent_run.and_then(|parent| parent.project_id.clone());
    run.runtime_source = parent_run.and_then(|parent| parent.runtime_source.clone());
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
    parent_run: Option<&AgentRun>,
    fallback_agent_name: Option<&str>,
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
    run.agent_name = parent_run
        .and_then(|parent| parent.agent_name.clone())
        .or_else(|| fallback_agent_name.map(str::to_string));
    run.launch_role = parent_run.and_then(|parent| parent.launch_role.clone());
    // Continuations inherit the parent's authoritative spawn identity so a
    // queued turn keeps exactly the tier its originating spawn resolved.
    run.routing_role = parent_run.and_then(|parent| parent.routing_role);
    run.project_id = parent_run.and_then(|parent| parent.project_id.clone());
    run.runtime_source = parent_run.and_then(|parent| parent.runtime_source.clone());
    run.apply_action_metadata_json(metadata);
    run
}

async fn persist_failed_queued_run(
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    plan_verification_completion: Option<&Arc<PlanVerificationCompletionAdapter>>,
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
    settle_terminal_queued_plan_verification(plan_verification_completion, &run_id).await;
    Some(run_id)
}

fn emit_queued_preflight_error(
    events: &dyn EventSink,
    conversation_id: &ChatConversationId,
    context_type: ChatContextType,
    context_id: &str,
    agent_run_id: Option<String>,
    error: String,
) {
    let _ = emit_serialized(
        events,
        "agent:error",
        &AgentErrorPayload {
            conversation_id: Some(conversation_id.as_str().to_string()),
            context_type: context_type.to_string(),
            context_id: context_id.to_string(),
            agent_run_id,
            error,
            stderr: None,
        },
    );
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
    coordination_mode: CoordinationMode,
) -> QueuedAgentIdentity {
    let Some(mode) = mode else {
        return QueuedAgentIdentity::default();
    };

    QueuedAgentIdentity {
        agent_name: Some(super::agent_name_for_conversation_mode(mode).to_string()),
        agent_profile: super::resolve_agent_conversation_runtime_profile(mode, coordination_mode),
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
    queued_agent_identity_for_mode(
        mode,
        conversation
            .map(|conversation| conversation.coordination_mode)
            .unwrap_or(CoordinationMode::Solo),
    )
}

async fn resolve_queued_agent_context(
    runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
) -> Result<QueuedAgentContext, String> {
    let Some(runtime_factory_deps) = runtime_factory_deps else {
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

    let mut builder_context_error = None;
    let conversation = match runtime_factory_deps
        .conversation_repo
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
        match runtime_factory_deps
            .persona_repo
            .as_ref()
            .ok_or_else(|| "Persona repository is unavailable for queue replay".to_string())?
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
    let workspace = match runtime_factory_deps
        .agent_conversation_workspace_repo
        .as_ref()
        .ok_or_else(|| {
            "Conversation workspace repository is unavailable for queue replay".to_string()
        })?
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

async fn resolve_queue_resume_persona(
    runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>,
    feature_enabled: bool,
    context_type: ChatContextType,
    conversation_id: &ChatConversationId,
    directive: &PersonaDirective,
    agent_name_override_set: bool,
) -> Result<Option<crate::application::persona_prompt::ResolvedPersona>, String> {
    if !feature_enabled {
        return Ok(None);
    }

    let Some(runtime_factory_deps) = runtime_factory_deps else {
        return Ok(None);
    };
    let conversation = runtime_factory_deps
        .conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|error| format!("Persona conversation lookup failed: {error}"))?
        .ok_or_else(|| {
            format!(
                "Persona conversation {} was not found",
                conversation_id.as_str()
            )
        })?;
    let workspace_mode = runtime_factory_deps
        .agent_conversation_workspace_repo
        .as_ref()
        .ok_or_else(|| "Persona workspace repository is unavailable for queue replay".to_string())?
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
        runtime_factory_deps
            .persona_repo
            .as_ref()
            .map(Arc::clone)
            .ok_or_else(|| "Persona repository is unavailable for queue replay".to_string())?,
    )
    .await
    .map_err(|error| error.to_string())
}

pub(super) async fn settle_terminal_queued_plan_verification(
    plan_verification_completion: Option<&Arc<PlanVerificationCompletionAdapter>>,
    run_id: &str,
) {
    let Some(adapter) = plan_verification_completion else {
        return;
    };
    if let Err(error) = adapter
        .release_for_run(&AgentRunId::from_string(run_id.to_string()))
        .await
    {
        tracing::warn!(error = %error, queued_run_id = run_id, "Failed to release deferred plan approval for terminal queued verification run");
    }
}

async fn fail_queued_agent_run(
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    registry_key: &RunningAgentKey,
    plan_verification_completion: Option<&Arc<PlanVerificationCompletionAdapter>>,
    run_id: &str,
    error: &str,
) {
    let _ = agent_run_repo
        .fail(&AgentRunId::from_string(run_id.to_string()), error)
        .await;
    running_agent_registry
        .unregister(registry_key, run_id)
        .await;
    settle_terminal_queued_plan_verification(plan_verification_completion, run_id).await;
}

async fn reconcile_queued_verification_child_completion(
    context_type: ChatContextType,
    context_id: &str,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    message_queue: &Arc<MessageQueue>,
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    conversation_repo: &Arc<dyn crate::domain::repositories::ChatConversationRepository>,
    events: &dyn EventSink,
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

    let verification_child_registry = None;
    super::chat_service_handlers::handle_verification_child_completion(
        &child_id,
        &parent_id,
        ideation_session_repo,
        conversation_repo,
        chat_message_repo,
        message_queue,
        queued_message_repo,
        events,
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
pub(super) async fn process_queued_messages(
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
    events: Arc<dyn EventSink>,
    plan_verification_completion: Option<Arc<PlanVerificationCompletionAdapter>>,
    runtime_factory_deps: Option<ChatRuntimeFactoryDeps>,
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
                runtime_factory_deps.as_ref(),
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
                    emit_queue_error(
                        events.as_ref(),
                        &conversation_id,
                        context_type,
                        context_id,
                        None,
                        error,
                        None,
                    );
                    total_processed += 1;
                    continue;
                }
            };

            let queued_agent_context = match resolve_queued_agent_context(
                runtime_factory_deps.as_ref(),
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
                    emit_queue_error(
                        events.as_ref(),
                        &conversation_id,
                        context_type,
                        context_id,
                        None,
                        error,
                        None,
                    );
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
                    emit_queue_error(
                        events.as_ref(),
                        &conversation_id,
                        context_type,
                        context_id,
                        None,
                        error,
                        None,
                    );
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
                emit_queue_error(
                    events.as_ref(),
                    &conversation_id,
                    context_type,
                    context_id,
                    None,
                    error.clone(),
                    None,
                );
                total_processed += 1;
                continue;
            }
            let target_harness = queued_target_harness(&queued_msg, harness);

            if queued_message_requires_fresh_provider_session(&queued_msg, harness) {
                let force_new_provider_session =
                    !can_reuse_fresh_provider_run(&queued_msg, fresh_provider_harness);
                match replay_queued_message_via_fresh_session(
                    runtime_factory_deps.as_ref(),
                    execution_state.as_ref(),
                    &queued_msg,
                    conversation_id.clone(),
                    context_type,
                    context_id,
                    queue_context_id,
                    queue_team_intent.clone(),
                    force_new_provider_session,
                    message_queue,
                    queued_message_repo.as_ref(),
                    &queue_key,
                    persona_feature_enabled,
                )
                .await
                {
                    ReplayOutcome::Delivered {
                        was_queued,
                        agent_run_id,
                    } => {
                        emit_queue_sent(events.as_ref(), &queued_msg, &conversation_id, &queue_key);
                        total_processed += 1;
                        if let Some(agent_run_id) = agent_run_id {
                            last_run_id = Some(agent_run_id);
                        }
                        if !was_queued {
                            fresh_provider_harness = Some(target_harness);
                        }
                        if was_queued {
                            return QueueProcessingOutcome {
                                total_processed,
                                last_run_id,
                            };
                        }
                        continue;
                    }
                    ReplayOutcome::Failed { error } => {
                        emit_queue_sent(events.as_ref(), &queued_msg, &conversation_id, &queue_key);
                        emit_queued_preflight_error(
                            events.as_ref(),
                            &conversation_id,
                            context_type,
                            context_id,
                            None,
                            error,
                        );
                        total_processed += 1;
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                    ReplayOutcome::NoHandle => {
                        // Message was restored to the queue front, not processed.
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                }
            }

            total_processed += 1;

            let parent_run = match parent_run_id {
                Some(parent_id) => match agent_run_repo
                    .get_by_id(&AgentRunId::from_string(parent_id.to_string()))
                    .await
                {
                    Ok(run) => run,
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            parent_run_id = parent_id,
                            "failed to load queued continuation parent run attribution"
                        );
                        None
                    }
                },
                None => None,
            };

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
                            "No completed {harness} run owns provider session {session_id}; falling back to fresh-session replay"
                        );
                        tracing::warn!(
                            %conversation_id,
                            %harness,
                            provider_session_id = session_id,
                            "{error}"
                        );
                        match replay_queued_message_via_fresh_session(
                            runtime_factory_deps.as_ref(),
                            execution_state.as_ref(),
                            &queued_msg,
                            conversation_id.clone(),
                            context_type,
                            context_id,
                            queue_context_id,
                            queue_team_intent.clone(),
                            true,
                            message_queue,
                            queued_message_repo.as_ref(),
                            &queue_key,
                            persona_feature_enabled,
                        )
                        .await
                        {
                            ReplayOutcome::Delivered { agent_run_id, .. } => {
                                emit_queue_sent(
                                    events.as_ref(),
                                    &queued_msg,
                                    &conversation_id,
                                    &queue_key,
                                );
                                last_run_id = agent_run_id.or(last_run_id);
                                return QueueProcessingOutcome {
                                    total_processed,
                                    last_run_id,
                                };
                            }
                            ReplayOutcome::NoHandle => {
                                return QueueProcessingOutcome {
                                    total_processed,
                                    last_run_id,
                                };
                            }
                            ReplayOutcome::Failed { error } => {
                                emit_queue_sent(
                                    events.as_ref(),
                                    &queued_msg,
                                    &conversation_id,
                                    &queue_key,
                                );
                                let failed_run = build_queued_preflight_failure_run(
                                    conversation_id.clone(),
                                    harness,
                                    session_id,
                                    run_chain_id,
                                    parent_run_id,
                                    queued_msg.metadata_override.as_deref(),
                                    &queued_msg,
                                    parent_run.as_ref(),
                                    queued_agent_context.identity.agent_name.as_deref(),
                                );
                                let failed_run_id = persist_failed_queued_run(
                                    agent_run_repo,
                                    plan_verification_completion.as_ref(),
                                    failed_run,
                                    &error,
                                )
                                .await;
                                emit_queued_preflight_error(
                                    events.as_ref(),
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
                        restore_queue_front(
                            queued_message_repo.as_ref(),
                            message_queue,
                            &queue_key,
                            queued_msg,
                        )
                        .await;
                        emit_queued_preflight_error(
                            events.as_ref(),
                            &conversation_id,
                            context_type,
                            context_id,
                            None,
                            error,
                        );
                        // Restored to the queue front, so the message was not processed.
                        total_processed = total_processed.saturating_sub(1);
                        return QueueProcessingOutcome {
                            total_processed,
                            last_run_id,
                        };
                    }
                };
            emit_queue_sent(events.as_ref(), &queued_msg, &conversation_id, &queue_key);
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
                        parent_run.as_ref(),
                        queued_agent_context.identity.agent_name.as_deref(),
                    );
                    let failed_run_id = persist_failed_queued_run(
                        agent_run_repo,
                        plan_verification_completion.as_ref(),
                        failed_run,
                        &error,
                    )
                    .await;
                    emit_queued_preflight_error(
                        events.as_ref(),
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
                parent_run.as_ref(),
                queued_agent_context.identity.agent_name.as_deref(),
            );
            let queued_run_id = queued_run.id.as_str().to_string();
            let queued_run_agent_name = queued_run.agent_name.clone();
            let queued_run_launch_role = queued_run.launch_role.clone();
            let queued_run_started_at = queued_run.started_at.to_rfc3339();
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
                    events.as_ref(),
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
                    plan_verification_completion.as_ref(),
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
            let mut started_payload = AgentRunStartedPayload::with_provider_session(
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
            );
            started_payload.agent_name = queued_run_agent_name.clone();
            started_payload.launch_role = queued_run_launch_role.clone();
            started_payload.started_at = Some(queued_run_started_at.clone());
            let _ = emit_serialized(events.as_ref(), "agent:run_started", &started_payload);

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
                        emit_queue_error(
                            events.as_ref(),
                            &conversation_id,
                            context_type,
                            context_id,
                            Some(queued_run_id.clone()),
                            error.clone(),
                            None,
                        );
                        fail_queued_agent_run(
                            agent_run_repo,
                            running_agent_registry,
                            &queue_registry_key,
                            plan_verification_completion.as_ref(),
                            &queued_run_id,
                            &error,
                        )
                        .await;
                        continue;
                    }
                }
            };
            let app_data_dir = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.folder_reference_app_data_dir.clone());
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
                    emit_queue_error(
                        events.as_ref(),
                        &conversation_id,
                        context_type,
                        context_id,
                        Some(queued_run_id.clone()),
                        error.clone(),
                        None,
                    );
                    fail_queued_agent_run(
                        agent_run_repo,
                        running_agent_registry,
                        &queue_registry_key,
                        plan_verification_completion.as_ref(),
                        &queued_run_id,
                        &error,
                    )
                    .await;
                    continue;
                }
            };

            // Persist user message at enqueue time so replayed timelines match live ordering.
            if !resume_in_place && queued_msg.persisted_message_id.is_none() {
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
                let persisted_timeline_item =
                    if chat_message_repo.create(user_msg.clone()).await.is_ok() {
                        persist_message_text_timeline_item(&chat_timeline_repo, &user_msg).await
                    } else {
                        None
                    };
                let assignment_project_id = project_id
                    .map(str::to_string)
                    .or_else(|| {
                        (context_type == ChatContextType::Project).then(|| context_id.to_string())
                    })
                    .map(ProjectId::from_string);
                if let (Some(project_id), Some(deps)) =
                    (assignment_project_id, runtime_factory_deps.as_ref())
                {
                    if let Some(repo) = deps.agent_conversation_jira_issue_repo.as_ref() {
                        if let Err(error) = crate::application::agent_conversation_jira_issue::assign_primary_jira_issue_if_absent_and_refresh(
                            repo,
                            deps.atlassian_integration_service.as_deref(),
                            &conversation_id,
                            &project_id,
                            &queued_msg.composer_integration_references,
                            Some(ChatMessageId::from_string(user_msg_id.clone())),
                            user_msg.created_at,
                        )
                        .await
                        {
                            tracing::warn!(conversation_id = %conversation_id.as_str(), error = %error, "[QUEUE] failed to auto-assign primary Jira issue from composer references");
                        }
                    }
                    if let Some(repo) = deps.agent_conversation_linear_issue_repo.as_ref() {
                        if let Err(error) = crate::application::agent_conversation_linear_issue::assign_primary_linear_issue_if_absent_and_refresh(
                            repo,
                            deps.linear_integration_service.as_deref(),
                            &conversation_id,
                            &project_id,
                            &queued_msg.composer_integration_references,
                            Some(ChatMessageId::from_string(user_msg_id.clone())),
                            user_msg.created_at,
                        )
                        .await
                        {
                            tracing::warn!(conversation_id = %conversation_id.as_str(), error = %error, "[QUEUE] failed to auto-assign primary Linear issue from composer references");
                        }
                    }
                    if let Some(repo) = deps.agent_conversation_granola_note_repo.as_ref() {
                        if let Err(error) = crate::application::agent_conversation_granola_note::assign_primary_granola_note_if_absent_and_refresh(
                            repo,
                            deps.granola_integration_service.as_deref(),
                            &conversation_id,
                            &project_id,
                            &queued_msg.composer_integration_references,
                            Some(ChatMessageId::from_string(user_msg_id.clone())),
                            user_msg.created_at,
                        )
                        .await
                        {
                            tracing::warn!(conversation_id = %conversation_id.as_str(), error = %error, "[QUEUE] failed to auto-assign primary Granola note from composer references");
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

                let _ = emit_serialized(
                    events.as_ref(),
                    "agent:message_created",
                    &AgentMessageCreatedPayload {
                        message_id: user_msg_id,
                        conversation_id: conversation_id.as_str().to_string(),
                        context_type: context_type.to_string(),
                        context_id: context_id.to_string(),
                        role: "user".to_string(),
                        content: queued_msg.content.clone(),
                        created_at: Some(user_msg_created_at),
                        metadata: user_msg_metadata,
                        // Hidden messages persist no timeline item, so they keep
                        // emitting the event with no render-ready position.
                        render_ready: persisted_timeline_item.and_then(|item| {
                            AgentMessageRenderReadyPayload::from_message_and_timeline_items(
                                &user_msg,
                                vec![item],
                            )
                        }),
                    },
                );
            }

            let ideation_model_settings_repo = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.ideation_model_settings_repo.as_ref().map(Arc::clone));
            let agent_lane_settings_repo = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.agent_lane_settings_repo.as_ref().map(Arc::clone));
            let ideation_effort_settings_repo = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.ideation_effort_settings_repo.as_ref().map(Arc::clone));
            let delegated_session_repo = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.delegated_session_repo.as_ref().map(Arc::clone));
            let atlassian_integration_service = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.atlassian_integration_service.as_ref().map(Arc::clone));
            let linear_integration_service = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.linear_integration_service.as_ref().map(Arc::clone));
            let granola_integration_service = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.granola_integration_service.as_ref().map(Arc::clone));
            let clickup_integration_service = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.clickup_integration_service.as_ref().map(Arc::clone));
            let agent_conversation_jira_issue_repo =
                runtime_factory_deps.as_ref().and_then(|deps| {
                    deps.agent_conversation_jira_issue_repo
                        .as_ref()
                        .map(Arc::clone)
                });
            let agent_conversation_linear_issue_repo =
                runtime_factory_deps.as_ref().and_then(|deps| {
                    deps.agent_conversation_linear_issue_repo
                        .as_ref()
                        .map(Arc::clone)
                });
            let agent_conversation_granola_note_repo =
                runtime_factory_deps.as_ref().and_then(|deps| {
                    deps.agent_conversation_granola_note_repo
                        .as_ref()
                        .map(Arc::clone)
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
            let inherited_integration_references =
                match collect_conversation_inherited_integration_references(
                    chat_message_repo.as_ref(),
                    &conversation_id,
                )
                .await
                {
                    Ok(references) => references,
                    Err(error) => {
                        let error = error.to_string();
                        tracing::error!(
                            conversation_id = %conversation_id.as_str(),
                            queued_message_id = %queued_msg.id,
                            error = %error,
                            "[QUEUE] Failed to load inherited integration references"
                        );
                        fail_queued_agent_run(
                            agent_run_repo,
                            running_agent_registry,
                            &queue_registry_key,
                            plan_verification_completion.as_ref(),
                            &queued_run_id,
                            &error,
                        )
                        .await;
                        continue;
                    }
                };
            let merged_integration_references = super::merge_conversation_integration_references(
                &inherited_integration_references.references,
                &queued_msg.composer_integration_references,
                assigned_jira_issue.as_ref(),
                assigned_linear_issue.as_ref(),
                assigned_granola_note.as_ref(),
            );
            log_skipped_integration_references(
                &inherited_integration_references.skipped_references,
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
                clickup_integration_service,
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
                            plan_verification_completion.as_ref(),
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
            let spawn_context = if let (Some(deps), Some(conversation)) = (
                runtime_factory_deps.as_ref(),
                queued_agent_context.conversation.as_ref(),
            ) {
                match chat_service_context::resolve_conversation_spawn_context(
                    conversation,
                    queued_agent_context.effective_mode,
                    project_id,
                    Arc::clone(&deps.project_repo),
                    working_directory,
                    deps.folder_reference_app_data_dir.as_deref(),
                    deps.folder_reference_app_data_dir.as_deref(),
                    deps.conversation_folder_reference_repo
                        .as_ref()
                        .map(Arc::clone),
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
                            plan_verification_completion.as_ref(),
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
            let agent_runtime_context = if let Some(deps) = runtime_factory_deps.as_ref() {
                match deps.agent_runtime_context_deps() {
                    Some(context_deps) => match queued_agent_context.conversation.as_ref() {
                        Some(conversation) => {
                            compose_agent_runtime_context(
                                &AgentRuntimeContextScope {
                                    conversation_id: &conversation.id,
                                    context_type: launch_context_type,
                                    context_id: launch_context_id,
                                    project_id,
                                    workspace: queued_agent_context.workspace.as_ref(),
                                    working_directory,
                                    entity_status: None,
                                },
                                &context_deps,
                            )
                            .await
                        }
                        None => None,
                    },
                    None => None,
                }
            } else {
                None
            };
            let queued_effort_override = queued_msg
                .logical_effort_override
                .map(|effort| effort.to_string());

            let queue_agent_name = queued_agent_context
                .identity
                .agent_name
                .as_deref()
                .unwrap_or("ralphx-chat-project");
            let readiness = chat_service_context::await_required_external_mcp(
                None,
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
                    plan_verification_completion.as_ref(),
                    &queued_run_id,
                    &error_string,
                )
                .await;
                return QueueProcessingOutcome {
                    total_processed,
                    last_run_id,
                };
            }

            // Role-tiered Atlassian MCP grants for the queued continuation, resolved
            // from the just-persisted run's authoritative routing_role/project_id
            // (never re-derived). Absent services or role yields no tools.
            let queued_extra_allowed_mcp_tools = match runtime_factory_deps.as_ref() {
                Some(deps) => {
                    crate::application::atlassian_mcp_tools_for_resumed_run(
                        agent_run_repo,
                        &deps.project_repo,
                        deps.atlassian_integration_service.as_ref(),
                        deps.manual_role_default_service.as_ref(),
                        Some(queued_run_id.as_str()),
                    )
                    .await
                }
                None => Vec::new(),
            };

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
                    Some(queued_run_id.as_str()),
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
                    queued_extra_allowed_mcp_tools,
                    agent_runtime_context.as_deref(),
                    Some(attachment_context.as_str()),
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
                            plan_verification_completion.as_ref(),
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
            let provider_env =
                match queue_provider_decision(&agent_provider_settings_repo, harness, context_type)
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
                            plan_verification_completion.as_ref(),
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
            let Some(policy_service) = runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.mcp_policy_service.as_ref())
            else {
                let error_string = "MCP launch policy service is unavailable";
                fail_queued_agent_run(
                    agent_run_repo,
                    running_agent_registry,
                    &queue_registry_key,
                    plan_verification_completion.as_ref(),
                    &queued_run_id,
                    error_string,
                )
                .await;
                return QueueProcessingOutcome {
                    total_processed,
                    last_run_id,
                };
            };
            let policy = match policy_service
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
                        plan_verification_completion.as_ref(),
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
                        events.as_ref(),
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
                            plan_verification_completion.as_ref(),
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
                                plan_verification_completion.as_ref(),
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
                        Arc::clone(&events),
                        plan_verification_completion.clone(),
                        runtime_factory_deps.clone(),
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
                                    events.as_ref(),
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
                                    emit_queue_error(
                                        events.as_ref(),
                                        &conversation_id,
                                        context_type,
                                        context_id,
                                        Some(queued_run_id.clone()),
                                        "Agent stopped after tool activity without a final response after automated recovery attempts".to_string(),
                                        None,
                                    );
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
                                    super::chat_service_run_finalization::finalize_run_completed(
                                        agent_run_repo,
                                        &AgentRunId::from_string(queued_run_id.clone()),
                                    )
                                    .await
                                };
                                if completion_applied
                                    && ((meaningful_output && assistant_message_persisted)
                                        || turns_finalized > 0)
                                {
                                    if let (Some(adapter), Some(deps)) = (
                                        plan_verification_completion.as_ref(),
                                        runtime_factory_deps.as_ref(),
                                    ) {
                                        let chat_service = build_chat_service_from_deps(
                                            execution_state.clone(),
                                            deps,
                                        );
                                        match adapter
                                            .admit_automatic(
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
                                                tracing::error!(error = %error, conversation_id = %conversation_id, queued_run_id, "Queue: automatic plan verification admission failed");
                                            }
                                        }
                                    }
                                }
                                if completion_applied {
                                    if let Some(deps) = runtime_factory_deps.as_ref() {
                                        reconcile_queued_verification_child_completion(
                                            context_type,
                                            context_id,
                                            ideation_session_repo,
                                            chat_message_repo,
                                            message_queue,
                                            queued_message_repo.as_ref(),
                                            &deps.conversation_repo,
                                            events.as_ref(),
                                        )
                                        .await;
                                    }
                                }
                            }
                            if let Some(adapter) = plan_verification_completion.as_ref() {
                                if !verification_pending {
                                    if let Err(error) =
                                        adapter.release_for_conversation(&conversation_id).await
                                    {
                                        tracing::warn!(error = %error, conversation_id = %conversation_id, "Failed to release deferred plan approval after queued admission settled");
                                    }
                                }
                                if let Err(error) = adapter
                                    .release_for_run(&AgentRunId::from_string(
                                        queued_run_id.clone(),
                                    ))
                                    .await
                                {
                                    tracing::warn!(error = %error, queued_run_id, "Failed to release deferred plan approval for terminal queued verification run");
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
                                    resumed_msg.clone(),
                                )
                                .await;
                                emit_backend_message_queued(
                                    events.as_ref(),
                                    &resumed_msg,
                                    Some(conversation_id.as_str()),
                                    &queue_key,
                                );
                                super::chat_service_handlers::apply_system_wide_provider_pause(
                                    runtime_factory_deps.as_ref(),
                                    execution_state.as_ref(),
                                    Arc::clone(&events),
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
                                plan_verification_completion.as_ref(),
                                &queued_run_id,
                            )
                            .await;
                            emit_queue_error(
                                events.as_ref(),
                                &conversation_id,
                                context_type,
                                context_id,
                                Some(queued_run_id.clone()),
                                error_string.clone(),
                                Some(error_string),
                            );
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
                        plan_verification_completion.as_ref(),
                        &queued_run_id,
                        &error_string,
                    )
                    .await;
                    emit_queue_error(
                        events.as_ref(),
                        &conversation_id,
                        context_type,
                        context_id,
                        Some(queued_run_id.clone()),
                        e.to_string(),
                        None,
                    );
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

#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub async fn process_queued_messages_for_test(
    state: &AppState,
    execution_state: Option<Arc<ExecutionState>>,
    events: Arc<dyn EventSink>,
    context_type: ChatContextType,
    harness: AgentHarnessKind,
    context_id: &str,
    conversation_id: ChatConversationId,
    session_id: &str,
    cli_path: &Path,
) -> (u32, Option<String>) {
    process_queued_messages_for_test_with_persona_feature(
        state,
        execution_state,
        events,
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

#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub async fn process_queued_messages_for_test_with_persona_feature(
    state: &AppState,
    execution_state: Option<Arc<ExecutionState>>,
    events: Arc<dyn EventSink>,
    context_type: ChatContextType,
    harness: AgentHarnessKind,
    context_id: &str,
    conversation_id: ChatConversationId,
    session_id: &str,
    cli_path: &Path,
    persona_feature_enabled: bool,
) -> (u32, Option<String>) {
    let streaming_state_cache = super::StreamingStateCache::new();
    let queue_context_id = conversation_id.as_str();
    let current_dir = std::env::current_dir().expect("resolve queue test working directory");
    let working_directory = if context_type == ChatContextType::Standalone {
        crate::application::standalone_workspace::resolve_workspace(
            state.app_paths.app_data_dir(),
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
        &state.message_queue,
        None,
        Some(Arc::clone(&state.agent_provider_settings_repo)),
        &state.running_agent_registry,
        &state.agent_run_repo,
        &state.chat_message_repo,
        Some(Arc::clone(&state.chat_timeline_repo)),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.activity_event_repo,
        &state.task_repo,
        &state.ideation_session_repo,
        cli_path,
        &current_dir,
        &working_directory,
        None,
        execution_state,
        events,
        Some(Arc::new(PlanVerificationCompletionAdapter::from_app_state(
            state,
        ))),
        Some(ChatRuntimeFactoryDeps::from_app_state(state)),
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
