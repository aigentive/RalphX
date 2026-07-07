// Chat conversation repository trait - domain layer abstraction
//
// This trait defines the contract for chat conversation persistence.
// Conversations track provider sessions linked to contexts (ideation, task, project).

use async_trait::async_trait;

use crate::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::entities::{
    AgentConversationWorkspaceMode, AutomationId, ChatContextType, ChatConversation,
    ChatConversationId, ConversationAttributionBackfillState, ConversationAttributionBackfillSummary,
};
use crate::error::AppResult;

#[derive(Clone, Debug)]
pub struct ChatConversationPage {
    pub conversations: Vec<ChatConversation>,
    pub total_count: i64,
    pub offset: u32,
    pub limit: u32,
}

impl ChatConversationPage {
    pub fn has_more(&self) -> bool {
        i64::from(self.offset) + (self.conversations.len() as i64) < self.total_count
    }
}

/// Repository trait for ChatConversation persistence.
/// Implementations can use SQLite, PostgreSQL, in-memory, etc.
#[async_trait]
pub trait ChatConversationRepository: Send + Sync {
    /// Create a new conversation
    async fn create(&self, conversation: ChatConversation) -> AppResult<ChatConversation>;

    /// Get conversation by ID
    async fn get_by_id(&self, id: &ChatConversationId) -> AppResult<Option<ChatConversation>>;

    /// Get all conversations for a specific context
    async fn get_by_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<Vec<ChatConversation>>;

    /// List all conversations owned by an automation (setup + run conversations),
    /// including archived rows, matched by the `automation_id` column.
    ///
    /// Run conversations set both `automation_id` and `automation_run_id`; setup
    /// conversations set only `automation_id`. Archived rows are intentionally
    /// included so the automation-delete flow can decide which conversations
    /// still need archiving.
    async fn list_by_automation_id(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<Vec<ChatConversation>>;

    /// Get all conversations for a specific context, optionally including archived rows.
    async fn get_by_context_filtered(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        include_archived: bool,
    ) -> AppResult<Vec<ChatConversation>>;

    /// Get a page of conversations for a specific context, optionally including
    /// archived rows and filtering by title.
    async fn get_by_context_page_filtered(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        include_archived: bool,
        archived_only: bool,
        offset: u32,
        limit: u32,
        search: Option<&str>,
    ) -> AppResult<ChatConversationPage>;

    /// Get the active (most recent) conversation for a context
    async fn get_active_for_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<Option<ChatConversation>>;

    /// List recent active conversations for a context type that still have a
    /// resumable provider session.
    async fn list_recent_resumable_by_context_type(
        &self,
        context_type: ChatContextType,
        limit: u32,
    ) -> AppResult<Vec<ChatConversation>>;

    /// Update the provider session reference for a conversation.
    async fn update_provider_session_ref(
        &self,
        id: &ChatConversationId,
        session_ref: &ProviderSessionRef,
    ) -> AppResult<()>;

    /// Clear the provider session reference for a conversation.
    async fn clear_provider_session_ref(&self, id: &ChatConversationId) -> AppResult<()>;

    /// Update provider-origin metadata for a conversation.
    async fn update_provider_origin(
        &self,
        id: &ChatConversationId,
        upstream_provider: Option<&str>,
        provider_profile: Option<&str>,
    ) -> AppResult<()>;

    /// Update the current Agents mode for a project conversation.
    async fn update_agent_mode(
        &self,
        id: &ChatConversationId,
        mode: Option<AgentConversationWorkspaceMode>,
    ) -> AppResult<()>;

    /// Compatibility helper for legacy Claude-specific callers.
    async fn update_claude_session_id(
        &self,
        id: &ChatConversationId,
        claude_session_id: &str,
    ) -> AppResult<()> {
        self.update_provider_session_ref(
            id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: claude_session_id.to_string(),
            },
        )
        .await
    }

    /// Compatibility helper for legacy Claude-specific callers.
    async fn clear_claude_session_id(&self, id: &ChatConversationId) -> AppResult<()> {
        self.clear_provider_session_ref(id).await
    }

    /// Update conversation title
    async fn update_title(&self, id: &ChatConversationId, title: &str) -> AppResult<()>;

    /// Archive a conversation.
    async fn archive(&self, id: &ChatConversationId) -> AppResult<()>;

    /// Restore an archived conversation.
    async fn restore(&self, id: &ChatConversationId) -> AppResult<()>;

    /// Update message count and last message timestamp
    /// This is typically called by a database trigger, but can be manually updated if needed
    async fn update_message_stats(
        &self,
        id: &ChatConversationId,
        message_count: i64,
        last_message_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<()>;

    /// List conversations backed by historical Claude sessions that still need
    /// attribution backfill work in the current pass.
    ///
    /// Automatic startup passes should only claim `pending` / unset work. Rows
    /// already marked `partial`, `session_not_found`, or `parse_failed` need an
    /// explicit repair/retry flow instead of being re-claimed forever.
    async fn list_needing_attribution_backfill(
        &self,
        limit: u32,
    ) -> AppResult<Vec<ChatConversation>>;

    /// Reset stale `running` attribution-backfill rows back to `pending`.
    ///
    /// This is used on startup so an interrupted prior import pass does not
    /// leave rows permanently excluded from future automatic runs.
    async fn reset_running_attribution_backfill_to_pending(&self) -> AppResult<u64>;

    /// Update attribution backfill workflow state for a conversation.
    async fn update_attribution_backfill_state(
        &self,
        id: &ChatConversationId,
        state: ConversationAttributionBackfillState,
    ) -> AppResult<()>;

    /// Return aggregate historical attribution-backfill workflow counts.
    async fn get_attribution_backfill_summary(
        &self,
    ) -> AppResult<ConversationAttributionBackfillSummary>;

    /// Delete a conversation and all its messages
    async fn delete(&self, id: &ChatConversationId) -> AppResult<()>;

    /// Delete all conversations for a context
    async fn delete_by_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<()>;
}

#[cfg(test)]
#[path = "chat_conversation_repository_tests.rs"]
mod tests;
