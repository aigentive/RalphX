use crate::application::chat_attachment_service::ChatAttachmentService;
use crate::application::personas::PersonaService;
use crate::application::standalone_workspace::remove_workspace_if_present;
use crate::application::AppState;
use crate::domain::entities::{ChatConversationId, PersonaId, PersonaStatus};
use crate::error::{AppError, AppResult};

/// Deletes a never-started seeded conversation and every resource created while
/// preparing its first send.
pub async fn abort_seeded_agent_conversation(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<()> {
    let conversation = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Conversation not found: {conversation_id}")))?;
    let has_messages = !state
        .chat_message_repo
        .get_by_conversation(conversation_id)
        .await?
        .is_empty();
    let has_runs = !state
        .agent_run_repo
        .get_by_conversation(conversation_id)
        .await?
        .is_empty();
    if has_messages
        || has_runs
        || conversation.provider_session_id.is_some()
        || conversation.claude_session_id.is_some()
    {
        return Err(AppError::SeededAgentConversationAlreadyStarted {
            conversation_id: conversation_id.as_str(),
        });
    }

    let bound_draft_id = if let Some(draft_id) = conversation.builder_draft_id.as_deref() {
        let draft_id = PersonaId::from(draft_id);
        let draft = state
            .persona_repo
            .get_by_id(&draft_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Persona draft not found: {draft_id}")))?;
        if draft.status != PersonaStatus::Draft {
            return Err(AppError::Validation(format!(
                "Bound persona {draft_id} must remain a draft during seeded abort"
            )));
        }
        Some(draft_id)
    } else {
        None
    };

    let attachment_service = ChatAttachmentService::new(
        state.chat_attachment_repo.clone(),
        state.attachment_storage_path.clone(),
    );
    for attachment in attachment_service
        .list_for_conversation(conversation_id)
        .await?
    {
        attachment_service.delete(&attachment.id).await?;
    }
    state
        .conversation_folder_reference_repo
        .delete_by_conversation_id(conversation_id)
        .await?;
    if let Some(draft_id) = bound_draft_id.as_ref() {
        PersonaService::new(
            state.db.clone(),
            state.persona_repo.clone(),
            state.chat_conversation_repo.clone(),
        )
        .hard_delete_draft(true, draft_id)
        .await?;
    }
    remove_workspace_if_present(state.app_paths.app_data_dir(), &conversation_id.as_str())?;
    state.chat_conversation_repo.delete(conversation_id).await
}
