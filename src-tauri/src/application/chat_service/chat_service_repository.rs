// Repository Operations for ChatService
//
// Helper functions for interacting with repositories in the ChatService implementation.

use std::sync::Arc;

use crate::domain::entities::{
    ChatContextType, ChatConversation, ChatConversationId, IdeationSessionId, ProjectId, TaskId,
};
use crate::domain::repositories::{ChatConversationRepository, ChatMessageRepository};

use super::chat_service_types::{ChatConversationWithMessages, ChatServiceError};

/// Get or create a conversation for a context.
/// Returns `(conversation, is_new)` where `is_new` is `true` when a new conversation was created.
pub async fn get_or_create_conversation(
    conversation_repo: Arc<dyn ChatConversationRepository>,
    context_type: ChatContextType,
    context_id: &str,
) -> Result<(ChatConversation, bool), ChatServiceError> {
    // TaskExecution + Merge: ALWAYS create a fresh conversation — never reuse prior run context.
    // - TaskExecution: Worker agents reconstruct context via MCP tools (get_task_context, etc.).
    // - Merge: Each merge attempt (conflict resolution or validation recovery) needs a fresh
    //   provider session. Reusing a stale session causes the agent to resume dead context.
    //   Mirrors is_fresh_review_cycle logic in chat_service_context.rs.
    let force_fresh =
        context_type == ChatContextType::TaskExecution || context_type == ChatContextType::Merge;
    if !force_fresh {
        // Try to get existing active conversation
        if let Some(conv) = conversation_repo
            .get_active_for_context(context_type, context_id)
            .await
            .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?
        {
            return Ok((conv, false));
        }
    }

    // For force-fresh contexts, look up the most recent prior conversation to set as parent.
    let parent_conversation_id = if force_fresh {
        conversation_repo
            .get_active_for_context(context_type, context_id)
            .await
            .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?
            .map(|c| c.id.as_str())
    } else {
        None
    };

    // Create new conversation based on context type
    let mut conv = match context_type {
        ChatContextType::Ideation => {
            ChatConversation::new_ideation(IdeationSessionId::from_string(context_id))
        }
        ChatContextType::Delegation => ChatConversation::new_delegation(
            crate::domain::entities::DelegatedSessionId::from_string(context_id),
        ),
        ChatContextType::Task => {
            ChatConversation::new_task(TaskId::from_string(context_id.to_string()))
        }
        ChatContextType::Project => {
            ChatConversation::new_project(ProjectId::from_string(context_id.to_string()))
        }
        ChatContextType::TaskExecution => {
            ChatConversation::new_task_execution(TaskId::from_string(context_id.to_string()))
        }
        ChatContextType::Review => {
            ChatConversation::new_review(TaskId::from_string(context_id.to_string()))
        }
        ChatContextType::Merge => {
            ChatConversation::new_merge(TaskId::from_string(context_id.to_string()))
        }
        ChatContextType::BranchUpdate => {
            ChatConversation::new_branch_update(TaskId::from_string(context_id.to_string()))
        }
        ChatContextType::Standalone => {
            // Standalone conversations are always explicitly created (via
            // `create_agent_conversation` or `AgentConversationStartService::start()`,
            // both using `ChatConversation::new_standalone()`), never lazily
            // auto-vivified from a bare context_id: a standalone conversation's
            // context_id IS its own conversation id, so if `get_active_for_context`
            // above didn't find an active row for it, the id is stale/archived/
            // unknown — fabricating a new conversation here would silently diverge
            // from the conversation the caller actually meant.
            return Err(ChatServiceError::ContextNotFound(format!(
                "Standalone conversation {context_id} is not active; standalone conversations \
                 cannot be lazily created from a bare context_id"
            )));
        }
    };

    conv.parent_conversation_id = parent_conversation_id;

    let created = conversation_repo
        .create(conv.clone())
        .await
        .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?;
    Ok((created, true))
}

/// Get a conversation by ID with all its messages
pub async fn get_conversation_with_messages(
    conversation_repo: Arc<dyn ChatConversationRepository>,
    chat_message_repo: Arc<dyn ChatMessageRepository>,
    conversation_id: &ChatConversationId,
) -> Result<Option<ChatConversationWithMessages>, ChatServiceError> {
    let conversation = match conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?
    {
        Some(c) => c,
        None => return Ok(None),
    };

    let messages = chat_message_repo
        .get_by_conversation(conversation_id)
        .await
        .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?;

    Ok(Some(ChatConversationWithMessages {
        conversation,
        messages,
    }))
}

/// List all conversations for a context
pub async fn list_conversations(
    conversation_repo: Arc<dyn ChatConversationRepository>,
    context_type: ChatContextType,
    context_id: &str,
) -> Result<Vec<ChatConversation>, ChatServiceError> {
    conversation_repo
        .get_by_context(context_type, context_id)
        .await
        .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))
}

#[cfg(test)]
#[path = "chat_service_repository_tests.rs"]
mod tests;
