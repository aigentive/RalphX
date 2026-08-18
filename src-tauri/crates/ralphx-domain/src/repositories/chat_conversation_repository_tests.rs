use super::*;
use crate::agents::ProviderSessionRef;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation,
    ConversationAttributionBackfillState, ConversationAttributionBackfillSummary, CoordinationMode,
    IdeationSessionId,
};
use std::sync::Arc;

// Mock implementation for testing trait object usage
struct MockChatConversationRepository {
    conversations: Vec<ChatConversation>,
}

impl MockChatConversationRepository {
    fn new() -> Self {
        Self {
            conversations: vec![],
        }
    }

    fn with_conversations(conversations: Vec<ChatConversation>) -> Self {
        Self { conversations }
    }
}

#[async_trait]
impl ChatConversationRepository for MockChatConversationRepository {
    async fn create(&self, conversation: ChatConversation) -> AppResult<ChatConversation> {
        Ok(conversation)
    }

    async fn get_by_id(&self, id: &ChatConversationId) -> AppResult<Option<ChatConversation>> {
        Ok(self.conversations.iter().find(|c| c.id == *id).cloned())
    }

    async fn get_by_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<Vec<ChatConversation>> {
        self.get_by_context_filtered(context_type, context_id, false)
            .await
    }

    async fn get_by_context_filtered(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        include_archived: bool,
    ) -> AppResult<Vec<ChatConversation>> {
        Ok(self
            .conversations
            .iter()
            .filter(|c| {
                c.context_type == context_type
                    && c.context_id == context_id
                    && (include_archived || c.archived_at.is_none())
            })
            .cloned()
            .collect())
    }

    async fn list_by_automation_id(
        &self,
        automation_id: &crate::entities::AutomationId,
    ) -> AppResult<Vec<ChatConversation>> {
        Ok(self
            .conversations
            .iter()
            .filter(|c| c.automation_id.as_ref() == Some(automation_id))
            .cloned()
            .collect())
    }

    async fn get_by_context_page_filtered(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        include_archived: bool,
        archived_only: bool,
        offset: u32,
        limit: u32,
        search: Option<&str>,
    ) -> AppResult<ChatConversationPage> {
        let mut conversations: Vec<ChatConversation> = self
            .conversations
            .iter()
            .filter(|conversation| {
                conversation.context_type == context_type
                    && conversation.context_id == context_id
                    && (include_archived || conversation.archived_at.is_none())
                    && (!archived_only || conversation.archived_at.is_some())
                    && search.is_none_or(|term| {
                        let normalized = term.trim().to_lowercase();
                        let title = conversation
                            .title
                            .as_deref()
                            .unwrap_or("Untitled agent")
                            .to_lowercase();
                        title.contains(&normalized)
                    })
            })
            .cloned()
            .collect();
        conversations.sort_by(|left, right| right.created_at.cmp(&left.created_at));

        let total_count = conversations.len() as i64;
        let start = offset as usize;
        let end = start
            .saturating_add(limit as usize)
            .min(conversations.len());
        let page_conversations = if start >= conversations.len() {
            Vec::new()
        } else {
            conversations[start..end].to_vec()
        };

        Ok(ChatConversationPage {
            conversations: page_conversations,
            total_count,
            offset,
            limit,
        })
    }

    async fn get_active_for_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<Option<ChatConversation>> {
        Ok(self
            .conversations
            .iter()
            .filter(|c| {
                c.context_type == context_type
                    && c.context_id == context_id
                    && c.archived_at.is_none()
            })
            .max_by_key(|c| c.created_at)
            .cloned())
    }

    async fn list_recent_resumable_by_context_type(
        &self,
        context_type: ChatContextType,
        limit: u32,
    ) -> AppResult<Vec<ChatConversation>> {
        let mut conversations: Vec<ChatConversation> = self
            .conversations
            .iter()
            .filter(|conversation| {
                conversation.context_type == context_type
                    && conversation.archived_at.is_none()
                    && (conversation.provider_session_id.is_some()
                        || conversation.claude_session_id.is_some())
            })
            .cloned()
            .collect();
        conversations.sort_by(|left, right| {
            right
                .last_message_at
                .unwrap_or(right.updated_at)
                .cmp(&left.last_message_at.unwrap_or(left.updated_at))
        });
        conversations.truncate(limit as usize);
        Ok(conversations)
    }

    async fn update_provider_session_ref(
        &self,
        _id: &ChatConversationId,
        _session_ref: &ProviderSessionRef,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn refresh_provider_session_ref(
        &self,
        _id: &ChatConversationId,
        _session_ref: &ProviderSessionRef,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn clear_provider_session_ref(&self, _id: &ChatConversationId) -> AppResult<()> {
        Ok(())
    }

    async fn update_provider_origin(
        &self,
        _id: &ChatConversationId,
        _upstream_provider: Option<&str>,
        _provider_profile: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_agent_mode(
        &self,
        _id: &ChatConversationId,
        _mode: Option<AgentConversationWorkspaceMode>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_bound_agent_name(
        &self,
        _id: &ChatConversationId,
        _bound_agent_name: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_persona_binding(
        &self,
        _id: &ChatConversationId,
        _persona_id: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_builder_draft_binding(
        &self,
        _id: &ChatConversationId,
        _builder_draft_id: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_coordination_mode(
        &self,
        _id: &ChatConversationId,
        _mode: CoordinationMode,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_role_default_bindings(
        &self,
        _id: &ChatConversationId,
        _mode: CoordinationMode,
        _persona_id: Option<&str>,
        _clear_provider_session: bool,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_agent_mode_and_role_default_bindings(
        &self,
        _id: &ChatConversationId,
        _agent_mode: AgentConversationWorkspaceMode,
        _coordination_mode: CoordinationMode,
        _persona_id: Option<&str>,
        _clear_provider_session: bool,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_title(&self, _id: &ChatConversationId, _title: &str) -> AppResult<()> {
        Ok(())
    }

    async fn archive(&self, _id: &ChatConversationId) -> AppResult<()> {
        Ok(())
    }

    async fn restore(&self, _id: &ChatConversationId) -> AppResult<()> {
        Ok(())
    }

    async fn update_message_stats(
        &self,
        _id: &ChatConversationId,
        _message_count: i64,
        _last_message_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn list_needing_attribution_backfill(
        &self,
        _limit: u32,
    ) -> AppResult<Vec<ChatConversation>> {
        Ok(self.conversations.clone())
    }

    async fn reset_running_attribution_backfill_to_pending(&self) -> AppResult<u64> {
        Ok(0)
    }

    async fn update_attribution_backfill_state(
        &self,
        _id: &ChatConversationId,
        _state: ConversationAttributionBackfillState,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_attribution_backfill_summary(
        &self,
    ) -> AppResult<ConversationAttributionBackfillSummary> {
        Ok(ConversationAttributionBackfillSummary::default())
    }

    async fn delete(&self, _id: &ChatConversationId) -> AppResult<()> {
        Ok(())
    }

    async fn delete_by_context(
        &self,
        _context_type: ChatContextType,
        _context_id: &str,
    ) -> AppResult<()> {
        Ok(())
    }
}

#[test]
fn test_trait_object_safety() {
    let repo = MockChatConversationRepository::new();
    let _: Arc<dyn ChatConversationRepository> = Arc::new(repo);
}

#[test]
fn test_mock_with_conversations() {
    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id);
    let repo = MockChatConversationRepository::with_conversations(vec![conv.clone()]);

    assert_eq!(repo.conversations.len(), 1);
    assert_eq!(repo.conversations[0].id, conv.id);
}
