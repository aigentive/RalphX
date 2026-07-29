use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::{AgentConversationMute, ChatConversationId};
use crate::domain::repositories::AgentConversationMuteRepository;
use crate::error::AppResult;

pub struct MemoryAgentConversationMuteRepository {
    mutes: RwLock<HashMap<ChatConversationId, AgentConversationMute>>,
}

impl MemoryAgentConversationMuteRepository {
    pub fn new() -> Self {
        Self {
            mutes: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryAgentConversationMuteRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentConversationMuteRepository for MemoryAgentConversationMuteRepository {
    async fn set_muted(&self, mute: AgentConversationMute) -> AppResult<()> {
        self.mutes
            .write()
            .await
            .insert(mute.conversation_id.clone(), mute);
        Ok(())
    }

    async fn clear(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.mutes.write().await.remove(conversation_id);
        Ok(())
    }

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationMute>> {
        Ok(self.mutes.read().await.get(conversation_id).cloned())
    }

    async fn list_by_conversation_ids(
        &self,
        conversation_ids: &[ChatConversationId],
    ) -> AppResult<Vec<AgentConversationMute>> {
        let mutes = self.mutes.read().await;
        Ok(conversation_ids
            .iter()
            .filter_map(|conversation_id| mutes.get(conversation_id).cloned())
            .collect())
    }
}
