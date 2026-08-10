use std::sync::Arc;

use chrono::Utc;
use serde::Deserialize;
use tauri::State;

use crate::application::AppState;
use crate::commands::agent_sidebar_commands::{
    attention_state_fingerprint, managed_team_activity_for_conversation,
    normalized_supervision_status, publication_state_for_workspace,
};
use crate::commands::unified_chat_commands::AgentConversationWorkspaceResponse;
use crate::commands::unified_chat_commands::{
    agent_workspace_response_with_pr_supervision_for_state,
    agent_workspace_response_without_repair_recovery_for_state,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationMute, AgentConversationWorkspace, ChatConversation, ChatConversationId,
};

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
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<(), String> {
    set_agent_conversation_muted_for_app_state(input, state.inner(), execution_state.inner()).await
}

/// The mute fingerprint needs the SAME workspace projection the sidebar read computes, and
/// the two callers differ in exactly one way: the local path is allowed to schedule PR
/// supervision repair recovery while projecting, the remote twin is not.
///
/// That difference is expressed as two separate functions rather than one shared body behind
/// a boolean, because the ledger's detectors are call-graph based: a shared body that NAMES
/// both builders makes the recovery scheduler — and everything it reaches, up to
/// `reconcile_reserved_claude_registration` and the corrective-transition sinks — appear in
/// the twin's closure no matter what the flag says at runtime. Measured 2026-08-05: the
/// boolean form failed `detector_c_floors_process_spawn_authority`,
/// `batch13_detector_gap_is_measured_not_inherited`, and
/// `no_registered_facade_target_reaches_a_corrective_transition`. Keep the projection call
/// in the caller and the shared work in `finish_agent_conversation_mute`, which names
/// neither builder.
#[doc(hidden)]
pub async fn set_agent_conversation_muted_for_app_state(
    input: SetAgentConversationMutedInput,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> Result<(), String> {
    let Some(target) = load_agent_conversation_mute_target(input, state).await? else {
        return Ok(());
    };
    let workspace = match target.workspace {
        Some(workspace) => Some(
            agent_workspace_response_with_pr_supervision_for_state(
                state,
                execution_state,
                workspace,
            )
            .await?,
        ),
        None => None,
    };
    finish_agent_conversation_mute(state, target.conversation, workspace).await
}

/// Spawn-free half of the pair: projects through the recovery-free builder ONLY. Its closure
/// must never name `agent_workspace_response_with_pr_supervision_for_state` — see the note above.
#[doc(hidden)]
pub async fn set_agent_conversation_muted_for_state_without_repair_recovery(
    input: SetAgentConversationMutedInput,
    state: &AppState,
) -> Result<(), String> {
    let Some(target) = load_agent_conversation_mute_target(input, state).await? else {
        return Ok(());
    };
    let workspace = match target.workspace {
        Some(workspace) => Some(
            agent_workspace_response_without_repair_recovery_for_state(state, workspace).await?,
        ),
        None => None,
    };
    finish_agent_conversation_mute(state, target.conversation, workspace).await
}

struct AgentConversationMuteTarget {
    conversation: ChatConversation,
    workspace: Option<AgentConversationWorkspace>,
}

/// Unmute settles here and yields `None`; a mute yields the conversation plus the raw
/// workspace entity for the caller to project.
async fn load_agent_conversation_mute_target(
    input: SetAgentConversationMutedInput,
    state: &AppState,
) -> Result<Option<AgentConversationMuteTarget>, String> {
    let conversation_id = ChatConversationId::from_string(input.conversation_id);
    if !input.muted {
        state
            .agent_conversation_mute_repo
            .clear(&conversation_id)
            .await
            .map_err(|error| error.to_string())?;
        return Ok(None);
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
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(AgentConversationMuteTarget {
        conversation,
        workspace,
    }))
}

async fn finish_agent_conversation_mute(
    state: &AppState,
    conversation: ChatConversation,
    workspace: Option<AgentConversationWorkspaceResponse>,
) -> Result<(), String> {
    let conversation_id = conversation.id.clone();
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
