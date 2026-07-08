use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::application::agent_terminal::{
    AgentTerminalCloseRequest, AgentTerminalOpenRequest, AgentTerminalResizeRequest,
    AgentTerminalSnapshot, AgentTerminalWorkspaceDeps, AgentTerminalWriteRequest,
};
use crate::domain::entities::ChatConversationId;
use crate::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTerminalOpenInput {
    pub conversation_id: String,
    pub terminal_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTerminalWriteInput {
    pub conversation_id: String,
    pub terminal_id: String,
    pub data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTerminalResizeInput {
    pub conversation_id: String,
    pub terminal_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTerminalCloseInput {
    pub conversation_id: String,
    pub terminal_id: String,
    pub delete_history: Option<bool>,
}

#[tauri::command]
pub async fn open_agent_terminal(
    input: AgentTerminalOpenInput,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<AgentTerminalSnapshot, String> {
    state
        .agent_terminal_service
        .open(
            AgentTerminalOpenRequest {
                conversation_id: ChatConversationId::from_string(input.conversation_id),
                terminal_id: input.terminal_id,
                cols: input.cols,
                rows: input.rows,
            },
            terminal_deps(&state),
            Some(app_handle),
        )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn write_agent_terminal(
    input: AgentTerminalWriteInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .agent_terminal_service
        .write(AgentTerminalWriteRequest {
            conversation_id: ChatConversationId::from_string(input.conversation_id),
            terminal_id: input.terminal_id,
            data: input.data,
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn resize_agent_terminal(
    input: AgentTerminalResizeInput,
    state: State<'_, AppState>,
) -> Result<AgentTerminalSnapshot, String> {
    state
        .agent_terminal_service
        .resize(AgentTerminalResizeRequest {
            conversation_id: ChatConversationId::from_string(input.conversation_id),
            terminal_id: input.terminal_id,
            cols: input.cols,
            rows: input.rows,
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn clear_agent_terminal(
    input: AgentTerminalCloseInput,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<AgentTerminalSnapshot, String> {
    state
        .agent_terminal_service
        .clear(
            ChatConversationId::from_string(input.conversation_id),
            input.terminal_id,
            Some(app_handle),
        )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn restart_agent_terminal(
    input: AgentTerminalOpenInput,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<AgentTerminalSnapshot, String> {
    state
        .agent_terminal_service
        .restart(
            AgentTerminalOpenRequest {
                conversation_id: ChatConversationId::from_string(input.conversation_id),
                terminal_id: input.terminal_id,
                cols: input.cols,
                rows: input.rows,
            },
            terminal_deps(&state),
            Some(app_handle),
        )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn close_agent_terminal(
    input: AgentTerminalCloseInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .agent_terminal_service
        .close(AgentTerminalCloseRequest {
            conversation_id: ChatConversationId::from_string(input.conversation_id),
            terminal_id: input.terminal_id,
            delete_history: input.delete_history.unwrap_or(false),
        })
        .await
        .map_err(|error| error.to_string())
}

fn terminal_deps<'a>(state: &'a AppState) -> AgentTerminalWorkspaceDeps<'a> {
    AgentTerminalWorkspaceDeps {
        chat_conversation_repo: &state.chat_conversation_repo,
        workspace_repo: &state.agent_conversation_workspace_repo,
        plan_branch_repo: &state.plan_branch_repo,
        project_repo: &state.project_repo,
    }
}
