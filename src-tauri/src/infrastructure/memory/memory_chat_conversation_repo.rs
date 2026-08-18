// In-memory implementation of ChatConversationRepository for testing

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use tokio::sync::RwLock;

use crate::domain::agents::ProviderSessionRef;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AttributionBackfillStatus, AutomationId, ChatContextType,
    ChatConversation, ChatConversationId, ConversationAttributionBackfillState,
    ConversationAttributionBackfillSummary, CoordinationMode,
};
use crate::domain::repositories::{ChatConversationPage, ChatConversationRepository};
use crate::error::{AppError, AppResult};

/// In-memory implementation of ChatConversationRepository for testing
pub struct MemoryChatConversationRepository {
    conversations: RwLock<HashMap<ChatConversationId, ChatConversation>>,
    failed_get_by_ids: RwLock<HashSet<ChatConversationId>>,
}

impl MemoryChatConversationRepository {
    pub fn new() -> Self {
        Self {
            conversations: RwLock::new(HashMap::new()),
            failed_get_by_ids: RwLock::new(HashSet::new()),
        }
    }

    pub async fn fail_get_by_id(&self, id: ChatConversationId) {
        self.failed_get_by_ids.write().await.insert(id);
    }
}

impl Default for MemoryChatConversationRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatConversationRepository for MemoryChatConversationRepository {
    async fn create(&self, conversation: ChatConversation) -> AppResult<ChatConversation> {
        if conversation.context_type == ChatContextType::Standalone
            && !conversation.is_valid_standalone_self_key()
        {
            return Err(AppError::Validation(
                "Standalone conversation context_id must equal its conversation id".to_string(),
            ));
        }
        let mut convos = self.conversations.write().await;
        convos.insert(conversation.id, conversation.clone());
        Ok(conversation)
    }

    async fn get_by_id(&self, id: &ChatConversationId) -> AppResult<Option<ChatConversation>> {
        if self.failed_get_by_ids.read().await.contains(id) {
            return Err(AppError::Infrastructure(format!(
                "injected conversation lookup failure for {id}"
            )));
        }
        let convos = self.conversations.read().await;
        Ok(convos.get(id).cloned())
    }

    async fn get_by_builder_draft_id(
        &self,
        builder_draft_id: &str,
    ) -> AppResult<Option<ChatConversation>> {
        Ok(self
            .conversations
            .read()
            .await
            .values()
            .filter(|conversation| {
                !conversation.is_archived()
                    && conversation.builder_draft_id.as_deref() == Some(builder_draft_id)
            })
            .max_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.id.as_str().cmp(&right.id.as_str()))
            })
            .cloned())
    }

    async fn get_by_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<Vec<ChatConversation>> {
        let convos = self.conversations.read().await;
        let filtered: Vec<ChatConversation> = convos
            .values()
            .filter(|c| {
                c.context_type == context_type && c.context_id == context_id && !c.is_archived()
            })
            .cloned()
            .collect();
        Ok(filtered)
    }

    async fn list_by_automation_id(
        &self,
        automation_id: &AutomationId,
    ) -> AppResult<Vec<ChatConversation>> {
        let convos = self.conversations.read().await;
        let mut filtered: Vec<ChatConversation> = convos
            .values()
            .filter(|c| c.automation_id.as_ref() == Some(automation_id))
            .cloned()
            .collect();
        filtered.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(filtered)
    }

    async fn get_by_context_filtered(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        include_archived: bool,
    ) -> AppResult<Vec<ChatConversation>> {
        let convos = self.conversations.read().await;
        let filtered: Vec<ChatConversation> = convos
            .values()
            .filter(|c| {
                c.context_type == context_type
                    && c.context_id == context_id
                    && (include_archived || !c.is_archived())
            })
            .cloned()
            .collect();
        Ok(filtered)
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
        let normalized_search = search
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_lowercase());
        let convos = self.conversations.read().await;
        let mut filtered: Vec<ChatConversation> = convos
            .values()
            .filter(|conversation| {
                conversation.context_type == context_type
                    && conversation.context_id == context_id
                    && if archived_only {
                        conversation.is_archived()
                    } else {
                        include_archived || !conversation.is_archived()
                    }
                    && normalized_search.as_ref().map_or(true, |term| {
                        conversation
                            .title
                            .as_deref()
                            .unwrap_or("Untitled agent")
                            .to_lowercase()
                            .contains(term)
                    })
            })
            .cloned()
            .collect();
        filtered.sort_by(|left, right| right.created_at.cmp(&left.created_at));

        let total_count = filtered.len() as i64;
        let start = offset as usize;
        let end = start.saturating_add(limit as usize).min(filtered.len());
        let conversations = if start >= filtered.len() {
            Vec::new()
        } else {
            filtered[start..end].to_vec()
        };

        Ok(ChatConversationPage {
            conversations,
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
        let convos = self.conversations.read().await;
        Ok(convos
            .values()
            .filter(|c| {
                c.context_type == context_type && c.context_id == context_id && !c.is_archived()
            })
            .max_by_key(|c| c.created_at)
            .cloned())
    }

    async fn list_recent_resumable_by_context_type(
        &self,
        context_type: ChatContextType,
        limit: u32,
    ) -> AppResult<Vec<ChatConversation>> {
        let convos = self.conversations.read().await;
        let mut filtered: Vec<ChatConversation> = convos
            .values()
            .filter(|conversation| {
                conversation.context_type == context_type
                    && !conversation.is_archived()
                    && (conversation.provider_session_id.is_some()
                        || conversation.claude_session_id.is_some())
            })
            .cloned()
            .collect();
        filtered.sort_by(|left, right| {
            right
                .last_message_at
                .unwrap_or(right.updated_at)
                .cmp(&left.last_message_at.unwrap_or(left.updated_at))
        });
        filtered.truncate(limit as usize);
        Ok(filtered)
    }

    async fn list_by_context_type(
        &self,
        context_type: ChatContextType,
        include_archived: bool,
        limit: u32,
    ) -> AppResult<Vec<ChatConversation>> {
        let convos = self.conversations.read().await;
        let mut filtered: Vec<ChatConversation> = convos
            .values()
            .filter(|conversation| {
                conversation.context_type == context_type
                    && (include_archived || !conversation.is_archived())
            })
            .cloned()
            .collect();
        filtered.sort_by(|left, right| {
            right
                .last_message_at
                .unwrap_or(right.updated_at)
                .cmp(&left.last_message_at.unwrap_or(left.updated_at))
        });
        filtered.truncate(limit as usize);
        Ok(filtered)
    }

    async fn update_provider_session_ref(
        &self,
        id: &ChatConversationId,
        session_ref: &ProviderSessionRef,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conv) = convos.get_mut(id) {
            conv.set_provider_session_ref(session_ref.clone());
        }
        Ok(())
    }

    async fn refresh_provider_session_ref(
        &self,
        id: &ChatConversationId,
        session_ref: &ProviderSessionRef,
    ) -> AppResult<bool> {
        let mut convos = self.conversations.write().await;
        let Some(conv) = convos.get_mut(id) else {
            return Ok(false);
        };
        if conv.provider_session_ref().is_none() {
            return Ok(false);
        }
        conv.set_provider_session_ref(session_ref.clone());
        Ok(true)
    }

    async fn clear_provider_session_ref(&self, id: &ChatConversationId) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conversation) = convos.get_mut(id) {
            conversation.clear_provider_session_ref();
        }
        Ok(())
    }

    async fn update_provider_origin(
        &self,
        id: &ChatConversationId,
        upstream_provider: Option<&str>,
        provider_profile: Option<&str>,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conversation) = convos.get_mut(id) {
            conversation.set_provider_origin(
                upstream_provider.map(str::to_string),
                provider_profile.map(str::to_string),
            );
        }
        Ok(())
    }

    async fn update_agent_mode(
        &self,
        id: &ChatConversationId,
        mode: Option<AgentConversationWorkspaceMode>,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conversation) = convos.get_mut(id) {
            conversation.set_agent_mode(mode);
        }
        Ok(())
    }

    async fn update_bound_agent_name(
        &self,
        id: &ChatConversationId,
        bound_agent_name: Option<&str>,
    ) -> AppResult<()> {
        let mut conversations = self.conversations.write().await;
        let conversation = conversations.get_mut(id).ok_or_else(|| {
            crate::error::AppError::NotFound("Chat conversation not found".to_string())
        })?;
        conversation.bound_agent_name = bound_agent_name.map(str::to_string);
        conversation.updated_at = Utc::now();
        Ok(())
    }

    async fn update_persona_binding(
        &self,
        id: &ChatConversationId,
        persona_id: Option<&str>,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conversation) = convos.get_mut(id) {
            conversation.persona_id = persona_id.map(str::to_string);
            conversation.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_builder_draft_binding(
        &self,
        id: &ChatConversationId,
        builder_draft_id: Option<&str>,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conversation) = convos.get_mut(id) {
            conversation.builder_draft_id = builder_draft_id.map(str::to_string);
            conversation.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_coordination_mode(
        &self,
        id: &ChatConversationId,
        mode: CoordinationMode,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conversation) = convos.get_mut(id) {
            conversation.set_coordination_mode(mode);
        }
        Ok(())
    }

    async fn update_role_default_bindings(
        &self,
        id: &ChatConversationId,
        mode: CoordinationMode,
        persona_id: Option<&str>,
        clear_provider_session: bool,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conversation) = convos.get_mut(id) {
            conversation.coordination_mode = mode;
            conversation.persona_id = persona_id.map(str::to_string);
            if clear_provider_session {
                conversation.claude_session_id = None;
                conversation.provider_session_id = None;
                conversation.provider_harness = None;
            }
            conversation.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_agent_mode_and_role_default_bindings(
        &self,
        id: &ChatConversationId,
        agent_mode: AgentConversationWorkspaceMode,
        coordination_mode: CoordinationMode,
        persona_id: Option<&str>,
        clear_provider_session: bool,
    ) -> AppResult<()> {
        let mut conversations = self.conversations.write().await;
        let conversation = conversations.get_mut(id).ok_or_else(|| {
            crate::error::AppError::NotFound("Chat conversation not found".to_string())
        })?;
        conversation.agent_mode = Some(agent_mode);
        conversation.coordination_mode = coordination_mode;
        conversation.persona_id = persona_id.map(str::to_string);
        if clear_provider_session {
            conversation.claude_session_id = None;
            conversation.provider_session_id = None;
            conversation.provider_harness = None;
        }
        conversation.updated_at = Utc::now();
        Ok(())
    }

    async fn update_title(&self, id: &ChatConversationId, title: &str) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conv) = convos.get_mut(id) {
            conv.title = Some(title.to_string());
            conv.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn archive(&self, id: &ChatConversationId) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conv) = convos.get_mut(id) {
            conv.archive();
        }
        Ok(())
    }

    async fn restore(&self, id: &ChatConversationId) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conv) = convos.get_mut(id) {
            conv.restore();
        }
        Ok(())
    }

    async fn update_message_stats(
        &self,
        id: &ChatConversationId,
        message_count: i64,
        last_message_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conv) = convos.get_mut(id) {
            conv.message_count = message_count;
            conv.last_message_at = Some(last_message_at);
            conv.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn list_needing_attribution_backfill(
        &self,
        limit: u32,
    ) -> AppResult<Vec<ChatConversation>> {
        let convos = self.conversations.read().await;
        let mut filtered: Vec<ChatConversation> = convos
            .values()
            .filter(|conversation| {
                conversation.claude_session_id.is_some()
                    && matches!(
                        conversation.attribution_backfill_status,
                        None | Some(crate::domain::entities::AttributionBackfillStatus::Pending)
                    )
            })
            .cloned()
            .collect();
        filtered.sort_by_key(|conversation| {
            conversation
                .attribution_backfill_last_attempted_at
                .unwrap_or(conversation.created_at)
        });
        filtered.truncate(limit as usize);
        Ok(filtered)
    }

    async fn reset_running_attribution_backfill_to_pending(&self) -> AppResult<u64> {
        let mut convos = self.conversations.write().await;
        let mut updated = 0u64;
        for conversation in convos.values_mut() {
            if conversation.claude_session_id.is_some()
                && matches!(
                    conversation.attribution_backfill_status,
                    Some(AttributionBackfillStatus::Running)
                )
            {
                conversation.attribution_backfill_status = Some(AttributionBackfillStatus::Pending);
                conversation.attribution_backfill_completed_at = None;
                conversation.updated_at = Utc::now();
                updated += 1;
            }
        }
        Ok(updated)
    }

    async fn update_attribution_backfill_state(
        &self,
        id: &ChatConversationId,
        state: ConversationAttributionBackfillState,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        if let Some(conversation) = convos.get_mut(id) {
            conversation.update_attribution_backfill_state(state);
        }
        Ok(())
    }

    async fn get_attribution_backfill_summary(
        &self,
    ) -> AppResult<ConversationAttributionBackfillSummary> {
        let convos = self.conversations.read().await;
        let mut summary = ConversationAttributionBackfillSummary::default();

        for conversation in convos.values() {
            if conversation.claude_session_id.is_none() {
                continue;
            }

            summary.eligible_conversation_count += 1;
            match conversation.attribution_backfill_status {
                None | Some(AttributionBackfillStatus::Pending) => summary.pending_count += 1,
                Some(AttributionBackfillStatus::Running) => summary.running_count += 1,
                Some(AttributionBackfillStatus::Completed) => summary.completed_count += 1,
                Some(AttributionBackfillStatus::Partial) => summary.partial_count += 1,
                Some(AttributionBackfillStatus::SessionNotFound) => {
                    summary.session_not_found_count += 1;
                }
                Some(AttributionBackfillStatus::ParseFailed) => summary.parse_failed_count += 1,
            }
        }

        Ok(summary)
    }

    async fn delete(&self, id: &ChatConversationId) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        convos.remove(id);
        Ok(())
    }

    async fn delete_by_context(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> AppResult<()> {
        let mut convos = self.conversations.write().await;
        convos.retain(|_, c| !(c.context_type == context_type && c.context_id == context_id));
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_chat_conversation_repo_tests.rs"]
mod tests;
