use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::{
    ChatConversationId, ConversationFolderReference, ConversationFolderReferenceId,
};
use crate::domain::repositories::ConversationFolderReferenceRepository;
use crate::error::{AppError, AppResult};

#[derive(Default)]
pub struct MemoryConversationFolderReferenceRepository {
    references: RwLock<HashMap<String, ConversationFolderReference>>,
}

impl MemoryConversationFolderReferenceRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ConversationFolderReferenceRepository for MemoryConversationFolderReferenceRepository {
    async fn create_if_below_live_cap(
        &self,
        reference: ConversationFolderReference,
        max_live_references: usize,
    ) -> AppResult<ConversationFolderReference> {
        let mut references = self.references.write().expect("folder references lock");
        if references.values().any(|item| {
            item.conversation_id == reference.conversation_id
                && item.folder_path == reference.folder_path
                && item.removed_at.is_none()
        }) {
            return Err(AppError::ConversationFolderReferenceDuplicate {
                conversation_id: reference.conversation_id.as_str(),
                folder_path: reference.folder_path,
            });
        }
        let live_count = references
            .values()
            .filter(|item| {
                item.conversation_id == reference.conversation_id && item.removed_at.is_none()
            })
            .count();
        if live_count >= max_live_references {
            return Err(AppError::ConversationFolderReferenceLimit {
                conversation_id: reference.conversation_id.as_str(),
                limit: max_live_references,
            });
        }
        references.insert(reference.id.as_str(), reference.clone());
        Ok(reference)
    }

    async fn list_live(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ConversationFolderReference>> {
        let mut result: Vec<_> = self
            .references
            .read()
            .expect("folder references lock")
            .values()
            .filter(|item| item.conversation_id == *conversation_id && item.removed_at.is_none())
            .cloned()
            .collect();
        result.sort_by_key(|item| item.created_at);
        Ok(result)
    }

    async fn soft_remove(
        &self,
        id: &ConversationFolderReferenceId,
        conversation_id: &ChatConversationId,
    ) -> AppResult<bool> {
        let mut references = self.references.write().expect("folder references lock");
        let Some(reference) = references.get_mut(&id.as_str()) else {
            return Ok(false);
        };
        if reference.conversation_id != *conversation_id || reference.removed_at.is_some() {
            return Ok(false);
        }
        reference.removed_at = Some(Utc::now());
        Ok(true)
    }

    async fn delete_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        self.references
            .write()
            .expect("folder references lock")
            .retain(|_, reference| reference.conversation_id != *conversation_id);
        Ok(())
    }
}
