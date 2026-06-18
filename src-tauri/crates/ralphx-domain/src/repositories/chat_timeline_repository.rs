use async_trait::async_trait;

use crate::domain::entities::{
    ChatConversationId, ChatMessageId, ChatTimelineItem, ChatTimelineItemId, ChatTimelinePage,
};
use crate::error::AppResult;

#[async_trait]
pub trait ChatTimelineRepository: Send + Sync {
    async fn upsert_item(&self, item: ChatTimelineItem) -> AppResult<ChatTimelineItem>;

    async fn get_by_id(&self, id: &ChatTimelineItemId) -> AppResult<Option<ChatTimelineItem>>;

    async fn get_page(
        &self,
        conversation_id: &ChatConversationId,
        limit: u32,
        before_sequence: Option<i64>,
    ) -> AppResult<ChatTimelinePage>;

    async fn count_by_conversation(&self, conversation_id: &ChatConversationId) -> AppResult<u32>;

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatTimelineItem>>;

    async fn mark_message_items_finalized(&self, message_id: &ChatMessageId) -> AppResult<()>;
}
