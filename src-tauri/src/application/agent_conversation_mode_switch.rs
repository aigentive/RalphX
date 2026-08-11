use chrono::Utc;

use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, ChatConversationId,
};
use crate::error::{AppError, AppResult};

pub const AUTOMATION_RUN_MODE_LOCKED_ERROR_CODE: &str = "[ralphx:automation_run_mode_locked]";

pub(crate) fn is_automation_run_mode_switch_locked(conversation: &ChatConversation) -> bool {
    conversation.automation_run_id.is_some()
}

pub(crate) fn automation_run_mode_locked_error_message() -> String {
    format!(
        "{AUTOMATION_RUN_MODE_LOCKED_ERROR_CODE} Automation run conversations cannot be switched manually"
    )
}

pub(crate) async fn system_switch_automation_run_to_edit(
    conversation_id: &ChatConversationId,
    state: &AppState,
) -> AppResult<()> {
    system_switch_automation_run_mode(conversation_id, AgentConversationWorkspaceMode::Edit, state)
        .await
}

pub(crate) async fn system_switch_automation_run_to_ideation(
    conversation_id: &ChatConversationId,
    state: &AppState,
) -> AppResult<()> {
    system_switch_automation_run_mode(
        conversation_id,
        AgentConversationWorkspaceMode::Ideation,
        state,
    )
    .await
}

async fn system_switch_automation_run_mode(
    conversation_id: &ChatConversationId,
    mode: AgentConversationWorkspaceMode,
    state: &AppState,
) -> AppResult<()> {
    if !matches!(
        mode,
        AgentConversationWorkspaceMode::Edit | AgentConversationWorkspaceMode::Ideation
    ) {
        return Err(AppError::Validation(format!(
            "Automation run conversations cannot switch to {mode} mode"
        )));
    }
    let conversation = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "conversation not found: {}",
                conversation_id.as_str()
            ))
        })?;
    if conversation.context_type != ChatContextType::Project {
        return Err(AppError::Validation(
            "Only project agent conversations can change mode".to_string(),
        ));
    }
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "agent workspace not found for conversation {}",
                conversation_id.as_str()
            ))
        })?;

    let conversation_in_target_mode = conversation.agent_mode == Some(mode);
    let workspace_in_target_mode = workspace.mode == mode;
    if conversation_in_target_mode && workspace_in_target_mode {
        return Ok(());
    }

    if !matches!(
        workspace.mode,
        AgentConversationWorkspaceMode::Plan | AgentConversationWorkspaceMode::Edit
    ) {
        return Err(AppError::Validation(format!(
            "Automation run conversation {} cannot switch from {} to {} mode",
            conversation_id.as_str(),
            workspace.mode,
            mode
        )));
    }

    if !workspace_in_target_mode {
        workspace.mode = mode;
        workspace.updated_at = Utc::now();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await?;
    }

    if !conversation_in_target_mode {
        state
            .chat_conversation_repo
            .update_agent_mode(conversation_id, Some(mode))
            .await?;
    }

    Ok(())
}
