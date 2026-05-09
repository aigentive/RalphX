use std::collections::HashSet;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Map as JsonMap, Value as JsonValue};

use crate::application::chat_service::{ChatService, SendCallerContext, SendMessageOptions};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatContextType, ChatConversationId, ChatMessage, MessageRole, ProjectId, TaskId,
};
use crate::domain::repositories::{
    external_events_repository::ExternalEventRecord, AgentConversationWorkspaceRepository,
    ChatConversationRepository, ChatMessageRepository, ExternalEventsRepository, ProjectRepository,
    TaskRepository,
};
use crate::domain::services::MessageQueue;
use crate::error::{AppError, AppResult};

const LEGACY_BRIDGE_SOURCE: &str = "project_agent_ideation_bridge";
const WAKEUP_SOURCE: &str = "project_agent_workspace_bridge_wakeup";
const WAKEUP_MARKER_KIND: &str = "agent_workspace_bridge_delivery_marker";
const WAKEUP_MARKER_CONTENT: &str = "RalphX workflow bridge events were delivered.";
const MAX_BRIDGE_EVENT_REPLAY: i64 = 10_000;

#[derive(Clone)]
pub struct AgentWorkspaceBridgeDeps {
    pub project_repo: Arc<dyn ProjectRepository>,
    pub chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    pub chat_message_repo: Arc<dyn ChatMessageRepository>,
    pub agent_conversation_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    pub external_events_repo: Arc<dyn ExternalEventsRepository>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub message_queue: Arc<MessageQueue>,
}

impl AgentWorkspaceBridgeDeps {
    pub fn from_app_state(state: &AppState) -> Self {
        Self {
            project_repo: Arc::clone(&state.project_repo),
            chat_conversation_repo: Arc::clone(&state.chat_conversation_repo),
            chat_message_repo: Arc::clone(&state.chat_message_repo),
            agent_conversation_workspace_repo: Arc::clone(&state.agent_conversation_workspace_repo),
            external_events_repo: Arc::clone(&state.external_events_repo),
            task_repo: Arc::clone(&state.task_repo),
            message_queue: Arc::clone(&state.message_queue),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentWorkspaceBridgeEvent {
    pub event_key: String,
    pub event_type: String,
    pub external_event_id: i64,
    pub payload: JsonMap<String, JsonValue>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentWorkspaceBridgeWakeUp {
    pub conversation_id: ChatConversationId,
    pub project_id: String,
    pub source_session_id: String,
    pub event_keys: Vec<String>,
    pub content: String,
    pub metadata: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentWorkspaceBridgeWakeUpResult {
    pub conversation_id: String,
    pub agent_run_id: String,
    pub was_queued: bool,
    pub queued_message_id: Option<String>,
    pub event_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AgentWorkspaceBridgeDispatchSummary {
    pub project_count: usize,
    pub workspace_count: usize,
    pub wake_up_count: usize,
    pub queued_wake_up_count: usize,
    pub error_count: usize,
}

pub async fn dispatch_agent_workspace_bridge_events_once<S: ChatService + ?Sized>(
    state: &AppState,
    chat_service: &S,
) -> AppResult<AgentWorkspaceBridgeDispatchSummary> {
    let deps = AgentWorkspaceBridgeDeps::from_app_state(state);
    dispatch_agent_workspace_bridge_events_once_with_deps(&deps, chat_service).await
}

pub async fn dispatch_agent_workspace_bridge_events_once_with_deps<S: ChatService + ?Sized>(
    deps: &AgentWorkspaceBridgeDeps,
    chat_service: &S,
) -> AppResult<AgentWorkspaceBridgeDispatchSummary> {
    let mut summary = AgentWorkspaceBridgeDispatchSummary::default();
    let projects = deps.project_repo.get_all().await?;
    summary.project_count = projects.len();

    for project in projects {
        let workspaces = deps
            .agent_conversation_workspace_repo
            .get_by_project_id(&project.id)
            .await?;

        for workspace in workspaces {
            if !workspace_should_receive_bridge_events(&workspace) {
                continue;
            }
            summary.workspace_count += 1;

            match wake_agent_workspace_for_bridge_events_with_deps(
                deps,
                chat_service,
                &workspace.conversation_id,
            )
            .await
            {
                Ok(Some(result)) => {
                    summary.wake_up_count += 1;
                    if result.was_queued {
                        summary.queued_wake_up_count += 1;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    summary.error_count += 1;
                    tracing::warn!(
                        project_id = %project.id,
                        conversation_id = %workspace.conversation_id,
                        error = %error,
                        "Agent workspace bridge dispatch failed for linked workspace"
                    );
                }
            }
        }
    }

    Ok(summary)
}

fn workspace_should_receive_bridge_events(workspace: &AgentConversationWorkspace) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.mode == AgentConversationWorkspaceMode::Ideation
        && workspace.linked_ideation_session_id.is_some()
}

pub async fn wake_agent_workspace_for_bridge_events<S: ChatService + ?Sized>(
    state: &AppState,
    chat_service: &S,
    conversation_id: &ChatConversationId,
) -> AppResult<Option<AgentWorkspaceBridgeWakeUpResult>> {
    let deps = AgentWorkspaceBridgeDeps::from_app_state(state);
    wake_agent_workspace_for_bridge_events_with_deps(&deps, chat_service, conversation_id).await
}

pub async fn wake_agent_workspace_for_bridge_events_with_deps<S: ChatService + ?Sized>(
    deps: &AgentWorkspaceBridgeDeps,
    chat_service: &S,
    conversation_id: &ChatConversationId,
) -> AppResult<Option<AgentWorkspaceBridgeWakeUpResult>> {
    let Some(wake_up) =
        prepare_agent_workspace_bridge_wakeup_with_deps(deps, conversation_id).await?
    else {
        return Ok(None);
    };
    let event_count = wake_up.event_keys.len();
    let result = chat_service
        .send_message(
            ChatContextType::Project,
            &wake_up.project_id,
            &wake_up.content,
            SendMessageOptions {
                metadata: Some(wake_up.metadata),
                conversation_id_override: Some(wake_up.conversation_id),
                caller_context: SendCallerContext::DrainService,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| AppError::Agent(error.to_string()))?;

    if !result.was_queued {
        persist_hidden_wakeup_marker(
            deps,
            &wake_up.conversation_id,
            &wake_up.project_id,
            &wake_up.source_session_id,
            &wake_up.event_keys,
        )
        .await?;
    }

    Ok(Some(AgentWorkspaceBridgeWakeUpResult {
        conversation_id: result.conversation_id,
        agent_run_id: result.agent_run_id,
        was_queued: result.was_queued,
        queued_message_id: result.queued_message_id,
        event_count,
    }))
}

pub async fn prepare_agent_workspace_bridge_wakeup(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<Option<AgentWorkspaceBridgeWakeUp>> {
    let deps = AgentWorkspaceBridgeDeps::from_app_state(state);
    prepare_agent_workspace_bridge_wakeup_with_deps(&deps, conversation_id).await
}

pub async fn prepare_agent_workspace_bridge_wakeup_with_deps(
    deps: &AgentWorkspaceBridgeDeps,
    conversation_id: &ChatConversationId,
) -> AppResult<Option<AgentWorkspaceBridgeWakeUp>> {
    let Some(conversation) = deps
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await?
    else {
        return Ok(None);
    };
    if conversation.context_type != ChatContextType::Project {
        return Ok(None);
    }

    let Some(workspace) = deps
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(None);
    };

    let existing_messages = deps
        .chat_message_repo
        .get_by_conversation(conversation_id)
        .await?;
    let (mut delivered_event_keys, removed_invalid_count) =
        reconcile_legacy_bridge_messages(deps, conversation_id, &workspace, existing_messages)
            .await?;
    if removed_invalid_count > 0 {
        refresh_conversation_stats(deps, conversation_id).await?;
    }
    collect_queued_bridge_event_keys(deps, conversation_id, &mut delivered_event_keys);

    if !workspace_should_receive_bridge_events(&workspace) {
        return Ok(None);
    }

    let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
        return Ok(None);
    };
    let events = deps
        .external_events_repo
        .get_events_after_cursor(
            &[workspace.project_id.as_str().to_string()],
            0,
            MAX_BRIDGE_EVENT_REPLAY,
        )
        .await?;

    let mut new_events = Vec::new();
    for event in events {
        let Some(event) =
            bridge_event_for_workspace_session(deps, &event, session_id.as_str()).await?
        else {
            continue;
        };
        if !delivered_event_keys.contains(&event.event_key) {
            new_events.push(event);
        }
    }

    if new_events.is_empty() {
        return Ok(None);
    }

    Ok(Some(build_wakeup(
        *conversation_id,
        workspace.project_id.as_str().to_string(),
        session_id.as_str().to_string(),
        new_events,
    )))
}

async fn reconcile_legacy_bridge_messages(
    deps: &AgentWorkspaceBridgeDeps,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    messages: Vec<ChatMessage>,
) -> AppResult<(HashSet<String>, usize)> {
    let expected_session_id = workspace
        .linked_ideation_session_id
        .as_ref()
        .map(|id| id.as_str().to_string());
    let mut delivered_event_keys = HashSet::new();
    let mut visible_wakeup_event_keys = HashSet::new();
    let mut removed = 0;

    for message in messages {
        let Some(metadata) = parse_message_metadata(&message) else {
            continue;
        };

        if metadata_source(&metadata) == Some(WAKEUP_SOURCE) {
            let source_session_id = metadata
                .get("source_session_id")
                .or_else(|| metadata.get("sourceSessionId"))
                .and_then(JsonValue::as_str);
            if expected_session_id.as_deref() != source_session_id {
                deps.chat_message_repo.delete(&message.id).await?;
                removed += 1;
                continue;
            }
            let keys = wakeup_event_keys(&metadata);
            delivered_event_keys.extend(keys.iter().cloned());
            if !is_hidden_wakeup_marker(&message, &metadata) {
                visible_wakeup_event_keys.extend(keys);
                deps.chat_message_repo.delete(&message.id).await?;
                removed += 1;
            }
            continue;
        }

        if metadata_source(&metadata) != Some(LEGACY_BRIDGE_SOURCE) {
            continue;
        }

        let source_session_id = metadata
            .get("source_session_id")
            .or_else(|| metadata.get("sourceSessionId"))
            .and_then(JsonValue::as_str);
        let is_valid_owner = expected_session_id.as_deref() == source_session_id;
        if !is_valid_owner {
            deps.chat_message_repo.delete(&message.id).await?;
            removed += 1;
            continue;
        }

        if let Some(event_key) = metadata
            .get("bridge_event_key")
            .or_else(|| metadata.get("bridgeEventKey"))
            .and_then(JsonValue::as_str)
        {
            delivered_event_keys.insert(event_key.to_string());
        }
    }

    if !visible_wakeup_event_keys.is_empty() {
        let mut event_keys: Vec<String> = visible_wakeup_event_keys.into_iter().collect();
        event_keys.sort();
        if let Some(source_session_id) = expected_session_id.as_deref() {
            persist_hidden_wakeup_marker(
                deps,
                conversation_id,
                workspace.project_id.as_str(),
                source_session_id,
                &event_keys,
            )
            .await?;
        }
    }

    if removed > 0 {
        tracing::warn!(
            conversation_id = %conversation_id,
            removed,
            "Removed agent workspace bridge messages that did not match the workspace's linked ideation session"
        );
    }

    Ok((delivered_event_keys, removed))
}

fn collect_queued_bridge_event_keys(
    deps: &AgentWorkspaceBridgeDeps,
    conversation_id: &ChatConversationId,
    delivered_event_keys: &mut HashSet<String>,
) {
    let queue_context_id = conversation_id.as_str();
    for queued in deps
        .message_queue
        .get_queued(ChatContextType::Project, &queue_context_id)
    {
        let Some(metadata) = queued.metadata_override.as_deref() else {
            continue;
        };
        let Ok(JsonValue::Object(metadata)) = serde_json::from_str::<JsonValue>(metadata) else {
            continue;
        };
        if metadata_source(&metadata) == Some(WAKEUP_SOURCE) {
            collect_wakeup_event_keys(&metadata, delivered_event_keys);
        }
    }
}

fn collect_wakeup_event_keys(
    metadata: &JsonMap<String, JsonValue>,
    delivered_event_keys: &mut HashSet<String>,
) {
    delivered_event_keys.extend(wakeup_event_keys(metadata));
}

fn wakeup_event_keys(metadata: &JsonMap<String, JsonValue>) -> Vec<String> {
    let Some(keys) = metadata
        .get("bridge_event_keys")
        .or_else(|| metadata.get("bridgeEventKeys"))
        .and_then(JsonValue::as_array)
    else {
        return Vec::new();
    };
    keys.iter()
        .filter_map(JsonValue::as_str)
        .map(str::to_string)
        .collect()
}

fn is_hidden_wakeup_marker(message: &ChatMessage, metadata: &JsonMap<String, JsonValue>) -> bool {
    message.role == MessageRole::System
        && metadata
            .get("kind")
            .and_then(JsonValue::as_str)
            .is_some_and(|kind| kind == WAKEUP_MARKER_KIND)
        && metadata
            .get("hidden_from_ui")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
}

async fn persist_hidden_wakeup_marker(
    deps: &AgentWorkspaceBridgeDeps,
    conversation_id: &ChatConversationId,
    project_id: &str,
    source_session_id: &str,
    event_keys: &[String],
) -> AppResult<()> {
    if event_keys.is_empty() {
        return Ok(());
    }

    let metadata = hidden_wakeup_marker_metadata(source_session_id, event_keys);
    let mut marker = ChatMessage::user_in_project(
        ProjectId::from_string(project_id.to_string()),
        WAKEUP_MARKER_CONTENT,
    )
    .with_metadata(metadata);
    marker.role = MessageRole::System;
    marker.conversation_id = Some(*conversation_id);
    deps.chat_message_repo.create(marker).await?;
    Ok(())
}

fn hidden_wakeup_marker_metadata(source_session_id: &str, event_keys: &[String]) -> String {
    json!({
        "source": WAKEUP_SOURCE,
        "kind": WAKEUP_MARKER_KIND,
        "source_session_id": source_session_id,
        "bridge_event_keys": event_keys,
        "event_count": event_keys.len(),
        "hidden_from_ui": true,
        "recovery_context": true,
    })
    .to_string()
}

async fn refresh_conversation_stats(
    deps: &AgentWorkspaceBridgeDeps,
    conversation_id: &ChatConversationId,
) -> AppResult<()> {
    let messages = deps
        .chat_message_repo
        .get_by_conversation(conversation_id)
        .await?;
    if let Some(last_message) = messages.last() {
        deps.chat_conversation_repo
            .update_message_stats(
                conversation_id,
                messages.len() as i64,
                last_message.created_at,
            )
            .await?;
    }
    Ok(())
}

fn parse_message_metadata(message: &ChatMessage) -> Option<JsonMap<String, JsonValue>> {
    let metadata = message.metadata.as_deref()?;
    let JsonValue::Object(parsed) = serde_json::from_str::<JsonValue>(metadata).ok()? else {
        return None;
    };
    Some(parsed)
}

fn metadata_source(metadata: &JsonMap<String, JsonValue>) -> Option<&str> {
    metadata.get("source").and_then(JsonValue::as_str)
}

pub fn bridge_event_from_external_event(
    event: &ExternalEventRecord,
    session_id: &str,
) -> Option<AgentWorkspaceBridgeEvent> {
    let payload = parse_event_payload(event)?;
    if payload_session_id(&payload).as_deref() != Some(session_id) {
        return None;
    }
    bridge_event_from_payload(event, session_id, payload)
}

async fn bridge_event_for_workspace_session(
    deps: &AgentWorkspaceBridgeDeps,
    event: &ExternalEventRecord,
    session_id: &str,
) -> AppResult<Option<AgentWorkspaceBridgeEvent>> {
    let Some(payload) = parse_event_payload(event) else {
        return Ok(None);
    };
    let matches_session = match payload_session_id(&payload) {
        Some(payload_session_id) => payload_session_id == session_id,
        None => {
            task_session_id_for_payload(deps, &payload)
                .await?
                .as_deref()
                == Some(session_id)
        }
    };
    if !matches_session {
        return Ok(None);
    }

    Ok(bridge_event_from_payload(event, session_id, payload))
}

async fn task_session_id_for_payload(
    deps: &AgentWorkspaceBridgeDeps,
    payload: &JsonMap<String, JsonValue>,
) -> AppResult<Option<String>> {
    let Some(task_id) =
        string_field(payload, "task_id").or_else(|| string_field(payload, "taskId"))
    else {
        return Ok(None);
    };

    Ok(deps
        .task_repo
        .get_by_id(&TaskId::from_string(task_id))
        .await?
        .and_then(|task| task.ideation_session_id)
        .map(|session_id| session_id.as_str().to_string()))
}

fn bridge_event_from_payload(
    event: &ExternalEventRecord,
    session_id: &str,
    payload: JsonMap<String, JsonValue>,
) -> Option<AgentWorkspaceBridgeEvent> {
    let event_key = bridge_event_key(event, session_id, &payload)?;

    Some(AgentWorkspaceBridgeEvent {
        event_key,
        event_type: event.event_type.clone(),
        external_event_id: event.id,
        payload,
        created_at: parse_event_created_at(&event.created_at),
    })
}

fn bridge_event_key(
    event: &ExternalEventRecord,
    session_id: &str,
    payload: &JsonMap<String, JsonValue>,
) -> Option<String> {
    match event.event_type.as_str() {
        "ideation:plan_created" => Some(format!("ideation:{session_id}:plan_created")),
        "ideation:verified" => Some(format!("ideation:{session_id}:verified")),
        "ideation:proposals_ready" => Some(format!("ideation:{session_id}:proposals_ready")),
        "ideation:session_accepted" => Some(format!("ideation:{session_id}:session_accepted")),
        "task:execution_started" => Some(format!(
            "pipeline:{session_id}:task_execution_started:{}",
            task_identity(payload)
        )),
        "task:execution_completed" => Some(format!(
            "pipeline:{session_id}:task_execution_completed:{}",
            task_identity(payload)
        )),
        "merge:ready" => Some(format!(
            "pipeline:{session_id}:merge_ready:{}",
            task_identity(payload)
        )),
        "merge:completed" => {
            let commit =
                string_field(payload, "commit_sha").unwrap_or_else(|| event.id.to_string());
            Some(format!(
                "pipeline:{session_id}:merge_completed:{}:{commit}",
                task_identity(payload)
            ))
        }
        "task:status_changed" => {
            let new_status = string_field(payload, "new_status")?;
            if !["blocked", "failed", "merge_incomplete", "cancelled"]
                .contains(&new_status.as_str())
            {
                return None;
            }
            Some(format!(
                "pipeline:{session_id}:task_status:{}:{new_status}",
                task_identity(payload)
            ))
        }
        _ => None,
    }
}

fn build_wakeup(
    conversation_id: ChatConversationId,
    project_id: String,
    source_session_id: String,
    events: Vec<AgentWorkspaceBridgeEvent>,
) -> AgentWorkspaceBridgeWakeUp {
    let event_keys: Vec<_> = events.iter().map(|event| event.event_key.clone()).collect();
    let event_payload = JsonValue::Array(
        events
            .iter()
            .map(|event| {
                json!({
                    "bridge_event_key": event.event_key,
                    "external_event_id": event.external_event_id,
                    "event_type": event.event_type,
                    "created_at": event.created_at.to_rfc3339(),
                    "payload": event.payload,
                })
            })
            .collect(),
    );
    let payload_text =
        serde_json::to_string_pretty(&event_payload).unwrap_or_else(|_| event_payload.to_string());
    let event_word = if events.len() == 1 { "event" } else { "events" };
    let content = format!(
        "<!-- ralphx_internal_skill=ralphx-agent-workspace-swe -->\nRalphX workflow {event_word} arrived for this agent workspace. Use /ralphx-agent-workspace-swe skill. Review the payload and explain what changed. Only use tools for explicit intervention cases from that guidance; otherwise keep the response brief.\n\n```json\n{payload_text}\n```"
    );
    let metadata = json!({
        "source": WAKEUP_SOURCE,
        "source_session_id": source_session_id,
        "bridge_event_keys": event_keys,
        "event_count": events.len(),
        "resume_in_place": true,
        "persist_hidden_marker": true,
        "hidden_from_ui": true,
        "recovery_context": true,
    })
    .to_string();

    AgentWorkspaceBridgeWakeUp {
        conversation_id,
        project_id,
        source_session_id,
        event_keys,
        content,
        metadata,
    }
}

fn parse_event_payload(event: &ExternalEventRecord) -> Option<JsonMap<String, JsonValue>> {
    let JsonValue::Object(payload) = serde_json::from_str::<JsonValue>(&event.payload).ok()? else {
        return None;
    };
    Some(payload)
}

fn parse_event_created_at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn payload_session_id(payload: &JsonMap<String, JsonValue>) -> Option<String> {
    string_field(payload, "session_id").or_else(|| string_field(payload, "sessionId"))
}

fn task_identity(payload: &JsonMap<String, JsonValue>) -> String {
    string_field(payload, "task_id")
        .or_else(|| string_field(payload, "taskId"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn string_field(payload: &JsonMap<String, JsonValue>, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::application::chat_service::create_assistant_message;
    use crate::application::AppState;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversation, ChatMessage, IdeationAnalysisBaseRefKind,
        IdeationSessionId, MessageRole, Project, ProjectId, Task,
    };

    fn event(id: i64, event_type: &str, payload: JsonValue) -> ExternalEventRecord {
        ExternalEventRecord {
            id,
            event_type: event_type.to_string(),
            project_id: "project-1".to_string(),
            payload: payload.to_string(),
            created_at: "2026-04-27T20:39:07Z".to_string(),
        }
    }

    async fn create_workspace(
        state: &AppState,
        project_id: ProjectId,
        title: &str,
        linked_session_id: Option<&str>,
    ) -> ChatConversationId {
        let mode = if linked_session_id.is_some() {
            AgentConversationWorkspaceMode::Ideation
        } else {
            AgentConversationWorkspaceMode::Edit
        };
        create_workspace_with_mode(state, project_id, title, linked_session_id, mode).await
    }

    async fn create_workspace_with_mode(
        state: &AppState,
        project_id: ProjectId,
        title: &str,
        linked_session_id: Option<&str>,
        mode: AgentConversationWorkspaceMode,
    ) -> ChatConversationId {
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.title = Some(title.to_string());
        let conversation_id = conversation.id;
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .unwrap();

        let mut workspace = AgentConversationWorkspace::new(
            conversation_id,
            project_id,
            mode,
            IdeationAnalysisBaseRefKind::CurrentBranch,
            "main".to_string(),
            Some("Current branch".to_string()),
            None,
            format!("agent-{conversation_id}"),
            "/tmp/agent-workspace".to_string(),
        );
        workspace.linked_ideation_session_id =
            linked_session_id.map(|id| IdeationSessionId::from_string(id.to_string()));
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        conversation_id
    }

    async fn create_project(state: &AppState, project_id: &ProjectId) {
        let mut project = Project::new("Project".to_string(), "/tmp/project".to_string());
        project.id = project_id.clone();
        state.project_repo.create(project).await.unwrap();
    }

    #[test]
    fn maps_external_events_to_backend_bridge_event_keys() {
        let event = bridge_event_from_external_event(
            &event(
                10,
                "ideation:plan_created",
                json!({
                    "session_id": "session-1",
                    "plan_title": "Fix Font Scale Switching Regression"
                }),
            ),
            "session-1",
        )
        .unwrap();

        assert_eq!(event.event_key, "ideation:session-1:plan_created");
        assert_eq!(event.event_type, "ideation:plan_created");
    }

    #[tokio::test]
    async fn prepares_one_workspace_agent_wakeup_for_linked_events() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        create_project(&state, &project_id).await;
        let conversation_id = create_workspace(
            &state,
            project_id.clone(),
            "Linked workspace",
            Some("session-1"),
        )
        .await;

        state
            .external_events_repo
            .insert_event(
                "ideation:verified",
                project_id.as_str(),
                &json!({ "session_id": "session-1", "gap_score": 1 }).to_string(),
            )
            .await
            .unwrap();

        let wakeup = prepare_agent_workspace_bridge_wakeup(&state, &conversation_id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(wakeup.event_keys, vec!["ideation:session-1:verified"]);
        assert!(wakeup.content.contains("RalphX workflow event arrived"));
        assert!(wakeup.content.contains("\"gap_score\": 1"));
        let metadata: JsonValue = serde_json::from_str(&wakeup.metadata).unwrap();
        assert_eq!(metadata["source"], WAKEUP_SOURCE);

        let messages = state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .unwrap();
        assert!(
            messages.is_empty(),
            "bridge preparation must not write assistant messages"
        );
    }

    #[tokio::test]
    async fn does_not_prepare_wakeup_for_unlinked_edit_workspace() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        let conversation_id =
            create_workspace(&state, project_id.clone(), "Edit workspace", None).await;

        state
            .external_events_repo
            .insert_event(
                "ideation:verified",
                project_id.as_str(),
                &json!({ "session_id": "session-1" }).to_string(),
            )
            .await
            .unwrap();

        let wakeup = prepare_agent_workspace_bridge_wakeup(&state, &conversation_id)
            .await
            .unwrap();

        assert!(wakeup.is_none());
    }

    #[tokio::test]
    async fn does_not_prepare_wakeup_for_linked_non_ideation_workspace() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        let conversation_id = create_workspace_with_mode(
            &state,
            project_id.clone(),
            "Linked edit workspace",
            Some("session-1"),
            AgentConversationWorkspaceMode::Edit,
        )
        .await;

        state
            .external_events_repo
            .insert_event(
                "ideation:verified",
                project_id.as_str(),
                &json!({ "session_id": "session-1" }).to_string(),
            )
            .await
            .unwrap();

        let wakeup = prepare_agent_workspace_bridge_wakeup(&state, &conversation_id)
            .await
            .unwrap();

        assert!(wakeup.is_none());
    }

    #[tokio::test]
    async fn skips_events_already_persisted_as_workspace_wakeups() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        create_project(&state, &project_id).await;
        let conversation_id = create_workspace(
            &state,
            project_id.clone(),
            "Linked workspace",
            Some("session-1"),
        )
        .await;

        let mut wakeup_message = create_assistant_message(
            ChatContextType::Project,
            project_id.as_str(),
            "RalphX workflow event arrived.",
            conversation_id,
            &[],
            &[],
        );
        wakeup_message.metadata = Some(
            json!({
                "source": WAKEUP_SOURCE,
                "source_session_id": "session-1",
                "bridge_event_keys": ["ideation:session-1:verified"]
            })
            .to_string(),
        );
        state
            .chat_message_repo
            .create(wakeup_message)
            .await
            .unwrap();
        state
            .external_events_repo
            .insert_event(
                "ideation:verified",
                project_id.as_str(),
                &json!({ "session_id": "session-1" }).to_string(),
            )
            .await
            .unwrap();

        let wakeup = prepare_agent_workspace_bridge_wakeup(&state, &conversation_id)
            .await
            .unwrap();

        assert!(wakeup.is_none());
    }

    #[tokio::test]
    async fn skips_events_already_queued_as_workspace_wakeups() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        let conversation_id = create_workspace(
            &state,
            project_id.clone(),
            "Linked workspace",
            Some("session-1"),
        )
        .await;

        state.message_queue.queue_with_overrides(
            ChatContextType::Project,
            conversation_id.as_str(),
            "queued wakeup".to_string(),
            Some(
                json!({
                    "source": WAKEUP_SOURCE,
                    "bridge_event_keys": ["ideation:session-1:verified"]
                })
                .to_string(),
            ),
            None,
            None,
        );
        state
            .external_events_repo
            .insert_event(
                "ideation:verified",
                project_id.as_str(),
                &json!({ "session_id": "session-1" }).to_string(),
            )
            .await
            .unwrap();

        let wakeup = prepare_agent_workspace_bridge_wakeup(&state, &conversation_id)
            .await
            .unwrap();

        assert!(wakeup.is_none());
    }

    #[tokio::test]
    async fn removes_existing_legacy_bridge_messages_that_do_not_match_workspace_link() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        let conversation_id = create_workspace(&state, project_id, "Edit workspace", None).await;

        let mut bad_message = create_assistant_message(
            ChatContextType::Project,
            "project-1",
            "Plan verified for the attached ideation run.",
            conversation_id,
            &[],
            &[],
        );
        bad_message.metadata = Some(
            json!({
                "source": LEGACY_BRIDGE_SOURCE,
                "bridge_event_key": "ideation:session-1:verified",
                "source_session_id": "session-1"
            })
            .to_string(),
        );
        state.chat_message_repo.create(bad_message).await.unwrap();

        let wakeup = prepare_agent_workspace_bridge_wakeup(&state, &conversation_id)
            .await
            .unwrap();
        let messages = state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .unwrap();

        assert!(wakeup.is_none());
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn dispatches_new_events_to_linked_workspace_agent() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        create_project(&state, &project_id).await;
        let conversation_id = create_workspace(
            &state,
            project_id.clone(),
            "Linked workspace",
            Some("session-1"),
        )
        .await;
        state
            .external_events_repo
            .insert_event(
                "ideation:verified",
                project_id.as_str(),
                &json!({ "session_id": "session-1", "gap_score": 1 }).to_string(),
            )
            .await
            .unwrap();
        let chat_service = crate::application::MockChatService::new();

        let summary = dispatch_agent_workspace_bridge_events_once(&state, &chat_service)
            .await
            .unwrap();

        assert_eq!(summary.workspace_count, 1);
        assert_eq!(summary.wake_up_count, 1);
        assert_eq!(chat_service.call_count(), 1);
        let sent = chat_service.get_sent_messages().await;
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains("RalphX workflow event arrived"));
        assert!(sent[0].contains("\"gap_score\": 1"));
        let options = chat_service.get_sent_options().await;
        assert_eq!(
            options[0].conversation_id_override,
            Some(conversation_id),
            "dispatcher must send to the explicit linked workspace conversation"
        );
        let metadata: JsonValue = serde_json::from_str(
            options[0]
                .metadata
                .as_deref()
                .expect("bridge wake-up metadata"),
        )
        .unwrap();
        assert_eq!(metadata["resume_in_place"], true);
        assert_eq!(metadata["source"], WAKEUP_SOURCE);

        let messages = state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1, "bridge dispatch records one hidden marker");
        assert_eq!(messages[0].role, MessageRole::System);
        assert!(
            !messages[0].content.contains("gap_score"),
            "hidden marker must not persist the wake-up payload"
        );
        let marker_metadata: JsonValue =
            serde_json::from_str(messages[0].metadata.as_deref().unwrap()).unwrap();
        assert_eq!(marker_metadata["source"], WAKEUP_SOURCE);
        assert_eq!(marker_metadata["hidden_from_ui"], true);
        assert_eq!(marker_metadata["recovery_context"], true);
        assert_eq!(
            marker_metadata["bridge_event_keys"],
            json!(["ideation:session-1:verified"])
        );
    }

    #[tokio::test]
    async fn reconciles_visible_workspace_wakeup_messages_into_hidden_markers() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        create_project(&state, &project_id).await;
        let conversation_id = create_workspace(
            &state,
            project_id.clone(),
            "Linked workspace",
            Some("session-1"),
        )
        .await;

        let mut visible_wakeup = ChatMessage::user_in_project(
            project_id.clone(),
            "RalphX workflow event arrived.\n\n```json\n{\"gap_score\":1}\n```",
        );
        visible_wakeup.conversation_id = Some(conversation_id);
        visible_wakeup.metadata = Some(
            json!({
                "source": WAKEUP_SOURCE,
                "source_session_id": "session-1",
                "bridge_event_keys": ["ideation:session-1:verified"],
                "event_count": 1
            })
            .to_string(),
        );
        state
            .chat_message_repo
            .create(visible_wakeup)
            .await
            .unwrap();
        state
            .external_events_repo
            .insert_event(
                "ideation:verified",
                project_id.as_str(),
                &json!({ "session_id": "session-1", "gap_score": 1 }).to_string(),
            )
            .await
            .unwrap();

        let wakeup = prepare_agent_workspace_bridge_wakeup(&state, &conversation_id)
            .await
            .unwrap();
        let messages = state
            .chat_message_repo
            .get_by_conversation(&conversation_id)
            .await
            .unwrap();

        assert!(wakeup.is_none(), "reconciled event remains delivered");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::System);
        assert!(
            !messages[0].content.contains("gap_score"),
            "reconciled marker must not keep the visible wake-up payload"
        );
        let metadata: JsonValue =
            serde_json::from_str(messages[0].metadata.as_deref().unwrap()).unwrap();
        assert_eq!(metadata["source"], WAKEUP_SOURCE);
        assert_eq!(metadata["hidden_from_ui"], true);
        assert_eq!(metadata["recovery_context"], true);
        assert_eq!(
            metadata["bridge_event_keys"],
            json!(["ideation:session-1:verified"])
        );
    }

    #[tokio::test]
    async fn dispatches_task_events_by_task_ideation_session_link() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-1".to_string());
        let session_id = IdeationSessionId::from_string("session-1".to_string());
        create_project(&state, &project_id).await;
        let conversation_id = create_workspace(
            &state,
            project_id.clone(),
            "Linked workspace",
            Some(session_id.as_str()),
        )
        .await;
        let mut task = Task::new(project_id.clone(), "Implement plan".to_string());
        task.ideation_session_id = Some(session_id.clone());
        let task = state.task_repo.create(task).await.unwrap();
        state
            .external_events_repo
            .insert_event(
                "task:execution_completed",
                project_id.as_str(),
                &json!({
                    "task_id": task.id.as_str(),
                    "project_id": project_id.as_str(),
                    "outcome": "completed"
                })
                .to_string(),
            )
            .await
            .unwrap();
        let chat_service = crate::application::MockChatService::new();

        let summary = dispatch_agent_workspace_bridge_events_once(&state, &chat_service)
            .await
            .unwrap();

        assert_eq!(summary.wake_up_count, 1);
        let sent = chat_service.get_sent_messages().await;
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains("task:execution_completed"));
        assert!(sent[0].contains(task.id.as_str()));
        let options = chat_service.get_sent_options().await;
        assert_eq!(options[0].conversation_id_override, Some(conversation_id));
    }
}
