use std::sync::Arc;

use crate::domain::entities::{ChatContextType, ChatConversationId, ProjectId};
use crate::domain::repositories::{ChatConversationRepository, ProjectRepository};
use crate::domain::services::{MessageQueue, QueueKey, RunningAgentRegistry};

pub(crate) fn workspace_capacity_available(
    active_workspaces: u32,
    workspace_max: u32,
    slot_running_count: u32,
    global_max: u32,
    paused: bool,
    provider_blocked: bool,
) -> bool {
    !paused
        && !provider_blocked
        && active_workspaces < workspace_max
        && slot_running_count.saturating_add(active_workspaces) < global_max
}

pub(crate) async fn resolve_project_queue_context(
    key: &QueueKey,
    project_repo: &Arc<dyn ProjectRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
) -> Result<Option<(ProjectId, Option<ChatConversationId>)>, String> {
    if key.context_type != ChatContextType::Project {
        return Ok(Some((ProjectId::from_string(key.context_id.clone()), None)));
    }

    let project_id = ProjectId::from_string(key.context_id.clone());
    if project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(Some((project_id, None)));
    }

    let conversation_id = ChatConversationId::from_string(key.context_id.clone());
    let Some(conversation) = conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };

    if conversation.context_type != ChatContextType::Project {
        return Ok(None);
    }

    Ok(Some((
        ProjectId::from_string(conversation.context_id),
        Some(conversation.id),
    )))
}

pub(crate) async fn queue_key_matches_workspace_project(
    key: &QueueKey,
    project_filter: Option<&ProjectId>,
    project_repo: &Arc<dyn ProjectRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
) -> Result<bool, String> {
    let Some(project_id) = project_filter else {
        return Ok(true);
    };

    Ok(
        resolve_project_queue_context(key, project_repo, conversation_repo)
            .await?
            .is_some_and(|(resolved_project_id, _)| resolved_project_id == *project_id),
    )
}

pub(crate) async fn count_queued_workspace_messages(
    message_queue: &MessageQueue,
    project_repo: &Arc<dyn ProjectRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    project_filter: Option<&ProjectId>,
) -> Result<u32, String> {
    let mut count = 0u32;
    for key in message_queue.list_keys() {
        if key.context_type != ChatContextType::Project {
            continue;
        }
        if !queue_key_matches_workspace_project(
            &key,
            project_filter,
            project_repo,
            conversation_repo,
        )
        .await?
        {
            continue;
        }
        count += message_queue.get_queued_with_key(&key).len() as u32;
    }
    Ok(count)
}

pub(crate) async fn count_active_workspace_sessions(
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    project_repo: &Arc<dyn ProjectRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    project_filter: Option<&ProjectId>,
) -> Result<u32, String> {
    let mut count = 0u32;
    for (key, info) in running_agent_registry.list_all().await {
        if key.context_type != ChatContextType::Project.to_string() || info.pid == 0 {
            continue;
        }

        let Some(project_id) = resolve_workspace_project_id(
            &key.context_id,
            &info.conversation_id,
            project_repo,
            conversation_repo,
        )
        .await?
        else {
            continue;
        };

        if project_filter.is_some_and(|filter| project_id != *filter) {
            continue;
        }

        count += 1;
    }
    Ok(count)
}

async fn resolve_workspace_project_id(
    runtime_context_id: &str,
    conversation_id: &str,
    project_repo: &Arc<dyn ProjectRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
) -> Result<Option<ProjectId>, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id.to_string());
    if let Some(conversation) = conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    {
        if conversation.context_type == ChatContextType::Project {
            return Ok(Some(ProjectId::from_string(conversation.context_id)));
        }
    }

    let project_id = ProjectId::from_string(runtime_context_id.to_string());
    if project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(Some(project_id));
    }

    let runtime_conversation_id = ChatConversationId::from_string(runtime_context_id.to_string());
    let Some(conversation) = conversation_repo
        .get_by_id(&runtime_conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };

    if conversation.context_type != ChatContextType::Project {
        return Ok(None);
    }

    Ok(Some(ProjectId::from_string(conversation.context_id)))
}
