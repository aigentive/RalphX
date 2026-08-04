use std::sync::Arc;

use std::collections::HashSet;

use crate::application::chat_service::{
    uses_execution_slot, ChatService, SendCallerContext, SendMessageOptions,
};
use crate::application::execution_running::context_matches_running_status_for_gc;
use crate::application::{AppState, ChatServiceError, ExecutionState};
use crate::domain::entities::{
    app_state::ExecutionHaltMode, ChatContextType, ChatConversationId, IdeationSessionId,
    IdeationSessionStatus, InternalStatus, ProjectId, TaskId,
};

use crate::domain::services::{QueueKey, QueuedMessage};

pub(crate) async fn persist_execution_halt_mode(
    app_state: &AppState,
    halt_mode: ExecutionHaltMode,
) -> Result<(), String> {
    app_state
        .app_state_repo
        .set_execution_halt_mode(halt_mode)
        .await
        .map_err(|e| e.to_string())
}

pub(crate) fn execution_halt_mode_str(halt_mode: ExecutionHaltMode) -> &'static str {
    match halt_mode {
        ExecutionHaltMode::Running => "running",
        ExecutionHaltMode::Paused => "paused",
        ExecutionHaltMode::Stopped => "stopped",
    }
}

pub(crate) async fn load_execution_halt_mode(
    app_state: &AppState,
) -> Result<ExecutionHaltMode, String> {
    app_state
        .app_state_repo
        .get()
        .await
        .map(|settings| settings.execution_halt_mode)
        .map_err(|e| e.to_string())
}

pub(crate) fn queued_message_to_send_options(
    message: &crate::domain::services::QueuedMessage,
) -> SendMessageOptions {
    let created_at = message
        .created_at_override
        .as_deref()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|ts| ts.with_timezone(&chrono::Utc));

    SendMessageOptions {
        metadata: message.metadata_override.clone(),
        created_at,
        harness_override: message.harness_override,
        model_override: message.model_override.clone(),
        logical_effort_override: message.logical_effort_override,
        service_tier_override: message.service_tier_override.clone(),
        composer_project_references: message.composer_project_references.clone(),
        composer_integration_references: message.composer_integration_references.clone(),
        composer_artifact_references: message.composer_artifact_references.clone(),
        composer_excerpt_references: message.composer_excerpt_references.clone(),
        attachment_ids: message.attachment_ids.clone(),
        ..Default::default()
    }
}

async fn queued_keys(app_state: &AppState) -> Result<Vec<QueueKey>, String> {
    let mut keys = app_state.message_queue.list_keys();
    let mut seen: HashSet<QueueKey> = keys.iter().cloned().collect();
    for key in app_state
        .queued_message_repo
        .list_keys()
        .await
        .map_err(|error| error.to_string())?
    {
        if seen.insert(key.clone()) {
            keys.push(key);
        }
    }
    Ok(keys)
}

async fn queued_messages_for_key(
    app_state: &AppState,
    key: &QueueKey,
) -> Result<Vec<QueuedMessage>, String> {
    let memory = app_state.message_queue.get_queued_with_key(key);
    let durable = app_state
        .queued_message_repo
        .list(key)
        .await
        .map_err(|error| error.to_string())?;
    let mut seen: HashSet<String> = durable.iter().map(|message| message.id.clone()).collect();
    let mut merged = durable;
    for message in memory {
        if seen.insert(message.id.clone()) {
            merged.push(message);
        }
    }
    Ok(merged)
}

async fn clear_queued_key(app_state: &AppState, key: &QueueKey) -> Result<(), String> {
    app_state.message_queue.clear_with_key(key);
    app_state
        .queued_message_repo
        .clear(key)
        .await
        .map_err(|error| error.to_string())
}

async fn pop_queued_key(
    app_state: &AppState,
    key: &QueueKey,
) -> Result<Option<QueuedMessage>, String> {
    if let Some(message) = app_state.message_queue.pop_with_key(key) {
        app_state
            .queued_message_repo
            .delete(key, &message.id)
            .await
            .map_err(|error| error.to_string())?;
        return Ok(Some(message));
    }
    app_state
        .queued_message_repo
        .pop_front(key)
        .await
        .map_err(|error| error.to_string())
}

/// True when the send failed only AFTER the live process already accepted the turn.
/// Restoring such a message to the queue would deliver it to the agent twice.
fn queued_send_reached_the_agent(error: &ChatServiceError) -> bool {
    matches!(error, ChatServiceError::MessageDeliveredNotPersisted(_))
}

async fn restore_queued_front(
    app_state: &AppState,
    key: &QueueKey,
    message: QueuedMessage,
) -> Result<(), String> {
    app_state.message_queue.queue_front_existing(
        key.context_type,
        key.context_id.clone(),
        message.clone(),
    );
    app_state
        .queued_message_repo
        .enqueue_front(key, &message)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) fn is_pause_managed_chat_context(context_type: ChatContextType) -> bool {
    matches!(
        context_type,
        ChatContextType::TaskExecution
            | ChatContextType::Review
            | ChatContextType::Merge
            | ChatContextType::Ideation
            | ChatContextType::Task
            | ChatContextType::Project
            | ChatContextType::Standalone
    )
}

pub(crate) fn is_ideation_registry_context(context_type: &str) -> bool {
    context_type == "ideation" || context_type == "session"
}

async fn resolve_project_queue_context(
    key: &QueueKey,
    app_state: &AppState,
) -> Result<Option<(String, Option<ChatConversationId>)>, String> {
    crate::application::workspace_capacity::resolve_project_queue_context(
        key,
        &app_state.project_repo,
        &app_state.chat_conversation_repo,
    )
    .await
    .map(|resolved| {
        resolved
            .map(|(project_id, conversation_id)| (project_id.as_str().to_string(), conversation_id))
    })
}

pub(crate) async fn queue_key_matches_project(
    key: &QueueKey,
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
) -> Result<bool, String> {
    let Some(project_id) = project_filter else {
        return Ok(true);
    };

    match key.context_type {
        ChatContextType::Ideation => {
            let session_id = IdeationSessionId::from_string(key.context_id.clone());
            let Some(session) = app_state
                .ideation_session_repo
                .get_by_id(&session_id)
                .await
                .map_err(|e| e.to_string())?
            else {
                return Ok(false);
            };
            Ok(session.project_id == *project_id)
        }
        ChatContextType::Delegation => {
            let session_id =
                crate::domain::entities::DelegatedSessionId::from_string(key.context_id.clone());
            let Some(session) = app_state
                .delegated_session_repo
                .get_by_id(&session_id)
                .await
                .map_err(|e| e.to_string())?
            else {
                return Ok(false);
            };
            Ok(session.project_id == *project_id)
        }
        ChatContextType::TaskExecution
        | ChatContextType::Review
        | ChatContextType::Merge
        | ChatContextType::BranchUpdate => {
            let task_id = TaskId::from_string(key.context_id.clone());
            let Some(task) = app_state
                .task_repo
                .get_by_id(&task_id)
                .await
                .map_err(|e| e.to_string())?
            else {
                return Ok(false);
            };
            Ok(task.project_id == *project_id)
        }
        ChatContextType::Task => {
            let task_id = TaskId::from_string(key.context_id.clone());
            let Some(task) = app_state
                .task_repo
                .get_by_id(&task_id)
                .await
                .map_err(|e| e.to_string())?
            else {
                return Ok(false);
            };
            Ok(task.project_id == *project_id)
        }
        ChatContextType::Project => Ok(resolve_project_queue_context(key, app_state)
            .await?
            .is_some_and(|(context_id, _)| context_id == project_id.as_str())),
        // Standalone conversations are projectless (self-keyed by conversation id),
        // so they never match a project filter.
        ChatContextType::Standalone => Ok(false),
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn clear_slot_consuming_queues(
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
) -> Result<u32, String> {
    let mut cleared = 0u32;
    for key in queued_keys(app_state).await? {
        if !uses_execution_slot(key.context_type) {
            continue;
        }
        if !queue_key_matches_project(&key, project_filter, app_state).await? {
            continue;
        }
        clear_queued_key(app_state, &key).await?;
        cleared += 1;
    }
    Ok(cleared)
}

pub(crate) async fn clear_paused_chat_queues(
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
) -> Result<u32, String> {
    let mut cleared = 0u32;
    for key in queued_keys(app_state).await? {
        if !is_pause_managed_chat_context(key.context_type) {
            continue;
        }
        if !queue_key_matches_project(&key, project_filter, app_state).await? {
            continue;
        }
        clear_queued_key(app_state, &key).await?;
        cleared += 1;
    }
    Ok(cleared)
}

pub(crate) async fn count_slot_consuming_queued_messages(
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
) -> Result<u32, String> {
    let mut count = 0u32;
    for key in queued_keys(app_state).await? {
        if !uses_execution_slot(key.context_type) {
            continue;
        }
        if !queue_key_matches_project(&key, project_filter, app_state).await? {
            continue;
        }
        count += queued_messages_for_key(app_state, &key).await?.len() as u32;
    }
    Ok(count)
}

pub(crate) async fn count_queued_messages_for_context_types(
    context_types: &[ChatContextType],
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
) -> Result<u32, String> {
    let mut count = 0u32;
    for key in queued_keys(app_state).await? {
        if !context_types.contains(&key.context_type) {
            continue;
        }
        if !queue_key_matches_project(&key, project_filter, app_state).await? {
            continue;
        }
        count += app_state.message_queue.get_queued_with_key(&key).len() as u32;
    }
    Ok(count)
}

#[doc(hidden)]
pub async fn count_active_workspace_sessions(
    app_state: &AppState,
    project_filter: Option<&ProjectId>,
) -> Result<u32, String> {
    crate::application::workspace_capacity::count_active_workspace_sessions(
        &app_state.running_agent_registry,
        &app_state.project_repo,
        &app_state.chat_conversation_repo,
        project_filter,
    )
    .await
}

pub(crate) async fn workspace_has_capacity_for_state(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> Result<bool, String> {
    let active_workspaces = count_active_workspace_sessions(app_state, None).await?;
    Ok(
        crate::application::workspace_capacity::workspace_capacity_available(
            active_workspaces,
            execution_state.workspace_max_concurrent(),
            execution_state.running_count(),
            execution_state.global_max_concurrent(),
            execution_state.is_paused(),
            execution_state.is_provider_blocked(),
        ),
    )
}

#[doc(hidden)]
pub async fn count_active_ideation_slots(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    project_filter: Option<&ProjectId>,
) -> Result<u32, String> {
    let registry_entries = app_state.running_agent_registry.list_all().await;
    let mut count = 0u32;

    for (key, info) in registry_entries {
        if info.pid == 0 || !is_ideation_registry_context(&key.context_type) {
            continue;
        }

        let session_id = IdeationSessionId::from_string(key.context_id.clone());
        let Some(session) = app_state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .map_err(|e| e.to_string())?
        else {
            continue;
        };

        if project_filter.is_some_and(|project_id| session.project_id != *project_id) {
            continue;
        }

        let slot_key = format!("{}/{}", key.context_type, key.context_id);
        if execution_state.is_interactive_idle(&slot_key) {
            continue;
        }

        count += 1;
    }

    Ok(count)
}

pub(crate) async fn count_active_slot_consuming_contexts_for_project(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    project_id: &ProjectId,
) -> Result<u32, String> {
    let registry_entries = app_state.running_agent_registry.list_all().await;
    let mut count = 0u32;

    for (key, info) in registry_entries {
        if info.pid == 0 {
            continue;
        }

        if is_ideation_registry_context(&key.context_type) {
            let session_id = IdeationSessionId::from_string(key.context_id.clone());
            let Some(session) = app_state
                .ideation_session_repo
                .get_by_id(&session_id)
                .await
                .map_err(|e| e.to_string())?
            else {
                continue;
            };

            if session.project_id != *project_id {
                continue;
            }

            let slot_key = format!("{}/{}", key.context_type, key.context_id);
            if execution_state.is_interactive_idle(&slot_key) {
                continue;
            }

            count += 1;
            continue;
        }

        let context_type = match key.context_type.parse::<ChatContextType>() {
            Ok(value) => value,
            Err(_) => continue,
        };

        if !uses_execution_slot(context_type) {
            continue;
        }

        let task_id = TaskId::from_string(key.context_id);
        let Some(task) = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .map_err(|e| e.to_string())?
        else {
            continue;
        };

        if task.project_id != *project_id
            || !context_matches_running_status_for_gc(context_type, task.internal_status)
        {
            continue;
        }

        count += 1;
    }

    Ok(count)
}

#[doc(hidden)]
pub async fn project_has_execution_capacity_for_state(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    project_id: &ProjectId,
) -> Result<bool, String> {
    let settings = app_state
        .execution_settings_repo
        .get_settings(Some(project_id))
        .await
        .map_err(|e| e.to_string())?;
    let running_project_total =
        count_active_slot_consuming_contexts_for_project(app_state, execution_state, project_id)
            .await?;

    Ok(execution_state
        .can_start_execution_context(running_project_total, settings.max_concurrent_tasks))
}

pub(crate) async fn has_runnable_execution_waiting(
    app_state: &AppState,
    project_filter: Option<&ProjectId>,
) -> Result<bool, String> {
    if let Some(project_id) = project_filter {
        let tasks = app_state
            .task_repo
            .get_by_project(project_id)
            .await
            .map_err(|e| e.to_string())?;
        if tasks
            .iter()
            .any(|task| task.internal_status == InternalStatus::Ready)
        {
            return Ok(true);
        }
    } else {
        let projects = app_state
            .project_repo
            .get_all()
            .await
            .map_err(|e| e.to_string())?;
        for project in projects {
            let tasks = app_state
                .task_repo
                .get_by_project(&project.id)
                .await
                .map_err(|e| e.to_string())?;
            if tasks
                .iter()
                .any(|task| task.internal_status == InternalStatus::Ready)
            {
                return Ok(true);
            }
        }
    }

    for key in queued_keys(app_state).await? {
        match key.context_type {
            ChatContextType::Project => {
                if queue_key_matches_project(&key, project_filter, app_state).await? {
                    return Ok(true);
                }
            }
            ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge => {
                let task_id = TaskId::from_string(key.context_id.clone());
                let Some(task) = app_state
                    .task_repo
                    .get_by_id(&task_id)
                    .await
                    .map_err(|e| e.to_string())?
                else {
                    continue;
                };

                if project_filter.is_none_or(|project_id| task.project_id == *project_id) {
                    return Ok(true);
                }
            }
            _ => {}
        }
    }

    Ok(false)
}

pub(crate) async fn resume_paused_ideation_queues_with_chat_service<F>(
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    build_chat_service: F,
) -> Result<u32, String>
where
    F: Fn() -> Arc<dyn ChatService>,
{
    let mut resumed = 0u32;
    let mut ideation_keys = Vec::new();
    for key in queued_keys(app_state).await? {
        if key.context_type != ChatContextType::Ideation {
            continue;
        }

        let session_id = IdeationSessionId::from_string(key.context_id.clone());
        let project_sort_key = app_state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .map_err(|e| e.to_string())?
            .map(|session| session.project_id.as_str().to_string())
            .unwrap_or_default();

        ideation_keys.push((project_sort_key, key.context_id.clone(), key));
    }
    ideation_keys.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    for (_, _, key) in ideation_keys {
        if !queue_key_matches_project(&key, project_filter, app_state).await? {
            continue;
        }

        let session_id = IdeationSessionId::from_string(key.context_id.clone());
        let Some(session) = app_state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .map_err(|e| e.to_string())?
        else {
            clear_queued_key(app_state, &key).await?;
            continue;
        };

        if session.status != IdeationSessionStatus::Active {
            clear_queued_key(app_state, &key).await?;
            continue;
        }

        let project_settings = app_state
            .execution_settings_repo
            .get_settings(Some(&session.project_id))
            .await
            .map_err(|e| e.to_string())?;
        let running_global_ideation =
            count_active_ideation_slots(app_state, execution_state, None).await?;
        let running_project_ideation =
            count_active_ideation_slots(app_state, execution_state, Some(&session.project_id))
                .await?;
        let running_project_total = count_active_slot_consuming_contexts_for_project(
            app_state,
            execution_state,
            &session.project_id,
        )
        .await?;
        let global_execution_waiting = has_runnable_execution_waiting(app_state, None).await?;
        let project_execution_waiting =
            has_runnable_execution_waiting(app_state, Some(&session.project_id)).await?;
        if !execution_state.can_start_ideation(
            running_global_ideation,
            running_project_ideation,
            running_project_total,
            project_settings.max_concurrent_tasks,
            project_settings.project_ideation_max,
            global_execution_waiting,
            project_execution_waiting,
        ) {
            let global_ideation_allows = if running_global_ideation
                < execution_state.global_ideation_max()
            {
                true
            } else {
                execution_state.allow_ideation_borrow_idle_execution() && !global_execution_waiting
            };

            if !execution_state.can_start_any_execution_context() || !global_ideation_allows {
                break;
            }

            continue;
        }

        let Some(queued) = pop_queued_key(app_state, &key).await? else {
            continue;
        };

        let send_result = build_chat_service()
            .send_message(
                ChatContextType::Ideation,
                session.id.as_str(),
                &queued.content,
                queued_message_to_send_options(&queued),
            )
            .await;

        match send_result {
            Ok(_) => {
                resumed += 1;
            }
            Err(error) => {
                if !queued_send_reached_the_agent(&error) {
                    restore_queued_front(app_state, &key, queued).await?;
                }
                tracing::warn!(
                    session_id = session.id.as_str(),
                    error = %error,
                    "Failed to relaunch paused ideation queue item on resume"
                );
                break;
            }
        }
    }

    Ok(resumed)
}

pub(crate) async fn resume_paused_workspace_queues_with_chat_service<F>(
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    build_chat_service: F,
) -> Result<u32, String>
where
    F: Fn() -> Arc<dyn ChatService>,
{
    let mut resumed = 0u32;
    let mut workspace_keys = Vec::new();

    for key in queued_keys(app_state).await? {
        if key.context_type != ChatContextType::Project {
            continue;
        }
        if !queue_key_matches_project(&key, project_filter, app_state).await? {
            continue;
        }
        let project_sort_key = resolve_project_queue_context(&key, app_state)
            .await?
            .map(|(context_id, _)| context_id)
            .unwrap_or_default();
        workspace_keys.push((project_sort_key, key.context_id.clone(), key));
    }

    workspace_keys.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    for (_, _, key) in workspace_keys {
        if !workspace_has_capacity_for_state(app_state, execution_state).await? {
            break;
        }

        let Some((send_context_id, conversation_id)) =
            resolve_project_queue_context(&key, app_state).await?
        else {
            continue;
        };

        let Some(queued) = pop_queued_key(app_state, &key).await? else {
            continue;
        };

        let mut options = queued_message_to_send_options(&queued);
        options.conversation_id_override = conversation_id;
        options.caller_context = SendCallerContext::DrainService;

        let send_result = build_chat_service()
            .send_message(
                ChatContextType::Project,
                &send_context_id,
                &queued.content,
                options,
            )
            .await;

        match send_result {
            Ok(result) if result.was_queued => {}
            Ok(_) => resumed += 1,
            Err(error) => {
                tracing::warn!(
                    context_type = %key.context_type,
                    context_id = key.context_id,
                    error = %error,
                    "Failed to relaunch paused workspace queued message"
                );
                if !queued_send_reached_the_agent(&error) {
                    restore_queued_front(app_state, &key, queued).await?;
                }
                break;
            }
        }
    }

    Ok(resumed)
}

pub(crate) async fn resume_paused_non_slot_chat_queues_with_chat_service<F>(
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
    build_chat_service: F,
) -> Result<u32, String>
where
    F: Fn() -> Arc<dyn ChatService>,
{
    let mut resumed = 0u32;
    let mut chat_keys = Vec::new();

    for key in queued_keys(app_state).await? {
        // Task: task-linked chat queue, sorted by owning project. Standalone:
        // self-keyed projectless chat queue (no project to sort by) — see
        // `is_pause_managed_chat_context` / `should_requeue_after_provider_pause`,
        // which both already admit Standalone at the pause layer; this is its
        // matching resume-drain arm (pause-drain parity, Phase 4a.3).
        if !matches!(
            key.context_type,
            ChatContextType::Task | ChatContextType::Standalone
        ) {
            continue;
        }
        if !queue_key_matches_project(&key, project_filter, app_state).await? {
            continue;
        }
        let project_sort_key = if key.context_type == ChatContextType::Task {
            let task_id = TaskId::from_string(key.context_id.clone());
            app_state
                .task_repo
                .get_by_id(&task_id)
                .await
                .map_err(|e| e.to_string())?
                .map(|task| task.project_id.as_str().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        chat_keys.push((
            project_sort_key,
            key.context_type.to_string(),
            key.context_id.clone(),
            key,
        ));
    }

    chat_keys.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    for (_, _, _, key) in chat_keys {
        let Some(queued) = pop_queued_key(app_state, &key).await? else {
            continue;
        };

        let options = queued_message_to_send_options(&queued);

        let send_result = build_chat_service()
            .send_message(key.context_type, &key.context_id, &queued.content, options)
            .await;

        match send_result {
            Ok(_) => resumed += 1,
            Err(error) => {
                tracing::warn!(
                    context_type = %key.context_type,
                    context_id = key.context_id,
                    error = %error,
                    "Failed to relaunch paused non-slot queued message"
                );
                if !queued_send_reached_the_agent(&error) {
                    restore_queued_front(app_state, &key, queued).await?;
                }
            }
        }
    }

    Ok(resumed)
}

pub(crate) async fn resume_paused_slot_consuming_queues_with_chat_service<F>(
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    build_chat_service: F,
) -> Result<u32, String>
where
    F: Fn() -> Arc<dyn ChatService>,
{
    let mut resumed = 0u32;
    let mut slot_keys = Vec::new();

    for key in queued_keys(app_state).await? {
        if !matches!(
            key.context_type,
            ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge
        ) {
            continue;
        }

        let task_id = TaskId::from_string(key.context_id.clone());
        let project_sort_key = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .map_err(|e| e.to_string())?
            .map(|task| task.project_id.as_str().to_string())
            .unwrap_or_default();

        slot_keys.push((
            project_sort_key,
            key.context_type.to_string(),
            key.context_id.clone(),
            key,
        ));
    }

    slot_keys.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    for (_, _, _, key) in slot_keys {
        let task_id = TaskId::from_string(key.context_id.clone());
        let Some(task) = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .map_err(|e| e.to_string())?
        else {
            continue;
        };

        if project_filter.is_some_and(|project_id| task.project_id != *project_id) {
            continue;
        }

        if !context_matches_running_status_for_gc(key.context_type, task.internal_status) {
            continue;
        }

        let slot_key = format!("{}/{}", key.context_type, key.context_id);
        if execution_state.is_interactive_idle(&slot_key) {
            continue;
        }

        if !project_has_execution_capacity_for_state(app_state, execution_state, &task.project_id)
            .await?
        {
            continue;
        }

        let Some(queued) = pop_queued_key(app_state, &key).await? else {
            continue;
        };

        let chat_service = build_chat_service();
        let send_result = chat_service
            .send_message(
                key.context_type,
                &key.context_id,
                &queued.content,
                queued_message_to_send_options(&queued),
            )
            .await;

        match send_result {
            Ok(_) => resumed += 1,
            Err(error) => {
                tracing::warn!(
                    context_type = %key.context_type,
                    context_id = key.context_id,
                    error = %error,
                    "Failed to relaunch paused slot-consuming queued message"
                );
                if !queued_send_reached_the_agent(&error) {
                    restore_queued_front(app_state, &key, queued).await?;
                }
            }
        }
    }

    Ok(resumed)
}
