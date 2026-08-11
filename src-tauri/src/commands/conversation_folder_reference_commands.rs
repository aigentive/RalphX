use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::conversation_folder_reference_service::ConversationFolderReferenceService;
use crate::application::AppState;
use crate::domain::entities::{ChatContextType, ChatConversationId, ConversationFolderReferenceId};
use crate::error::AppError;
use crate::infrastructure::agents::limits_config;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddConversationFolderReferenceInput {
    pub conversation_id: String,
    pub folder_path: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveConversationFolderReferenceInput {
    pub conversation_id: String,
    pub folder_reference_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationFolderReferenceResponse {
    pub id: String,
    pub conversation_id: String,
    pub folder_path: String,
    pub display_name: String,
    pub created_at: String,
}

impl From<crate::domain::entities::ConversationFolderReference>
    for ConversationFolderReferenceResponse
{
    fn from(reference: crate::domain::entities::ConversationFolderReference) -> Self {
        Self {
            id: reference.id.as_str(),
            conversation_id: reference.conversation_id.as_str(),
            folder_path: reference.folder_path,
            display_name: reference.display_name,
            created_at: reference.created_at.to_rfc3339(),
        }
    }
}

fn service(state: &AppState) -> ConversationFolderReferenceService {
    ConversationFolderReferenceService::new(
        Arc::clone(&state.conversation_folder_reference_repo),
        state.app_paths.app_data_dir().to_path_buf(),
        limits_config().max_live_folder_references,
    )
}

#[tauri::command]
pub async fn add_conversation_folder_reference(
    input: AddConversationFolderReferenceInput,
    state: State<'_, AppState>,
) -> Result<ConversationFolderReferenceResponse, AppError> {
    add_conversation_folder_reference_for_state(input, state.inner()).await
}

#[doc(hidden)]
pub async fn add_conversation_folder_reference_for_state(
    input: AddConversationFolderReferenceInput,
    state: &AppState,
) -> Result<ConversationFolderReferenceResponse, AppError> {
    let conversation_id = ChatConversationId::from_string(input.conversation_id);
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Conversation {conversation_id} was not found"))
        })?;
    let is_builder = conversation.is_persona_builder();
    if conversation.context_type != ChatContextType::Project && !is_builder {
        return Err(AppError::ConversationFolderReferenceUnsupportedContext);
    }
    service(state)
        .add(
            conversation_id,
            &PathBuf::from(input.folder_path),
            input.display_name,
        )
        .await
        .map(Into::into)
}

#[tauri::command]
pub async fn remove_conversation_folder_reference(
    input: RemoveConversationFolderReferenceInput,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    remove_conversation_folder_reference_for_state(input, state.inner()).await
}

#[doc(hidden)]
pub async fn remove_conversation_folder_reference_for_state(
    input: RemoveConversationFolderReferenceInput,
    state: &AppState,
) -> Result<(), AppError> {
    service(state)
        .remove(
            &ConversationFolderReferenceId::from_string(input.folder_reference_id),
            &ChatConversationId::from_string(input.conversation_id),
        )
        .await
}

#[tauri::command]
pub async fn list_conversation_folder_references(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ConversationFolderReferenceResponse>, AppError> {
    list_conversation_folder_references_for_state(conversation_id, state.inner()).await
}

#[doc(hidden)]
pub async fn list_conversation_folder_references_for_state(
    conversation_id: String,
    state: &AppState,
) -> Result<Vec<ConversationFolderReferenceResponse>, AppError> {
    service(state)
        .list_live(&ChatConversationId::from_string(conversation_id))
        .await
        .map(|items| items.into_iter().map(Into::into).collect())
}
