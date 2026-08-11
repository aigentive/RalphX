// HTTP handlers for conversation endpoints
//
// Endpoints for querying conversation state, used by frontend to hydrate
// streaming UI when navigating to an active agent execution.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use super::*;
use crate::domain::entities::{
    AgentRunId, ChatContextType, ChatConversationId, DelegatedSessionId,
};
use crate::domain::services::RunningAgentKey;

#[derive(Clone)]
struct DelegatedTaskSnapshot {
    status: String,
    delegated_conversation_id: Option<String>,
    delegated_agent_run_id: Option<String>,
    provider_harness: Option<String>,
    provider_session_id: Option<String>,
    upstream_provider: Option<String>,
    provider_profile: Option<String>,
    logical_model: Option<String>,
    effective_model_id: Option<String>,
    logical_effort: Option<String>,
    effective_effort: Option<String>,
    approval_policy: Option<String>,
    sandbox_mode: Option<String>,
    total_tokens: Option<u64>,
    duration_ms: Option<u64>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    estimated_usd: Option<f64>,
    started_at: Option<String>,
    completed_at: Option<String>,
    timestamp_provenance: Option<String>,
}

fn delegated_duration_ms(
    started_at: &DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
) -> Option<u64> {
    completed_at
        .and_then(|completed| completed.signed_duration_since(*started_at).to_std().ok())
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

async fn load_delegated_task_snapshot(
    state: &HttpServerState,
    task: &ActiveStreamingTask,
) -> Result<Option<DelegatedTaskSnapshot>, StatusCode> {
    let Some(delegated_session_id) = task.delegated_session_id.as_deref() else {
        return Ok(None);
    };
    let session = state
        .app_state
        .delegated_session_repo
        .get_by_id(&DelegatedSessionId::from_string(delegated_session_id))
        .await
        .map_err(|error| {
            tracing::error!(delegated_session_id, %error, "Failed to enrich delegated session state");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let Some(session) = session else {
        return Ok(None);
    };

    let delegated_conversation = if let Some(conversation_id) =
        task.delegated_conversation_id.as_deref()
    {
        state
                .app_state
                .chat_conversation_repo
                .get_by_id(&ChatConversationId::from_string(conversation_id))
                .await
                .map_err(|error| {
                    tracing::error!(delegated_session_id, conversation_id, %error, "Failed to enrich delegated conversation state");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
    } else {
        state
                .app_state
                .chat_conversation_repo
                .get_active_for_context(ChatContextType::Delegation, delegated_session_id)
                .await
                .map_err(|error| {
                    tracing::error!(delegated_session_id, %error, "Failed to resolve delegated conversation state");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
    };
    let Some(delegated_conversation) = delegated_conversation else {
        return Ok(None);
    };
    if delegated_conversation.context_type != ChatContextType::Delegation
        || delegated_conversation.context_id != delegated_session_id
    {
        return Ok(None);
    }

    let Some(agent_run_id) = task.delegated_agent_run_id.as_deref() else {
        return Ok(None);
    };
    let run = state
            .app_state
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(agent_run_id))
            .await
            .map_err(|error| {
                tracing::error!(delegated_session_id, agent_run_id, %error, "Failed to enrich delegated run state");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    let Some(latest_run) = run else {
        return Ok(None);
    };
    if latest_run.conversation_id != delegated_conversation.id {
        return Ok(None);
    }

    let status = latest_run.status.to_string();

    Ok(Some(DelegatedTaskSnapshot {
        status,
        delegated_conversation_id: Some(delegated_conversation.id.as_str()),
        delegated_agent_run_id: Some(latest_run.id.as_str()),
        provider_harness: latest_run
            .harness
            .map(|value| value.to_string())
            .or_else(|| Some(session.harness.to_string())),
        provider_session_id: latest_run
            .provider_session_id
            .clone()
            .or_else(|| session.provider_session_id.clone()),
        upstream_provider: latest_run.upstream_provider.clone(),
        provider_profile: latest_run.provider_profile.clone(),
        logical_model: latest_run.logical_model.clone(),
        effective_model_id: latest_run.effective_model_id.clone(),
        logical_effort: latest_run.logical_effort.map(|value| value.to_string()),
        effective_effort: latest_run.effective_effort.clone(),
        approval_policy: latest_run.approval_policy.clone(),
        sandbox_mode: latest_run.sandbox_mode.clone(),
        total_tokens: latest_run.processed_tokens(),
        duration_ms: delegated_duration_ms(&latest_run.started_at, latest_run.completed_at),
        input_tokens: latest_run.input_tokens,
        output_tokens: latest_run.output_tokens,
        cache_creation_tokens: latest_run.cache_creation_tokens,
        cache_read_tokens: latest_run.cache_read_tokens,
        estimated_usd: latest_run.estimated_usd,
        started_at: Some(latest_run.started_at.to_rfc3339()),
        completed_at: latest_run
            .completed_at
            .map(|timestamp| timestamp.to_rfc3339()),
        timestamp_provenance: Some("delegated_run".to_string()),
    }))
}

fn apply_delegated_task_snapshot(task: &mut ActiveStreamingTask, snapshot: &DelegatedTaskSnapshot) {
    task.status = snapshot.status.clone();
    task.delegated_conversation_id = snapshot.delegated_conversation_id.clone();
    task.delegated_agent_run_id = snapshot.delegated_agent_run_id.clone();
    task.provider_harness = snapshot.provider_harness.clone();
    task.provider_session_id = snapshot.provider_session_id.clone();
    task.upstream_provider = snapshot.upstream_provider.clone();
    task.provider_profile = snapshot.provider_profile.clone();
    task.logical_model = snapshot.logical_model.clone();
    task.effective_model_id = snapshot.effective_model_id.clone();
    task.logical_effort = snapshot.logical_effort.clone();
    task.effective_effort = snapshot.effective_effort.clone();
    task.approval_policy = snapshot.approval_policy.clone();
    task.sandbox_mode = snapshot.sandbox_mode.clone();
    task.total_tokens = snapshot.total_tokens;
    task.duration_ms = snapshot.duration_ms;
    task.input_tokens = snapshot.input_tokens;
    task.output_tokens = snapshot.output_tokens;
    task.cache_creation_tokens = snapshot.cache_creation_tokens;
    task.cache_read_tokens = snapshot.cache_read_tokens;
    task.estimated_usd = snapshot.estimated_usd;
    if snapshot.started_at.is_some() {
        task.started_at = snapshot.started_at.clone();
        task.completed_at = snapshot.completed_at.clone();
        task.timestamp_provenance = snapshot.timestamp_provenance.clone();
    }
}

/// GET /api/conversations/:id/active-state
///
/// Returns the current streaming state for a conversation, including:
/// - Whether an agent is currently running for this conversation
/// - Active tool calls in progress or recently completed
/// - Streaming tasks (subagents) currently running or completed
/// - Partial text content accumulated from agent:chunk events
///
/// Used by frontend to hydrate streaming UI when navigating to a conversation
/// where an agent execution was already in progress.
pub async fn get_conversation_active_state(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<ActiveStateResponse>, StatusCode> {
    // Check if any agent is running for this conversation
    // We need to check all context types since a conversation belongs to a specific context
    // The RunningAgentRegistry stores info by context_type + context_id, but we have conversation_id
    // We can look up the conversation to find its context, then check the registry

    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&crate::domain::entities::ChatConversationId::from_string(
            conversation_id.clone(),
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to get conversation: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Check if agent is running for this context
    let context_key = RunningAgentKey::new(
        conversation.context_type.to_string(),
        conversation.context_id.clone(),
    );
    let is_active = state
        .app_state
        .running_agent_registry
        .is_running(&context_key)
        .await;

    // Get streaming state from cache
    let cached_state = state
        .app_state
        .streaming_state_cache
        .get(&conversation_id)
        .await;

    // Build response
    let response = if let Some(cached_state) = cached_state {
        let mut delegated_snapshot_cache =
            HashMap::<(String, Option<String>), Option<DelegatedTaskSnapshot>>::new();
        let mut streaming_tasks = Vec::with_capacity(cached_state.streaming_tasks.len());
        for task in cached_state
            .streaming_tasks
            .into_iter()
            .map(ActiveStreamingTask::from)
        {
            let mut active_task = task;
            if let Some(delegated_session_id) = active_task.delegated_session_id.clone() {
                let cache_key = (
                    delegated_session_id.clone(),
                    active_task.delegated_agent_run_id.clone(),
                );
                let snapshot = if let Some(snapshot) = delegated_snapshot_cache.get(&cache_key) {
                    snapshot.clone()
                } else {
                    let snapshot = load_delegated_task_snapshot(&state, &active_task).await?;
                    delegated_snapshot_cache.insert(cache_key, snapshot.clone());
                    snapshot
                };
                if let Some(snapshot) = snapshot.as_ref() {
                    apply_delegated_task_snapshot(&mut active_task, snapshot);
                }
            }
            streaming_tasks.push(active_task);
        }

        ActiveStateResponse {
            is_active,
            run_id: cached_state.run_id,
            tool_calls: cached_state
                .tool_calls
                .into_iter()
                .map(ActiveToolCall::from)
                .collect(),
            streaming_tasks,
            partial_text: cached_state.partial_text,
            partial_text_segments: cached_state.partial_text_segments,
            partial_thinking_segments: cached_state.partial_thinking_segments,
        }
    } else {
        ActiveStateResponse {
            is_active,
            run_id: None,
            tool_calls: Vec::new(),
            streaming_tasks: Vec::new(),
            partial_text: String::new(),
            partial_text_segments: Vec::new(),
            partial_thinking_segments: Vec::new(),
        }
    };

    Ok(Json(response))
}
