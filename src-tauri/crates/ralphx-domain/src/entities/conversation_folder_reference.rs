use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ChatConversationId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConversationFolderReferenceId(Uuid);

impl ConversationFolderReferenceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_string(value: impl AsRef<str>) -> Self {
        Self(Uuid::parse_str(value.as_ref()).unwrap_or_else(|_| Uuid::nil()))
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for ConversationFolderReferenceId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationFolderReference {
    pub id: ConversationFolderReferenceId,
    pub conversation_id: ChatConversationId,
    pub folder_path: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    pub removed_at: Option<DateTime<Utc>>,
}

impl ConversationFolderReference {
    pub fn new(
        conversation_id: ChatConversationId,
        folder_path: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        Self {
            id: ConversationFolderReferenceId::new(),
            conversation_id,
            folder_path: folder_path.into(),
            display_name: display_name.into(),
            created_at: Utc::now(),
            removed_at: None,
        }
    }
}
