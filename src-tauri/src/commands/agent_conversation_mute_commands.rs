use chrono::Utc;
use serde::Deserialize;
use tauri::State;

use crate::application::AppState;
use crate::commands::agent_sidebar_commands::{
    attention_state_fingerprint, managed_team_activity_for_conversation,
    normalized_supervision_status, publication_state_for_workspace,
};
use crate::commands::unified_chat_commands::agent_workspace_response_for_state;
use crate::domain::entities::{AgentConversationMute, ChatConversationId};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAgentConversationMutedInput {
    pub conversation_id: String,
    pub muted: bool,
}

#[tauri::command]
pub async fn set_agent_conversation_muted(
    input: SetAgentConversationMutedInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    set_agent_conversation_muted_for_app_state(input, state.inner()).await
}

#[doc(hidden)]
pub async fn set_agent_conversation_muted_for_app_state(
    input: SetAgentConversationMutedInput,
    state: &AppState,
) -> Result<(), String> {
    let conversation_id = ChatConversationId::from_string(input.conversation_id);
    if !input.muted {
        return state
            .agent_conversation_mute_repo
            .clear(&conversation_id)
            .await
            .map_err(|error| error.to_string());
    }

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("agent conversation not found: {conversation_id}"))?;
    // The workspace goes through the same response projection the sidebar read
    // path uses. A linked plan branch can overlay the workspace's publication
    // status there, so deriving from the raw entity here would fingerprint a
    // state the sidebar never computes and the mute would never apply.
    let workspace = match state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    {
        Some(workspace) => Some(agent_workspace_response_for_state(state, workspace).await?),
        None => None,
    };
    let latest_run = state
        .agent_run_repo
        .get_latest_for_conversation(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let latest_run_status = latest_run.as_ref().map(|run| run.status);
    // Same Team-activity projection as the sidebar read path; a divergent
    // component here would produce a fingerprint the sidebar never computes.
    let managed_team_activity =
        managed_team_activity_for_conversation(state, &conversation_id).await?;
    let fingerprint = attention_state_fingerprint(
        conversation.is_archived(),
        publication_state_for_workspace(workspace.as_ref(), latest_run_status),
        latest_run.as_ref().map(|run| run.id.to_string()).as_deref(),
        latest_run_status,
        normalized_supervision_status(workspace.as_ref()).as_deref(),
        conversation.last_message_at,
        managed_team_activity
            .as_ref()
            .map(|activity| activity.fingerprint.as_str()),
    );
    state
        .agent_conversation_mute_repo
        .set_muted(AgentConversationMute {
            conversation_id,
            muted_at: Utc::now(),
            state_fingerprint: fingerprint,
        })
        .await
        .map_err(|error| error.to_string())
}
