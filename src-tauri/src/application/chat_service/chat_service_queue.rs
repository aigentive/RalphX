// Message Queue Processing
//
// Handles queued messages that were sent while an agent was running.
// These messages are automatically processed via --resume after the initial run completes.

use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::application::AppState;
use super::chat_service_context;
use super::chat_service_helpers::{effective_team_mode_for_harness, get_assistant_role};
use super::chat_service_streaming::process_stream_background;
use super::chat_service_types::{
    AgentErrorPayload, AgentMessageCreatedPayload, AgentQueueSentPayload, AgentRunStartedPayload,
};
use super::has_meaningful_output;
use crate::application::question_state::QuestionState;
use crate::commands::ExecutionState;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    ChatContextType, ChatConversationId, InternalStatus, MessageRole, TaskId,
};
use crate::domain::repositories::{
    ActivityEventRepository, ArtifactRepository, ChatMessageRepository,
    IdeationSessionRepository, TaskRepository,
};
use crate::domain::services::MessageQueue;
use crate::utils::secret_redactor::redact;
use tokio_util::sync::CancellationToken;

const HIDDEN_RESUME_IN_PLACE_MARKER_CONTENT: &str =
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

fn hidden_resume_in_place_marker_metadata(metadata_override: Option<&str>) -> Option<String> {
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
    message_queue: &Arc<MessageQueue>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
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
    team_mode: bool,
    cancellation_token: CancellationToken,
    run_chain_id: Option<&str>,
    parent_run_id: Option<&str>,
    streaming_state_cache: super::StreamingStateCache,
) -> u32 {
    let mut total_processed = 0u32;

    // Outer loop: keep processing until queue is stable-empty
    loop {
        if queue_processing_blocked_by_pause(context_type, execution_state.as_ref()) {
            tracing::info!(
                %context_type,
                context_id,
                queue_context_id,
                pending = message_queue.get_queued(context_type, queue_context_id).len(),
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

        let queue_count = message_queue.get_queued(context_type, queue_context_id).len();

        if queue_count == 0 {
            // Queue is empty, wait briefly then check once more for race condition
            if total_processed > 0 {
                // We processed messages, give a small window for late arrivals
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let final_count = message_queue.get_queued(context_type, queue_context_id).len();
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
            queue_count
        );

        // Inner loop: process all currently queued messages
        while let Some(queued_msg) = message_queue.pop(context_type, queue_context_id) {
            if queue_processing_blocked_by_pause(context_type, execution_state.as_ref()) {
                message_queue.queue_front_existing(
                    context_type,
                    queue_context_id,
                    queued_msg,
                );
                tracing::info!(
                    %context_type,
                    context_id,
                    queue_context_id,
                    "[QUEUE] Execution paused after dequeue, restored message to queue front"
                );
                break;
            }

            if cancellation_token.is_cancelled() {
                tracing::info!("[QUEUE] Cancellation requested mid-queue, stopping");
                break;
            }

            // Guard: for task execution, verify task is still in Executing/ReExecuting state
            if context_type == ChatContextType::TaskExecution {
                let task_id = TaskId::from_string(context_id.to_string());
                match task_repo.get_by_id(&task_id).await {
                    Ok(Some(task)) => {
                        if task.internal_status != InternalStatus::Executing
                            && task.internal_status != InternalStatus::ReExecuting
                        {
                            let remaining = message_queue.get_queued(context_type, queue_context_id).len();
                            tracing::info!(
                                "[QUEUE] Task {} has transitioned to {:?}, draining {} queued messages without spawning",
                                context_id,
                                task.internal_status,
                                remaining + 1,
                            );
                            while message_queue.pop(context_type, queue_context_id).is_some() {}
                            break;
                        }
                    }
                    Ok(None) => {
                        tracing::warn!("[QUEUE] Task {} not found, draining queued messages", context_id);
                        while message_queue.pop(context_type, queue_context_id).is_some() {}
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("[QUEUE] Failed to check task state for {}: {}, proceeding cautiously", context_id, e);
                    }
                }
            }

            total_processed += 1;
            tracing::info!(
                "[QUEUE] Processing queued message id={}, content_len={}",
                queued_msg.id,
                queued_msg.content.len()
            );

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

            // Emit run_started for the queued message (so frontend shows activity)
            let queued_run_id = uuid::Uuid::new_v4().to_string();
            tracing::info!(
                queued_run_id = %queued_run_id,
                run_chain_id = run_chain_id.unwrap_or("none"),
                parent_run_id = parent_run_id.unwrap_or("none"),
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

            // Persist user message — apply overrides if present (e.g. auto-verification metadata + trigger timestamp)
            let resume_in_place =
                queued_message_resume_in_place(queued_msg.metadata_override.as_deref());
            if !resume_in_place {
                let created_at_override = queued_msg
                    .created_at_override
                    .as_deref()
                    .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                    .map(|ts| ts.with_timezone(&chrono::Utc));
                let mut user_msg = chat_service_context::create_user_message(
                    context_type,
                    context_id,
                    &queued_msg.content,
                    conversation_id,
                    queued_msg.metadata_override.clone(),
                    created_at_override,
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
                let _ = chat_message_repo.create(user_msg).await;

                if context_type == ChatContextType::Ideation {
                    let _ = ideation_session_repo.touch_updated_at(context_id).await;
                }

                // Link pending attachments to the user message
                if let Ok(pending_attachments) = chat_attachment_repo
                    .find_by_conversation_id(&conversation_id)
                    .await
                {
                    let pending: Vec<_> = pending_attachments
                        .into_iter()
                        .filter(|a| a.message_id.is_none())
                        .collect();

                    if !pending.is_empty() {
                        let attachment_ids: Vec<_> =
                            pending.iter().map(|a| a.id.clone()).collect();
                        let _ = chat_attachment_repo
                            .update_message_ids(
                                &attachment_ids,
                                &crate::domain::entities::ChatMessageId::from_string(&user_msg_id),
                            )
                            .await;
                        tracing::debug!(
                            message_id = %user_msg_id,
                            attachment_count = pending.len(),
                            "[QUEUE] Linked attachments to user message"
                        );
                    }
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

            // Build and spawn resume command
            let provider_spawnable = match chat_service_context::build_resume_command_for_harness(
                harness,
                cli_path,
                plugin_dir,
                context_type,
                context_id,
                &queued_msg.content,
                working_directory,
                session_id,
                project_id,
                if context_type == ChatContextType::Project {
                    Some(conversation_id.as_str())
                } else {
                    None
                },
                team_mode,
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
                None,
                None,
                false,
            )
            .await {
                Ok(spawnable) => spawnable,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        %context_type,
                        context_id,
                        harness = %harness,
                        "queue spawn blocked"
                    );
                    return total_processed;
                }
            };
            let spawnable = provider_spawnable.spawnable;

            tracing::info!(cmd = ?spawnable, "Spawning CLI agent (queue resume)");
            match spawnable.spawn().await {
                Ok(child) => {
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
                    .with_attribution(crate::domain::entities::ChatMessageAttribution {
                        attribution_source: Some("native_runtime".to_string()),
                        provider_harness: Some(harness),
                        provider_session_id: Some(session_id.to_string()),
                        upstream_provider: None,
                        provider_profile: None,
                        logical_model: None,
                        effective_model_id: None,
                        logical_effort: None,
                        effective_effort: None,
                    });
                    let queue_assistant_msg_id = queue_assistant_msg.id.as_str().to_string();
                    let _ = chat_message_repo.create(queue_assistant_msg).await;

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
                        Some(queue_assistant_msg_id.clone()),
                        question_state.clone(),
                        cancellation_token.clone(),
                        None, // Queue processing doesn't need team events
                        effective_team_mode_for_harness(team_mode, harness),
                        streaming_state_cache.clone(),
                        None, // Queue processing doesn't have registry in scope
                        None, // Queue processing doesn't complete agent_run
                        None,
                        None, // Queue processing doesn't track execution slots
                        None, // Queue processing doesn't persist session_id
                        split_verification_transcript,
                        true,
                    )
                    .await
                    {
                        Ok(outcome) => {
                            let response = outcome.response_text;
                            let tools = outcome.tool_calls;
                            let blocks = outcome.content_blocks;
                            let provider_session_id = outcome.session_id;
                            let queue_stderr = outcome.stderr_text;
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
                            if has_meaningful_output(&response, tools.len(), &queue_stderr) {
                                super::chat_service_send_background::finalize_structured_assistant_message(
                                    chat_message_repo,
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
                                .await;
                            }

                            // NOTE: Don't emit run_completed here for each queued message.
                            // We emit a single run_completed after ALL queue processing is done,
                            // to prevent UI flickering between messages.
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
                                message_queue.queue_front_existing(
                                    context_type,
                                    queue_context_id,
                                    resumed_msg,
                                );
                                super::chat_service_handlers::apply_system_wide_provider_pause(
                                    &app_handle,
                                    category,
                                    message,
                                    retry_after,
                                    &context_type.to_string(),
                                    context_id,
                                )
                                .await;
                            }
                            let error_string = redact(&e.to_string());
                            tracing::error!(
                                "Failed to process queued message stream: {}",
                                error_string
                            );
                            // Emit error event
                            if let Some(ref handle) = app_handle {
                                let _ = handle.emit(
                                    "agent:error",
                                    AgentErrorPayload {
                                        conversation_id: Some(conversation_id.as_str().to_string()),
                                        context_type: context_type.to_string(),
                                        context_id: context_id.to_string(),
                                        error: error_string.clone(),
                                        stderr: Some(error_string),
                                    },
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to spawn queued message command: {}", e);
                    // Emit error event
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit(
                            "agent:error",
                            AgentErrorPayload {
                                conversation_id: Some(conversation_id.as_str().to_string()),
                                context_type: context_type.to_string(),
                                context_id: context_id.to_string(),
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

    total_processed
}
