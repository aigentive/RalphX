use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::agent_conversation_plan_import::{
    copy_agent_conversation_plan as copy_agent_conversation_plan_for_state,
    import_agent_conversation_plan_markdown as import_agent_conversation_plan_markdown_for_state,
    AgentConversationMarkdownImportRequest, AgentConversationPlanCopyRequest,
    AgentConversationPlanDraft,
};
use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    agent_workspace_response_for_state, AgentConversationWorkspaceResponse,
};
use crate::domain::entities::ChatConversationId;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyAgentConversationPlanInput {
    pub conversation_id: String,
    pub source_session_id: String,
    pub source_artifact_id: String,
    pub source_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAgentConversationPlanMarkdownInput {
    pub conversation_id: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct AgentConversationPlanDraftResponse {
    pub conversation_id: String,
    pub project_id: String,
    pub planning_session_id: String,
    pub plan_artifact_id: String,
    pub plan_artifact_version: u32,
    pub source_artifact_id: Option<String>,
    pub source_version: Option<u32>,
    pub workspace: AgentConversationWorkspaceResponse,
}

#[tauri::command]
pub async fn copy_agent_conversation_plan(
    input: CopyAgentConversationPlanInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationPlanDraftResponse, String> {
    let draft = copy_agent_conversation_plan_for_state(
        state.inner(),
        AgentConversationPlanCopyRequest {
            conversation_id: input.conversation_id,
            source_session_id: input.source_session_id,
            source_artifact_id: input.source_artifact_id,
            source_version: input.source_version,
        },
    )
    .await?;
    response_for_draft(state.inner(), draft).await
}

#[tauri::command]
pub async fn import_agent_conversation_plan_markdown(
    input: ImportAgentConversationPlanMarkdownInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationPlanDraftResponse, String> {
    let draft = import_agent_conversation_plan_markdown_for_state(
        state.inner(),
        AgentConversationMarkdownImportRequest {
            conversation_id: input.conversation_id,
            title: input.title,
            content: input.content,
        },
    )
    .await?;
    response_for_draft(state.inner(), draft).await
}

async fn response_for_draft(
    state: &AppState,
    draft: AgentConversationPlanDraft,
) -> Result<AgentConversationPlanDraftResponse, String> {
    let conversation_id = ChatConversationId::from_string(draft.conversation_id.clone());
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "Workspace not found for conversation {}",
                draft.conversation_id
            )
        })?;
    Ok(AgentConversationPlanDraftResponse {
        conversation_id: draft.conversation_id,
        project_id: draft.project_id,
        planning_session_id: draft.planning_session_id,
        plan_artifact_id: draft.plan_artifact_id,
        plan_artifact_version: draft.plan_artifact_version,
        source_artifact_id: draft.source_artifact_id,
        source_version: draft.source_version,
        workspace: agent_workspace_response_for_state(state, workspace).await?,
    })
}
