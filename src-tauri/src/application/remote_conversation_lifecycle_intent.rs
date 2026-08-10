use crate::application::{
    agent_conversation_archive::workspace_allows_pr_closure,
    agent_conversation_fork::validate_forkable_parent, AppState,
};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversationId,
    RemoteConversationLifecycleKind, RemoteConversationLifecycleRequest,
    RemoteConversationLifecycleStatus,
};
use serde::{Deserialize, Serialize};

pub const LOOKUP_FAILED: &str = "REMOTE_CONVERSATION_LIFECYCLE_LOOKUP_FAILED";
pub const NOT_FOUND: &str = "REMOTE_CONVERSATION_LIFECYCLE_NOT_FOUND";
pub const NOT_PROJECT: &str = "REMOTE_CONVERSATION_LIFECYCLE_NOT_PROJECT";
pub const ALREADY_ARCHIVED: &str = "REMOTE_CONVERSATION_LIFECYCLE_ALREADY_ARCHIVED";
pub const PARENT_NOT_FORKABLE: &str = "REMOTE_CONVERSATION_LIFECYCLE_PARENT_NOT_FORKABLE";
pub const CHILD_EXISTS: &str = "REMOTE_CONVERSATION_LIFECYCLE_CHILD_EXISTS";
pub const PR_CLOSURE_FORBIDDEN: &str = "REMOTE_CONVERSATION_LIFECYCLE_PR_CLOSURE_FORBIDDEN";
pub const HOST_FAILED: &str = "REMOTE_CONVERSATION_LIFECYCLE_HOST_FAILED";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteConversationArchiveInput {
    pub conversation_id: String,
    pub close_pull_request: bool,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteConversationForkInput {
    pub conversation_id: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConversationLifecycleIntentResponse {
    pub request_id: String,
    pub allocated_conversation_id: Option<String>,
    pub status: RemoteConversationLifecycleStatus,
    pub deduplicated: bool,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConversationLifecycleRequestView {
    pub request_id: String,
    pub kind: RemoteConversationLifecycleKind,
    pub conversation_id: String,
    pub allocated_conversation_id: Option<String>,
    pub status: RemoteConversationLifecycleStatus,
    pub error_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

async fn load(
    state: &AppState,
    id: &str,
) -> Result<crate::domain::entities::ChatConversation, String> {
    state
        .chat_conversation_repo
        .get_by_id(&ChatConversationId::from_string(id))
        .await
        .map_err(|_| LOOKUP_FAILED.to_string())?
        .ok_or_else(|| NOT_FOUND.to_string())
}
async fn unsettled(
    state: &AppState,
    id: &str,
) -> Result<Option<RemoteConversationLifecycleRequest>, String> {
    state
        .remote_conversation_lifecycle_request_repo
        .find_unsettled(id)
        .await
        .map_err(|_| LOOKUP_FAILED.to_string())
}
fn response(
    r: RemoteConversationLifecycleRequest,
    deduplicated: bool,
) -> RemoteConversationLifecycleIntentResponse {
    RemoteConversationLifecycleIntentResponse {
        request_id: r.id,
        allocated_conversation_id: r.allocated_conversation_id,
        status: r.status,
        deduplicated,
        created_at: r.created_at.to_rfc3339(),
    }
}
async fn persist(
    state: &AppState,
    kind: RemoteConversationLifecycleKind,
    id: String,
    close: bool,
    allocated: Option<String>,
) -> Result<RemoteConversationLifecycleIntentResponse, String> {
    let now = chrono::Utc::now();
    let row = RemoteConversationLifecycleRequest {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        conversation_id: id,
        close_pull_request: close,
        allocated_conversation_id: allocated,
        status: RemoteConversationLifecycleStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    state
        .remote_conversation_lifecycle_request_repo
        .create_remote_conversation_lifecycle_request(row)
        .await
        .map(|r| response(r, false))
        .map_err(|_| LOOKUP_FAILED.into())
}

pub async fn request_remote_conversation_archive_for_state(
    state: &AppState,
    input: RequestRemoteConversationArchiveInput,
) -> Result<RemoteConversationLifecycleIntentResponse, String> {
    let c = load(state, &input.conversation_id).await?;
    if c.context_type != ChatContextType::Project {
        return Err(NOT_PROJECT.into());
    }
    if c.is_archived() {
        return Err(ALREADY_ARCHIVED.into());
    }
    if input.close_pull_request {
        if let Some(w) = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&c.id)
            .await
            .map_err(|_| LOOKUP_FAILED.to_string())?
        {
            if !workspace_allows_pr_closure(&w) {
                return Err(PR_CLOSURE_FORBIDDEN.into());
            }
        }
    }
    if let Some(r) = unsettled(state, &input.conversation_id).await? {
        return Ok(response(r, true));
    }
    persist(
        state,
        RemoteConversationLifecycleKind::Archive,
        input.conversation_id,
        input.close_pull_request,
        None,
    )
    .await
}
pub async fn request_remote_conversation_fork_for_state(
    state: &AppState,
    input: RequestRemoteConversationForkInput,
) -> Result<RemoteConversationLifecycleIntentResponse, String> {
    let c = load(state, &input.conversation_id).await?;
    validate_forkable_parent(state, &c)
        .await
        .map_err(|_| PARENT_NOT_FORKABLE.to_string())?;
    let w = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&c.id)
        .await
        .map_err(|_| LOOKUP_FAILED.to_string())?;
    if w.as_ref().map(|w| w.mode).or(c.agent_mode) == Some(AgentConversationWorkspaceMode::Tasks) {
        return Err(PARENT_NOT_FORKABLE.into());
    }
    if let Some(r) = unsettled(state, &input.conversation_id).await? {
        return Ok(response(r, true));
    }
    persist(
        state,
        RemoteConversationLifecycleKind::Fork,
        input.conversation_id,
        false,
        Some(ChatConversationId::new().as_str()),
    )
    .await
}
pub async fn get_remote_conversation_lifecycle_request_for_state(
    state: &AppState,
    id: String,
) -> Result<RemoteConversationLifecycleRequestView, String> {
    let r = state
        .remote_conversation_lifecycle_request_repo
        .get(&id)
        .await
        .map_err(|_| LOOKUP_FAILED.to_string())?
        .ok_or_else(|| NOT_FOUND.to_string())?;
    Ok(RemoteConversationLifecycleRequestView {
        request_id: r.id,
        kind: r.kind,
        conversation_id: r.conversation_id,
        allocated_conversation_id: r.allocated_conversation_id,
        status: r.status,
        error_code: r.error_code,
        result: r.result,
        created_at: r.created_at.to_rfc3339(),
        updated_at: r.updated_at.to_rfc3339(),
    })
}
