use async_trait::async_trait;

use crate::entities::{
    ChatConversationId, ConversationFolderReference, ConversationFolderReferenceId,
};
use crate::error::AppResult;

#[async_trait]
pub trait ConversationFolderReferenceRepository: Send + Sync {
    async fn create_if_below_live_cap(
        &self,
        reference: ConversationFolderReference,
        max_live_references: usize,
    ) -> AppResult<ConversationFolderReference>;

    async fn list_live(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ConversationFolderReference>>;

    async fn soft_remove(
        &self,
        id: &ConversationFolderReferenceId,
        conversation_id: &ChatConversationId,
    ) -> AppResult<bool>;

    async fn delete_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()>;
}
